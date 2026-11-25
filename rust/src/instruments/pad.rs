use std::f32::consts::TAU;

use super::{
    dsp::{fast_sin, note_seed, release_fade, saw_blep, sin01, Svf},
    sane_sample_rate, NoteEvent, Voice,
};
use crate::core::midi_to_freq;

/// Pad: 5 BLEP saws through a cascade of gentle SVFs (24 dB/oct) with a
/// breathing cutoff. Without BLEP the upper saws added up into aliasing
/// "sand" — the main source of the chiptune timbre in the previous version.
/// Attack and release scale with duration.
const DETUNE: [f32; 5] = [1.0, 1.004, 0.996, 1.007, 0.993];
const CUTOFF_MIN_HZ: f32 = 400.0;
const CUTOFF_SWEEP_HZ: f32 = 900.0;
const CUTOFF_VEL_HZ: f32 = 300.0;
const FILTER_Q: f32 = 0.7;
const LFO_HZ: f32 = 0.15;
/// 200 Hz high-pass (low end of the deep house spec is 200–300): the rumble
/// of the saw sub-octave is cut, making room for the bass. The bottom edge
/// of the spec is below our range.
const PAD_HP_HZ: f32 = 200.0;
/// Slow tuning drift (±3.5 cents): live analog breathing instead of
/// digital sterility. Drift phase is determined by note pitch —
/// the same chord sounds the same.
const DRIFT_HZ: f32 = 0.07;
const DRIFT_DEPTH: f32 = 0.002;
const VOICE_GAIN: f32 = 0.5;

#[derive(Clone, Default)]
pub struct PadVoice {
    sample_rate: f32,
    phases: [f32; 5],
    /// Per-voice detune: the same chord pulses differently in different
    /// voices instead of a synchronized "chorus" of copies.
    detunes: [f32; 5],
    lfo_phase: f32,
    drift_phase: f32,
    lp1: Svf,
    lp2: Svf,
    hp: Svf,
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
        // Seed articulation stretches attack/release: some seeds open the
        // pad sharply, others as a swell. The original clamps are kept.
        self.attack_samples = ((total as f32 * 0.25 * note.attack_mul.max(0.25)) as usize)
            .clamp(
                (0.005 * self.sample_rate) as usize,
                (1.8 * self.sample_rate) as usize,
            )
            .max(1);
        self.fade_samples = ((total as f32 * 0.3 * note.release_mul.max(0.25)) as usize)
            .min((0.6 * self.sample_rate) as usize)
            .max(1);
        // Small seed-dependent offsets of phases/detune/LFO: avoid a
        // synchronized "chorus" of copies and an identical transient.
        let mut seed = note_seed(note, 0x4321);
        for (i, phase) in self.phases.iter_mut().enumerate() {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            *phase = (seed as f32 / u32::MAX as f32) * 0.03;
            self.detunes[i] = DETUNE[i] * (1.0 + (((seed >> 12) % 1000) as f32 / 1000.0 - 0.5) * 0.0016);
        }
        self.lfo_phase = ((seed >> 5) % 1000) as f32 / 1000.0;
        self.drift_phase = ((seed >> 17) % 1000) as f32 / 1000.0 * TAU;
        // LFO and filters are preserved — the wall of sound breathes continuously.
        // States are only scrubbed of denormals (otherwise rare CPU spikes).
        self.lp1.scrub_denormals();
        self.lp2.scrub_denormals();
        self.hp.reset();
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
        let drift = 1.0 + DRIFT_DEPTH * fast_sin(self.drift_phase);
        for (phase, detune) in self.phases.iter_mut().zip(self.detunes) {
            let freq = self.frequency * detune * drift;
            *phase = (*phase + freq / sample_rate) % 1.0;
            raw += saw_blep(*phase, freq, sample_rate);
        }
        raw /= 5.0;

        self.lfo_phase = (self.lfo_phase + LFO_HZ / sample_rate) % 1.0;
        let lfo = sin01(self.lfo_phase) * 0.5 + 0.5;
        let cutoff = CUTOFF_MIN_HZ + lfo * CUTOFF_SWEEP_HZ + self.gain * CUTOFF_VEL_HZ;
        let (_, _, hipassed) = self.hp.process(raw, PAD_HP_HZ, FILTER_Q, sample_rate);
        let (l1, _, _) = self.lp1.process(hipassed, cutoff, FILTER_Q, sample_rate);
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
        self.hp.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Instrument, VoicePool};
    use super::*;

    #[test]
    fn seed_articulation_softens_attack() {
        // A long seed attack = swell instead of strike: less energy in the
        // first 150 ms than with a sharp one.
        fn early_energy(atk: f32) -> f32 {
            let sr = 44_100.0;
            let mut pad = VoicePool::pads(3, 1);
            pad.set_sample_rate(sr);
            pad.trigger(&NoteEvent::new(3, 55, 0.7, 1.0).shaped(atk, 1.0));
            (0..(0.15 * sr) as usize)
                .map(|_| {
                    let s = pad.next_sample();
                    assert!(s.is_finite());
                    s.abs()
                })
                .sum()
        }
        let sharp = early_energy(0.8);
        let soft = early_energy(1.8);
        assert!(
            soft < sharp,
            "articulation ignored: soft={soft} sharp={sharp}"
        );
    }

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
