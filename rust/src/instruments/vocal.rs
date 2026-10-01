use std::f32::consts::TAU;

use super::{
    dsp::{release_fade, saw_blep, Noise, Svf},
    sane_sample_rate, NoteEvent, Voice,
};
use crate::core::midi_to_freq;

/// Вокальный чоп: пара пил с вибрато через формантные полосы.
/// Гласная — от ноты (ah/oh/eh): каждый чоп свой слог, а не один
/// вечный «ah» (статичная гласная = половина синтетичности).
/// Дыхание добавляет воздуха сибилянтам. Половину узнаваемости дает
/// сильный дилэй на мастере (сенд роли Vocal).
const DETUNE: f32 = 1.004;
const VIBRATO_HZ: f32 = 5.2;
const VIBRATO_DEPTH: f32 = 0.006;
/// Гласные (F1, F2): ah, oh, eh. Выбор — сидом ноты.
const VOWELS: [(f32, f32); 3] = [(730.0, 1090.0), (500.0, 860.0), (610.0, 1900.0)];
const FORMANT_Q: f32 = 3.5;
/// Доля дыхательного шума: слышно воздух, не шипение.
const BREATH_GAIN: f32 = 0.06;
const BREATH_HZ: f32 = 2400.0;
const ATTACK_SECS: f32 = 0.008;
const AMP_DECAY_RATE: f32 = 5.0;
const MAX_RING_SECS: f32 = 0.8;
const FADE_SECS: f32 = 0.04;
const VOICE_GAIN: f32 = 0.5;

#[derive(Clone, Default)]
pub struct VocalVoice {
    sample_rate: f32,
    phase1: f32,
    phase2: f32,
    vib_phase: f32,
    form1: Svf,
    form2: Svf,
    breath_bp: Svf,
    noise: Noise,
    frequency: f32,
    gain: f32,
    t: f32,
    samples_left: usize,
    pub(crate) f1_hz: f32,
    pub(crate) f2_hz: f32,
}

impl Voice for VocalVoice {
    fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sane_sample_rate(sample_rate);
    }

    fn trigger(&mut self, note: &NoteEvent) {
        self.frequency = midi_to_freq(note.midi_pitch);
        let ring = note.duration_secs.clamp(0.05, MAX_RING_SECS);
        self.samples_left = (ring * self.sample_rate) as usize;
        self.gain = note.velocity;
        self.t = 0.0;
        self.phase1 = 0.0;
        self.phase2 = 0.0;
        self.form1.reset();
        self.form2.reset();
        self.breath_bp.reset();
        // Гласная чопа — от сида ноты: разные ноты поют разные слоги.
        // Детерминировано: та же нота — тот же слог.
        let seed = super::dsp::note_seed(note, 0xB0CA);
        let (f1, f2) = VOWELS[(seed % VOWELS.len() as u32) as usize];
        self.f1_hz = f1;
        self.f2_hz = f2;
        self.noise = Noise::new(seed);
    }

    fn next_sample(&mut self) -> f32 {
        if self.samples_left == 0 {
            return 0.0;
        }
        self.samples_left -= 1;

        let sample_rate = self.sample_rate;
        let a = (self.t / ATTACK_SECS).min(1.0);
        let attack = a * a * (3.0 - 2.0 * a);
        let amp = (-AMP_DECAY_RATE * self.t).exp();
        let fade = release_fade(self.samples_left, (FADE_SECS * sample_rate) as usize);

        self.vib_phase = (self.vib_phase + VIBRATO_HZ / sample_rate) % 1.0;
        let wobble = 1.0 + VIBRATO_DEPTH * (self.vib_phase * TAU).sin();
        let freq = self.frequency * wobble;

        self.phase1 = (self.phase1 + freq / sample_rate) % 1.0;
        self.phase2 = (self.phase2 + (freq * DETUNE) / sample_rate) % 1.0;
        let raw = (saw_blep(self.phase1, freq, sample_rate)
            + saw_blep(self.phase2, freq * DETUNE, sample_rate))
            * 0.5;

        let (_, vow1, _) = self
            .form1
            .process(raw, self.f1_hz, FORMANT_Q, sample_rate);
        let (_, vow2, _) = self
            .form2
            .process(raw, self.f2_hz, FORMANT_Q, sample_rate);
        // Дыхание: шум через полосу 2.4 кГц поверх гласных.
        let (_, breath, _) = self
            .breath_bp
            .process(self.noise.sample(), BREATH_HZ, 1.2, sample_rate);

        self.t += 1.0 / sample_rate;
        (vow1 + vow2 + breath * BREATH_GAIN) * self.gain * amp * attack * fade * VOICE_GAIN
    }

    fn is_active(&self) -> bool {
        self.samples_left > 0
    }

    fn reset(&mut self) {
        self.samples_left = 0;
        self.t = 0.0;
        self.gain = 0.0;
        self.form1.reset();
        self.form2.reset();
        self.breath_bp.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Instrument, VoicePool};
    use super::*;

    #[test]
    fn vocal_chop_sings_and_decays() {
        let sr = 44_100.0;
        let mut vocal = VoicePool::vocals(11, 4);
        vocal.set_sample_rate(sr);
        vocal.trigger(&NoteEvent::new(11, 69, 0.9, 0.5));
        let mut peak: f32 = 0.0;
        let mut first = 0.0;
        let mut second = 0.0;
        for i in 0..20_000 {
            let s = vocal.next_sample();
            assert!(s.is_finite());
            peak = peak.max(s.abs());
            if i < 10_000 {
                first += s.abs();
            } else {
                second += s.abs();
            }
        }
        assert!(peak > 0.15, "vocal too quiet: {peak}");
        assert!(second < first, "vocal does not decay");
    }

    #[test]
    fn chops_sing_different_vowels() {
        // Слоги различаются от ноты к ноте, но детерминированы:
        // та же нота — та же гласная. Все гласные из таблицы.
        use std::collections::HashSet;
        let sr = 44_100.0;
        let mut seen = HashSet::new();
        for pitch in 60..72u8 {
            let mut v = VocalVoice::default();
            v.set_sample_rate(sr);
            v.trigger(&NoteEvent::new(11, pitch, 0.9, 0.5));
            assert!([730.0, 500.0, 610.0].contains(&v.f1_hz));
            assert!([1090.0, 860.0, 1900.0].contains(&v.f2_hz));
            seen.insert((v.f1_hz.to_bits(), v.f2_hz.to_bits()));
            let out: Vec<f32> = (0..2000).map(|_| v.next_sample()).collect();
            assert!(out.iter().all(|s| s.is_finite()));
            // Та же нота — та же гласная.
            let mut v2 = VocalVoice::default();
            v2.set_sample_rate(sr);
            v2.trigger(&NoteEvent::new(11, pitch, 0.9, 0.5));
            assert_eq!((v.f1_hz, v.f2_hz), (v2.f1_hz, v2.f2_hz));
        }
        assert!(seen.len() > 1, "one eternal vowel: {seen:?}");
    }
}
