use super::{
    dsp::{release_fade, saw_blep, square_blep, Svf},
    sane_sample_rate, NoteEvent, Voice,
};
use crate::core::midi_to_freq;

/// Голос электро-плака: BLEP-пилы + BLEP суб-квадрат через резонансный
/// SVF с быстрым «щелчком» среза и мягким насыщением. Чистый современный
/// плак вместо резкого чиптюна. Хвост ограничен 0.6 с.
const SUB_GAIN: f32 = 0.35;
const DETUNE: f32 = 1.004;
const FILTER_FLOOR_HZ: f32 = 350.0;
const FILTER_SNAP_HZ: f32 = 3800.0;
const FILTER_SNAP_RATE: f32 = 18.0;
const FILTER_Q: f32 = 1.15;
const AMP_DECAY_RATE: f32 = 7.0;
const MAX_RING_SECS: f32 = 0.6;
const ATTACK_SECS: f32 = 0.002;
const FADE_SECS: f32 = 0.02;
const VOICE_GAIN: f32 = 0.7;

#[derive(Clone, Default)]
pub struct PluckVoice {
    sample_rate: f32,
    phase_saw1: f32,
    phase_saw2: f32,
    phase_sub: f32,
    filter: Svf,
    frequency: f32,
    gain: f32,
    t: f32,
    samples_left: usize,
}

impl Voice for PluckVoice {
    fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sane_sample_rate(sample_rate);
    }

    fn trigger(&mut self, note: &NoteEvent) {
        self.frequency = midi_to_freq(note.midi_pitch);
        let ring = note.duration_secs.clamp(0.05, MAX_RING_SECS);
        self.samples_left = (ring * self.sample_rate) as usize;
        self.gain = note.velocity;
        self.t = 0.0;
        self.phase_saw1 = 0.0;
        self.phase_saw2 = 0.0;
        self.phase_sub = 0.0;
        self.filter.scrub_denormals();
    }

    fn next_sample(&mut self) -> f32 {
        if self.samples_left == 0 {
            return 0.0;
        }
        self.samples_left -= 1;

        let sample_rate = self.sample_rate;
        let attack_raw = (self.t / ATTACK_SECS).min(1.0);
        let attack = attack_raw * attack_raw * (3.0 - 2.0 * attack_raw);
        let amp = (-AMP_DECAY_RATE * self.t).exp();
        let fade = release_fade(self.samples_left, (FADE_SECS * sample_rate) as usize);

        self.phase_saw1 = (self.phase_saw1 + self.frequency / sample_rate) % 1.0;
        self.phase_saw2 = (self.phase_saw2 + (self.frequency * DETUNE) / sample_rate) % 1.0;
        self.phase_sub = (self.phase_sub + (self.frequency * 0.5) / sample_rate) % 1.0;
        let saw = (saw_blep(self.phase_saw1, self.frequency, sample_rate)
            + saw_blep(self.phase_saw2, self.frequency * DETUNE, sample_rate))
            * 0.5;
        let sub = square_blep(self.phase_sub, self.frequency * 0.5, sample_rate) * SUB_GAIN;

        let cutoff = FILTER_FLOOR_HZ + FILTER_SNAP_HZ * (-FILTER_SNAP_RATE * self.t).exp();
        let (low, _, _) = self
            .filter
            .process(saw + sub, cutoff, FILTER_Q, sample_rate);

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
    fn pluck_snaps_and_decays() {
        let sr = 44_100.0;
        let mut pluck = VoicePool::plucks(6, 6);
        pluck.set_sample_rate(sr);
        pluck.trigger(&NoteEvent::new(6, 72, 1.0, 0.5));
        let attack_peak = (0..2000)
            .map(|_| pluck.next_sample().abs())
            .fold(0.0f32, f32::max);
        assert!(attack_peak > 0.1, "pluck has no snap: {attack_peak}");
        let mut first = 0.0;
        let mut second = 0.0;
        for i in 0..20_000 {
            let s = pluck.next_sample().abs();
            assert!(s.is_finite());
            if i < 10_000 {
                first += s;
            } else {
                second += s;
            }
        }
        assert!(second < first, "pluck does not decay");
    }
}
