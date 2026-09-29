use super::{
    dsp::{release_fade, saw_blep, square_blep, Svf},
    sane_sample_rate, NoteEvent, Voice,
};
use crate::core::midi_to_freq;

/// Лазерный лид: пила с быстрым нисходящим запитчем («pew») через
/// резонансный свип. Резкий и пронзительный — для взрывных дропов,
/// запрещен в мягких стилях.
const DETUNE: f32 = 1.004;
const ZAP_AMOUNT: f32 = 2.5;
const ZAP_RATE: f32 = 25.0;
const FILTER_FLOOR_HZ: f32 = 600.0;
const FILTER_SNAP_HZ: f32 = 4000.0;
const FILTER_SNAP_RATE: f32 = 15.0;
const FILTER_Q: f32 = 1.6;
const SUB_GAIN: f32 = 0.3;
const AMP_DECAY_RATE: f32 = 6.0;
const MAX_RING_SECS: f32 = 0.5;
const ATTACK_SECS: f32 = 0.001;
const FADE_SECS: f32 = 0.015;
const VOICE_GAIN: f32 = 0.55;

#[derive(Clone, Default)]
pub struct LaserVoice {
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

impl Voice for LaserVoice {
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
        self.filter.reset();
    }

    fn next_sample(&mut self) -> f32 {
        if self.samples_left == 0 {
            return 0.0;
        }
        self.samples_left -= 1;

        let sample_rate = self.sample_rate;
        // Запитч вниз: старт в 3.5 раза выше ноты, быстрое падение.
        let zap = 1.0 + ZAP_AMOUNT * (-ZAP_RATE * self.t).exp();
        let freq = self.frequency * zap;

        let a = (self.t / ATTACK_SECS).min(1.0);
        let attack = a * a * (3.0 - 2.0 * a);
        let amp = (-AMP_DECAY_RATE * self.t).exp();
        let fade = release_fade(self.samples_left, (FADE_SECS * sample_rate) as usize);

        self.phase_saw1 = (self.phase_saw1 + freq / sample_rate) % 1.0;
        self.phase_saw2 = (self.phase_saw2 + (freq * DETUNE) / sample_rate) % 1.0;
        self.phase_sub = (self.phase_sub + (self.frequency * 0.5) / sample_rate) % 1.0;
        let saw = (saw_blep(self.phase_saw1, freq, sample_rate)
            + saw_blep(self.phase_saw2, freq * DETUNE, sample_rate))
            * 0.5;
        let sub = square_blep(self.phase_sub, self.frequency * 0.5, sample_rate) * SUB_GAIN;

        let cutoff = FILTER_FLOOR_HZ + FILTER_SNAP_HZ * (-FILTER_SNAP_RATE * self.t).exp();
        let (low, _, _) = self
            .filter
            .process(saw + sub, cutoff, FILTER_Q, sample_rate);

        self.t += 1.0 / sample_rate;
        (low * 1.4).tanh() * self.gain * amp * attack * fade * VOICE_GAIN
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
    fn laser_zaps_and_decays() {
        let sr = 44_100.0;
        let mut laser = VoicePool::lasers(9, 5);
        laser.set_sample_rate(sr);
        laser.trigger(&NoteEvent::new(9, 76, 1.0, 0.4));
        let attack_peak = (0..2000)
            .map(|_| laser.next_sample().abs())
            .fold(0.0f32, f32::max);
        assert!(attack_peak > 0.1, "laser has no bite: {attack_peak}");
        let mut first = 0.0;
        let mut second = 0.0;
        for i in 0..16_000 {
            let s = laser.next_sample().abs();
            assert!(s.is_finite());
            if i < 8000 {
                first += s;
            } else {
                second += s;
            }
        }
        assert!(second < first, "laser does not decay");
    }
}
