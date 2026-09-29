use super::{
    dsp::{release_fade, saw_blep, Svf},
    sane_sample_rate, NoteEvent, Voice,
};
use crate::core::midi_to_freq;

/// Фазз-синт: два расстроенных пилообразных осциллятора (PolyBLEP)
/// плюс суб-октава — через tanh-перегруз и тембровый lowpass.
/// Рев гитар без гитары: механический рок, индастриал-слои, жирные хуки.
/// Короткие ноты звучат чопами, длинные — стеной.
const DETUNE_UP: f32 = 1.00463;
const DETUNE_DOWN: f32 = 0.99539;
const DRIVE: f32 = 4.0;
const TONE_HZ: f32 = 2600.0;
const TONE_Q: f32 = 0.8;
const ATTACK_SECS: f32 = 0.004;
const VOICE_GAIN: f32 = 0.32;
const MAX_RING_SECS: f32 = 4.0;
const FADE_SECS: f32 = 0.02;

#[derive(Clone, Default)]
pub struct FuzzVoice {
    sample_rate: f32,
    phase_a: f32,
    phase_b: f32,
    phase_sub: f32,
    filter: Svf,
    frequency: f32,
    gain: f32,
    t: f32,
    samples_left: usize,
}

impl Voice for FuzzVoice {
    fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sane_sample_rate(sample_rate);
    }

    fn trigger(&mut self, note: &NoteEvent) {
        self.frequency = midi_to_freq(note.midi_pitch);
        let ring = note.duration_secs.clamp(0.05, MAX_RING_SECS);
        self.samples_left = (ring * self.sample_rate) as usize;
        self.gain = note.velocity;
        self.t = 0.0;
        self.phase_a = 0.0;
        self.phase_b = 0.0;
        self.phase_sub = 0.0;
        self.filter.reset();
    }

    fn next_sample(&mut self) -> f32 {
        if self.samples_left == 0 {
            return 0.0;
        }
        self.samples_left -= 1;

        let sample_rate = self.sample_rate;
        let a = saw_blep(self.phase_a, self.frequency * DETUNE_UP, sample_rate);
        let b = saw_blep(self.phase_b, self.frequency * DETUNE_DOWN, sample_rate);
        let s = saw_blep(self.phase_sub, self.frequency * 0.5, sample_rate);
        self.phase_a = (self.phase_a + self.frequency * DETUNE_UP / sample_rate) % 1.0;
        self.phase_b = (self.phase_b + self.frequency * DETUNE_DOWN / sample_rate) % 1.0;
        self.phase_sub = (self.phase_sub + self.frequency * 0.5 / sample_rate) % 1.0;

        // Перегруз вместо громкости: гармоники растут, пик — нет.
        let driven = ((a + b) * 0.5 + s * 0.6) * DRIVE;
        let shaped = driven.tanh();
        let (warm, _, _) = self.filter.process(shaped, TONE_HZ, TONE_Q, sample_rate);

        let u = (self.t / ATTACK_SECS).min(1.0);
        let attack = u * u * (3.0 - 2.0 * u);
        let fade = release_fade(self.samples_left, (FADE_SECS * sample_rate) as usize);

        self.t += 1.0 / sample_rate;
        warm * self.gain * attack * fade * VOICE_GAIN
    }

    fn is_active(&self) -> bool {
        self.samples_left > 0
    }

    fn reset(&mut self) {
        self.samples_left = 0;
        self.t = 0.0;
        self.gain = 0.0;
        self.filter.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Instrument, VoicePool};
    use super::*;

    fn fuzzes() -> VoicePool {
        VoicePool::fuzzes(17, 4)
    }

    #[test]
    fn fuzz_roars_finite_and_loud() {
        let sr = 44_100.0;
        let mut fuzz = fuzzes();
        fuzz.set_sample_rate(sr);
        fuzz.trigger(&NoteEvent::new(17, 45, 0.9, 0.5));
        let mut peak: f32 = 0.0;
        for _ in 0..(sr as usize) {
            let s = fuzz.next_sample();
            assert!(s.is_finite());
            peak = peak.max(s.abs());
        }
        assert!(peak > 0.15, "fuzz too quiet: {peak}");
        assert!(peak < 0.6, "fuzz blows up: {peak}");
        assert!(!fuzz.is_active());
    }

    #[test]
    fn fuzz_chugs_are_short() {
        // Короткая нота — короткий чоп: во второй половине тишина.
        let sr = 44_100.0;
        let mut fuzz = fuzzes();
        fuzz.set_sample_rate(sr);
        fuzz.trigger(&NoteEvent::new(17, 40, 0.8, 0.1));
        let mut late = 0.0;
        for i in 0..(sr as usize) {
            let s = fuzz.next_sample();
            assert!(s.is_finite());
            if i >= (0.5 * sr) as usize {
                late += s.abs();
            }
        }
        assert!(late < 1.0, "chug does not end: {late}");
    }
}
