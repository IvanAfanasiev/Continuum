use std::f32::consts::TAU;

use super::{
    dsp::{release_fade, Noise, Svf},
    sane_sample_rate, Instrument, InstrumentRole, Mono, NoteEvent, Voice,
};

/// Крэш-тарелка: полоса шума + негармонические синусы-звоны,
/// длинный хвост. Маркер взрывного дропа (первый удар такта)
/// и влеты в секции. Роль — Hat (семья тарелок).
const LEN_SECS: f32 = 1.6;
const NOISE_CENTER_HZ: f32 = 6500.0;
const NOISE_Q: f32 = 0.8;
const NOISE_DECAY: f32 = 4.5;
const NOISE_GAIN: f32 = 0.7;
const PARTIALS: [(f32, f32); 3] = [(1244.0, 0.3), (1731.0, 0.2), (2392.0, 0.15)];
const PARTIAL_DECAY: f32 = 6.0;
const ATTACK_SECS: f32 = 0.001;
const FADE_SECS: f32 = 0.2;

#[derive(Clone, Default)]
struct CrashVoice {
    sample_rate: f32,
    t: f32,
    phases: [f32; 3],
    gain: f32,
    samples_left: usize,
    noise: Noise,
    bp: Svf,
}

impl Voice for CrashVoice {
    fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sane_sample_rate(sample_rate);
    }

    fn trigger(&mut self, note: &NoteEvent) {
        self.samples_left = (LEN_SECS * self.sample_rate) as usize;
        self.gain = note.velocity;
        self.t = 0.0;
        self.phases = [0.0; 3];
        self.bp.reset();
        self.noise = Noise::new(super::dsp::note_seed(note, 0xC2A5));
    }

    fn next_sample(&mut self) -> f32 {
        if self.samples_left == 0 {
            return 0.0;
        }
        self.samples_left -= 1;

        let sample_rate = self.sample_rate;
        let a = (self.t / ATTACK_SECS).min(1.0);
        let attack = a * a * (3.0 - 2.0 * a);
        let fade = release_fade(self.samples_left, (FADE_SECS * sample_rate) as usize);

        let (_, hiss, _) =
            self.bp
                .process(self.noise.sample(), NOISE_CENTER_HZ, NOISE_Q, sample_rate);
        let wash = hiss * (-NOISE_DECAY * self.t).exp() * NOISE_GAIN;

        let mut bells = 0.0;
        for (phase, (freq, amp)) in self.phases.iter_mut().zip(PARTIALS) {
            *phase = (*phase + freq / sample_rate) % 1.0;
            bells += (*phase * TAU).sin() * amp;
        }
        let bells = bells * (-PARTIAL_DECAY * self.t).exp();

        self.t += 1.0 / sample_rate;
        (wash + bells) * self.gain * attack * fade * 0.6
    }

    fn is_active(&self) -> bool {
        self.samples_left > 0
    }

    fn reset(&mut self) {
        self.samples_left = 0;
        self.t = 0.0;
        self.gain = 0.0;
    }
}

/// Публичный инструмент: моно с кроссфейдом (редкие удары не рвутся).
pub struct Crash(Mono<CrashVoice>);

impl Crash {
    pub fn new(id: usize) -> Self {
        Self(Mono::new(id, InstrumentRole::Hat))
    }
}

impl Instrument for Crash {
    fn set_sample_rate(&mut self, sample_rate: f32) {
        self.0.set_sample_rate(sample_rate);
    }

    fn trigger(&mut self, note: &NoteEvent) {
        self.0.trigger(note);
    }

    fn next_sample(&mut self) -> f32 {
        self.0.next_sample()
    }

    fn id(&self) -> usize {
        self.0.id()
    }

    fn role(&self) -> InstrumentRole {
        self.0.role()
    }

    fn reset(&mut self) {
        self.0.reset();
    }

    fn is_active(&self) -> bool {
        self.0.is_active()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crash_rings_long_and_decays() {
        let sr = 44_100.0;
        let mut crash = Crash::new(15);
        crash.set_sample_rate(sr);
        crash.trigger(&NoteEvent::new(15, 49, 1.0, 1.0));
        let mut peak: f32 = 0.0;
        let mut first = 0.0;
        let mut second = 0.0;
        let n = (LEN_SECS * sr) as usize;
        for i in 0..n {
            let s = crash.next_sample();
            assert!(s.is_finite());
            peak = peak.max(s.abs());
            if i < n / 2 {
                first += s.abs();
            } else {
                second += s.abs();
            }
        }
        assert!(peak > 0.15, "crash too quiet: {peak}");
        assert!(second < first, "crash does not decay");
        assert!(!crash.is_active());
    }
}
