use std::f32::consts::TAU;

use super::{
    dsp::{release_fade, saw_blep, Svf},
    sane_sample_rate, NoteEvent, Voice,
};
use crate::core::midi_to_freq;

/// Пад: 5 BLEP-пил через каскад мягких SVF (24 дБ/окт) с дышащим срезом.
/// Без BLEP верхние пилы складывались в алиасинговый «песок» —
/// главный источник чиптюн-тембра у прошлой версии.
/// Атака и релиз масштабируются от длительности.
const DETUNE: [f32; 5] = [1.0, 1.004, 0.996, 1.007, 0.993];
const CUTOFF_MIN_HZ: f32 = 400.0;
const CUTOFF_SWEEP_HZ: f32 = 900.0;
const CUTOFF_VEL_HZ: f32 = 300.0;
const FILTER_Q: f32 = 0.7;
const LFO_HZ: f32 = 0.15;
/// Медленный дрейф строя (±3.5 цента): живое дыхание аналога
/// вместо цифровой стерильности. Фаза дрейфа детерминирована
/// высотой ноты — один и тот же аккорд звучит одинаково.
const DRIFT_HZ: f32 = 0.07;
const DRIFT_DEPTH: f32 = 0.002;
const VOICE_GAIN: f32 = 0.5;

#[derive(Clone, Default)]
pub struct PadVoice {
    sample_rate: f32,
    phases: [f32; 5],
    lfo_phase: f32,
    drift_phase: f32,
    lp1: Svf,
    lp2: Svf,
    frequency: f32,
    gain: f32,
    attack_samples: usize,
    elapsed: usize,
    samples_left: usize,
    fade_samples: usize,
}

impl Voice for PadVoice {
    fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sane_sample_rate(sample_rate);
    }

    fn trigger(&mut self, note: &NoteEvent) {
        self.frequency = midi_to_freq(note.midi_pitch);
        let total = (note.duration_secs * self.sample_rate).max(1.0) as usize;
        self.samples_left = total;
        self.elapsed = 0;
        self.gain = note.velocity;
        self.attack_samples = ((total as f32 * 0.25) as usize)
            .clamp(
                (0.005 * self.sample_rate) as usize,
                (1.8 * self.sample_rate) as usize,
            )
            .max(1);
        self.fade_samples = ((total as f32 * 0.3) as usize)
            .min((0.6 * self.sample_rate) as usize)
            .max(1);
        self.phases = [0.0; 5];
        self.drift_phase = (note.midi_pitch as f32 * 0.7) % TAU;
        // LFO и фильтры сохраняются — стена звука дышит непрерывно.
        // Состояния лишь чистятся от деначала (иначе редкие всплески CPU).
        self.lp1.scrub_denormals();
        self.lp2.scrub_denormals();
    }

    fn next_sample(&mut self) -> f32 {
        if self.samples_left == 0 {
            return 0.0;
        }
        self.samples_left -= 1;

        let sample_rate = self.sample_rate;
        let attack = (self.elapsed as f32 / self.attack_samples as f32).min(1.0);
        let attack = attack * attack * (3.0 - 2.0 * attack);
        let fade = release_fade(self.samples_left, self.fade_samples);
        self.elapsed += 1;

        let mut raw = 0.0;
        self.drift_phase = (self.drift_phase + TAU * DRIFT_HZ / sample_rate) % TAU;
        let drift = 1.0 + DRIFT_DEPTH * self.drift_phase.sin();
        for (phase, detune) in self.phases.iter_mut().zip(DETUNE) {
            let freq = self.frequency * detune * drift;
            *phase = (*phase + freq / sample_rate) % 1.0;
            raw += saw_blep(*phase, freq, sample_rate);
        }
        raw /= 5.0;

        self.lfo_phase = (self.lfo_phase + LFO_HZ / sample_rate) % 1.0;
        let lfo = (self.lfo_phase * TAU).sin() * 0.5 + 0.5;
        let cutoff = CUTOFF_MIN_HZ + lfo * CUTOFF_SWEEP_HZ + self.gain * CUTOFF_VEL_HZ;
        let (l1, _, _) = self.lp1.process(raw, cutoff, FILTER_Q, sample_rate);
        let (l2, _, _) = self.lp2.process(l1, cutoff, FILTER_Q, sample_rate);

        l2 * self.gain * attack * fade * VOICE_GAIN
    }

    fn is_active(&self) -> bool {
        self.samples_left > 0
    }

    fn reset(&mut self) {
        self.samples_left = 0;
        self.elapsed = 0;
        self.gain = 0.0;
        self.lp1.reset();
        self.lp2.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Instrument, VoicePool};
    use super::*;

    #[test]
    fn pad_chord_sounds_all_notes() {
        let sr = 44_100.0;
        let mut pad = VoicePool::pads(3, 6);
        pad.set_sample_rate(sr);
        for pitch in [48, 55, 60] {
            pad.trigger(&NoteEvent::new(3, pitch, 0.7, 1.0));
        }
        let mut sum = 0.0;
        for _ in 0..5000 {
            let s = pad.next_sample();
            assert!(s.is_finite());
            sum += s.abs();
        }
        assert!(sum > 1.0, "chord too quiet: {sum}");
    }
}
