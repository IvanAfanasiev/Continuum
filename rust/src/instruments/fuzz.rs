use super::{
    dsp::{release_fade, Svf},
    sane_sample_rate, NoteEvent, Voice,
};
use crate::core::midi_to_freq;
use std::f32::consts::TAU;

/// Фазз-синт: битовая формула из «math functions» (`−1 + ¼m₂ + m₄·m₁`)
/// плюс жирная суб-октава — через tanh-перегруз и темный lowpass.
/// Рев гитар без гитары: механический рок, индастриал-слои, жирные хуки.
/// Темный и злой — в противовес светлой стене фестиваля.
/// Короткие ноты звучат чопами, длинные — стеной.
const DRIVE: f32 = 4.5;
const TONE_HZ: f32 = 1900.0;
const TONE_Q: f32 = 0.8;
const ATTACK_SECS: f32 = 0.004;
const VOICE_GAIN: f32 = 0.32;
const MAX_RING_SECS: f32 = 4.0;
const FADE_SECS: f32 = 0.02;

#[derive(Clone, Default)]
pub struct FuzzVoice {
    sample_rate: f32,
    /// Циклы от старта (общая высота формулы).
    phase: f32,
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
        self.phase = 0.0;
        self.filter.reset();
    }

    fn next_sample(&mut self) -> f32 {
        if self.samples_left == 0 {
            return 0.0;
        }
        self.samples_left -= 1;

        let sample_rate = self.sample_rate;
        self.phase = (self.phase + self.frequency / sample_rate) % 4096.0;
        // Битовая формула вместо пил: скрежет цифры из битовых операций,
        // среднее вычтено (иначе постоянка косит перегруз).
        let x = self.phase;
        let m1 = (x.floor() as i64 & 1) as f32;
        let m2 = ((x * 2.0).floor() as i64 & 1) as f32;
        let m4 = ((x * 4.0).floor() as i64 & 1) as f32;
        let bit = (-1.0 + 0.25 * m2 + m4 * m1 + 0.625) * 0.7;
        let s = (TAU * x * 0.5).sin();

        // Перегруз вместо громкости: гармоники растут, пик — нет.
        // Суб жирный: стена давит низом, а не визжит верхом.
        let driven = (bit + s * 0.9) * DRIVE;
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
