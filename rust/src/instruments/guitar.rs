use std::f32::consts::TAU;

use super::{
    dsp::{release_fade, Noise, Svf},
    sane_sample_rate, NoteEvent, Voice,
};
use crate::core::midi_to_freq;

/// Чистая электрогитара: легкое расгеадо (три удара веером),
/// тихие ноты — с глушением струн (коротко, темно, перкуссивно).
/// Струнное тело: 5 гармоник с разной скоростью спада (верх гаснет
/// быстрее — подпись щипка, а не органный сустейн). Для лоуфай-боя
/// поверх Rhodes и рок-риффов. Полифония 6.
const PICK_DECAY: f32 = 250.0;
const PICK_GAIN: f32 = 0.45;
const PICK_LP_HZ: f32 = 3800.0;
/// (множитель частоты, амплитуда, скорость спада): чем выше частица,
/// тем короче живет — атака струнная, сустейн теплый.
const BODY_HARMONICS: [(f32, f32, f32); 5] = [
    (1.0, 1.0, 4.0),
    (2.0, 0.4, 6.5),
    (3.0, 0.22, 9.5),
    (4.0, 0.1, 13.0),
    (5.0, 0.05, 18.0),
];
const LP_HZ: f32 = 3400.0;
const LP_MUTED_HZ: f32 = 1800.0;
const LP_Q: f32 = 0.7;
const ATTACK_SECS: f32 = 0.006;
const AMP_DECAY_RATE: f32 = 4.0;
const SUSTAIN_LEVEL: f32 = 0.6;
const MUTE_DECAY_RATE: f32 = 28.0;
/// Тише этого — ghost-нота с приглушенными струнами.
const MUTE_VELOCITY: f32 = 0.6;
/// Расгеадо: сдвиги ударов и их акценты (сумма нормируется).
/// 16 мс между струнами — штрихи слышны раздельно, как бой,
/// а не один удар с тремоло.
const STRUM_GAP_SECS: f32 = 0.016;
const STRUM_GAP_MUTED_SECS: f32 = 0.008;
const STRUM_ACCENTS: [f32; 3] = [1.0, 0.7, 0.5];
const STRUM_NORM: f32 = 1.0 / (1.0 + 0.7 + 0.5);
const MAX_RING_SECS: f32 = 1.5;
const FADE_SECS: f32 = 0.05;
const VOICE_GAIN: f32 = 0.55;

#[derive(Clone, Default)]
pub struct GuitarVoice {
    sample_rate: f32,
    phases: [f32; 5],
    filter: Svf,
    pick_lp: Svf,
    noise: Noise,
    frequency: f32,
    gain: f32,
    muted: bool,
    t: f32,
    samples_left: usize,
}

impl Voice for GuitarVoice {
    fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sane_sample_rate(sample_rate);
    }

    fn trigger(&mut self, note: &NoteEvent) {
        self.frequency = midi_to_freq(note.midi_pitch);
        let ring = note.duration_secs.clamp(0.05, MAX_RING_SECS);
        self.samples_left = (ring * self.sample_rate) as usize;
        self.gain = note.velocity;
        // Тихие ghost-ноты играются с глушением струн.
        self.muted = note.velocity < MUTE_VELOCITY;
        self.t = 0.0;
        self.phases = [0.0; 5];
        self.filter.reset();
        self.pick_lp.reset();
        self.noise = Noise::new(super::dsp::note_seed(note, 0x6174));
    }

    fn next_sample(&mut self) -> f32 {
        if self.samples_left == 0 {
            return 0.0;
        }
        self.samples_left -= 1;

        let sample_rate = self.sample_rate;
        let a = (self.t / ATTACK_SECS).min(1.0);
        let attack = a * a * (3.0 - 2.0 * a);
        let amp = if self.muted {
            // Глушение: быстрый спад без сустейна, темно и перкуссивно.
            (-MUTE_DECAY_RATE * self.t).exp()
        } else {
            SUSTAIN_LEVEL + (1.0 - SUSTAIN_LEVEL) * (-AMP_DECAY_RATE * self.t).exp()
        };
        let fade = release_fade(self.samples_left, (FADE_SECS * sample_rate) as usize);

        // Расгеадо: веер из трех ударов со спадающими акцентами.
        let gap = if self.muted {
            STRUM_GAP_MUTED_SECS
        } else {
            STRUM_GAP_SECS
        };
        let mut strum = 0.0;
        for (k, accent) in STRUM_ACCENTS.iter().enumerate() {
            let tau = self.t - k as f32 * gap;
            if tau > 0.0 {
                let u = (tau / ATTACK_SECS).min(1.0);
                strum += accent * u * u * (3.0 - 2.0 * u);
            }
        }
        strum *= STRUM_NORM;

        // Тело: гармоники с собственным спадом — верх умирает первым.
        // Так звучит щипок струны, а не орган: атака струнная, хвост теплый.
        let mut body = 0.0;
        for (phase, (mult, amp_h, rate_h)) in self.phases.iter_mut().zip(BODY_HARMONICS) {
            *phase = (*phase + (self.frequency * mult) / sample_rate) % 1.0;
            let sustain_h = SUSTAIN_LEVEL + (1.0 - SUSTAIN_LEVEL) * (-rate_h * self.t).exp();
            body += (*phase * TAU).sin() * amp_h * sustain_h;
        }
        // Щипок медиатора: короткий шум через верх.
        let pick_raw = self.noise.sample() * (-PICK_DECAY * self.t).exp() * PICK_GAIN;
        let (pick, _, _) = self.pick_lp.process(pick_raw, PICK_LP_HZ, 0.7, sample_rate);

        let cutoff = if self.muted { LP_MUTED_HZ } else { LP_HZ };
        let (warm, _, _) = self.filter.process(body + pick, cutoff, LP_Q, sample_rate);

        self.t += 1.0 / sample_rate;
        warm * self.gain * amp * attack * fade * strum * VOICE_GAIN
    }

    fn is_active(&self) -> bool {
        self.samples_left > 0
    }

    fn reset(&mut self) {
        self.samples_left = 0;
        self.t = 0.0;
        self.gain = 0.0;
        self.filter.reset();
        self.pick_lp.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Instrument, VoicePool};
    use super::*;

    fn guitars() -> VoicePool {
        VoicePool::guitars(16, 6)
    }

    #[test]
    fn guitar_plucks_warm() {
        let sr = 44_100.0;
        let mut guitar = guitars();
        guitar.set_sample_rate(sr);
        guitar.trigger(&NoteEvent::new(16, 64, 0.9, 1.0));
        let mut peak: f32 = 0.0;
        let mut first = 0.0;
        let mut second = 0.0;
        for i in 0..(sr as usize) {
            let s = guitar.next_sample();
            assert!(s.is_finite());
            peak = peak.max(s.abs());
            if i < (0.5 * sr) as usize {
                first += s.abs();
            } else {
                second += s.abs();
            }
        }
        assert!(peak > 0.15, "guitar too quiet: {peak}");
        assert!(second < first, "guitar does not decay");
    }

    #[test]
    fn muted_ghost_dies_fast() {
        // Тихая нота (ghost) глушится: во второй половине почти тишина,
        // тогда как открытая в том же окне еще звучит.
        let sr = 44_100.0;
        let energy = |vel: f32| {
            let mut guitar = guitars();
            guitar.set_sample_rate(sr);
            guitar.trigger(&NoteEvent::new(16, 64, vel, 1.0));
            let mut late = 0.0;
            for i in 0..(sr as usize) {
                let s = guitar.next_sample();
                assert!(s.is_finite());
                if i >= (0.5 * sr) as usize {
                    late += s.abs();
                }
            }
            late
        };
        let open = energy(0.9);
        let muted = energy(0.4);
        assert!(muted < open * 0.2, "mute does not mute: {muted} vs {open}");
    }
}
