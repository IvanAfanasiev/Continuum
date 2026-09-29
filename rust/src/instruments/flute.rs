use std::f32::consts::TAU;

use super::{
    dsp::{release_fade, Noise, Svf},
    sane_sample_rate, NoteEvent, Voice,
};
use crate::core::midi_to_freq;

/// Духовая флейта: чистый синус + дыхание через полосу + позднее
/// вибрато (раскачивается после атаки, как у живого дыхания).
/// Для лоуфай-мелодий поверх Rhodes. Мягкая атака — без язычка.
const SHIMMER_GAIN: f32 = 0.15;
const BREATH_HZ: f32 = 2500.0;
const BREATH_Q: f32 = 1.5;
const BREATH_GAIN: f32 = 0.12;
const VIBRATO_HZ: f32 = 5.5;
const VIBRATO_DEPTH: f32 = 0.008;
const VIBRATO_BLOOM_SECS: f32 = 0.15;
const ATTACK_SECS: f32 = 0.08;
const AMP_DECAY_RATE: f32 = 1.2;
const SUSTAIN_LEVEL: f32 = 0.8;
const MAX_RING_SECS: f32 = 2.0;
const FADE_SECS: f32 = 0.15;
const VOICE_GAIN: f32 = 0.5;

#[derive(Clone, Default)]
pub struct FluteVoice {
    sample_rate: f32,
    phase: f32,
    phase2: f32,
    vib_phase: f32,
    breath: Svf,
    noise: Noise,
    frequency: f32,
    gain: f32,
    t: f32,
    samples_left: usize,
}

impl Voice for FluteVoice {
    fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sane_sample_rate(sample_rate);
    }

    fn trigger(&mut self, note: &NoteEvent) {
        self.frequency = midi_to_freq(note.midi_pitch);
        let ring = note.duration_secs.clamp(0.1, MAX_RING_SECS);
        self.samples_left = (ring * self.sample_rate) as usize;
        self.gain = note.velocity;
        self.t = 0.0;
        self.phase = 0.0;
        self.phase2 = 0.0;
        self.breath.reset();
    }

    fn next_sample(&mut self) -> f32 {
        if self.samples_left == 0 {
            return 0.0;
        }
        self.samples_left -= 1;

        let sample_rate = self.sample_rate;
        let a = (self.t / ATTACK_SECS).min(1.0);
        let attack = a * a * (3.0 - 2.0 * a);
        let amp = SUSTAIN_LEVEL + (1.0 - SUSTAIN_LEVEL) * (-AMP_DECAY_RATE * self.t).exp();
        let fade = release_fade(self.samples_left, (FADE_SECS * sample_rate) as usize);

        // Вибрато расцветает после атаки.
        self.vib_phase = (self.vib_phase + VIBRATO_HZ / sample_rate) % 1.0;
        let bloom = (self.t / VIBRATO_BLOOM_SECS).min(1.0);
        let wobble = 1.0 + VIBRATO_DEPTH * bloom * (self.vib_phase * TAU).sin();
        let freq = self.frequency * wobble;

        self.phase = (self.phase + freq / sample_rate) % 1.0;
        self.phase2 = (self.phase2 + (freq * 2.0) / sample_rate) % 1.0;
        let tone = (self.phase * TAU).sin() + (self.phase2 * TAU).sin() * SHIMMER_GAIN;

        // Дыхание следует за огибающей.
        let (_, breath_band, _) =
            self.breath
                .process(self.noise.sample(), BREATH_HZ, BREATH_Q, sample_rate);
        let breath = breath_band * BREATH_GAIN * amp;

        self.t += 1.0 / sample_rate;
        (tone + breath) * self.gain * amp * attack * fade * VOICE_GAIN
    }

    fn is_active(&self) -> bool {
        self.samples_left > 0
    }

    fn reset(&mut self) {
        self.samples_left = 0;
        self.t = 0.0;
        self.gain = 0.0;
        self.breath.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Instrument, VoicePool};
    use super::*;

    fn flutes() -> VoicePool {
        VoicePool::flutes(13, 4)
    }

    #[test]
    fn flute_breathes_softly() {
        let sr = 44_100.0;
        let mut flute = flutes();
        flute.set_sample_rate(sr);
        flute.trigger(&NoteEvent::new(13, 72, 0.8, 1.5));
        // Мягкая атака: первые 5 мс тише, чем дальше.
        let mut early = 0.0;
        for _ in 0..220 {
            early += flute.next_sample().abs();
        }
        let mut later = 0.0;
        for _ in 0..5000 {
            let s = flute.next_sample();
            assert!(s.is_finite());
            later += s.abs();
        }
        assert!(later / 5000.0 > early / 220.0, "no soft attack");
        assert!(later > 1.0, "flute too quiet");
    }
}
