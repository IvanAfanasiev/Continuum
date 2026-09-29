use super::{
    dsp::{release_fade, saw_blep, Svf},
    sane_sample_rate, NoteEvent, Voice,
};
use crate::core::midi_to_freq;

/// Фестивальный суперсо-лид: широкая стена из 5 пил через яркий
/// щелчок среза. Рифф биг-рума поверх дропа — светло и дерзко,
/// в противовес темному лазеру.
const DETUNE: [f32; 5] = [1.0, 1.009, 0.991, 1.005, 0.995];
const FILTER_FLOOR_HZ: f32 = 800.0;
const FILTER_SNAP_HZ: f32 = 5500.0;
const FILTER_SNAP_RATE: f32 = 16.0;
const FILTER_Q: f32 = 1.2;
const AMP_DECAY_RATE: f32 = 6.5;
const MAX_RING_SECS: f32 = 0.5;
const ATTACK_SECS: f32 = 0.002;
const FADE_SECS: f32 = 0.02;
const VOICE_GAIN: f32 = 0.5;

#[derive(Clone, Default)]
pub struct FestivalVoice {
    sample_rate: f32,
    phases: [f32; 5],
    filter: Svf,
    frequency: f32,
    gain: f32,
    t: f32,
    samples_left: usize,
}

impl Voice for FestivalVoice {
    fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sane_sample_rate(sample_rate);
    }

    fn trigger(&mut self, note: &NoteEvent) {
        self.frequency = midi_to_freq(note.midi_pitch);
        let ring = note.duration_secs.clamp(0.05, MAX_RING_SECS);
        self.samples_left = (ring * self.sample_rate) as usize;
        self.gain = note.velocity;
        self.t = 0.0;
        self.phases = [0.0; 5];
        self.filter.reset();
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

        let mut raw = 0.0;
        for (phase, detune) in self.phases.iter_mut().zip(DETUNE) {
            let freq = self.frequency * detune;
            *phase = (*phase + freq / sample_rate) % 1.0;
            raw += saw_blep(*phase, freq, sample_rate);
        }
        raw /= 5.0;

        let cutoff = FILTER_FLOOR_HZ + FILTER_SNAP_HZ * (-FILTER_SNAP_RATE * self.t).exp();
        let (low, _, _) = self.filter.process(raw, cutoff, FILTER_Q, sample_rate);

        self.t += 1.0 / sample_rate;
        (low * 1.3).tanh() * self.gain * amp * attack * fade * VOICE_GAIN
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

    #[test]
    fn festival_snaps_wide_and_decays() {
        let sr = 44_100.0;
        let mut lead = VoicePool::festivals(14, 6);
        lead.set_sample_rate(sr);
        lead.trigger(&NoteEvent::new(14, 79, 1.0, 0.4));
        let attack_peak = (0..2000)
            .map(|_| lead.next_sample().abs())
            .fold(0.0f32, f32::max);
        assert!(attack_peak > 0.1, "festival has no snap: {attack_peak}");
        let mut first = 0.0;
        let mut second = 0.0;
        for i in 0..16_000 {
            let s = lead.next_sample().abs();
            assert!(s.is_finite());
            if i < 8000 {
                first += s;
            } else {
                second += s;
            }
        }
        assert!(second < first, "festival does not decay");
    }
}
