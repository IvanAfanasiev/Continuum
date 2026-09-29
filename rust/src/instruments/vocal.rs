use std::f32::consts::TAU;

use super::{
    dsp::{release_fade, saw_blep, Svf},
    sane_sample_rate, NoteEvent, Voice,
};
use crate::core::midi_to_freq;

/// Вокальный чоп «ah»: пара пил с вибрато через две формантные полосы.
/// Половину узнаваемости дает сильный дилэй на мастере (сенд роли Vocal).
const DETUNE: f32 = 1.004;
const VIBRATO_HZ: f32 = 5.2;
const VIBRATO_DEPTH: f32 = 0.006;
const FORMANT_F1_HZ: f32 = 730.0;
const FORMANT_F2_HZ: f32 = 1090.0;
const FORMANT_Q: f32 = 3.5;
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
    frequency: f32,
    gain: f32,
    t: f32,
    samples_left: usize,
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
            .process(raw, FORMANT_F1_HZ, FORMANT_Q, sample_rate);
        let (_, vow2, _) = self
            .form2
            .process(raw, FORMANT_F2_HZ, FORMANT_Q, sample_rate);

        self.t += 1.0 / sample_rate;
        (vow1 + vow2) * self.gain * amp * attack * fade * VOICE_GAIN
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
}
