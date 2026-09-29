use std::f32::consts::TAU;

use super::{
    dsp::{note_seed, release_fade, Noise, Svf},
    sane_sample_rate, Instrument, InstrumentRole, Mono, NoteEvent, Voice,
};

/// Спокойный шум дождя: ровная пелена фильтрованного шума плюс редкие
/// капли-пинги (короткие синусы 2–5 кГц с быстрым спадом).
/// Тихий фон лоуфая — в отличие от винила без треска и пылинок.
/// Играет длинными нотами-фразами.
const WASH_LP_HZ: f32 = 1200.0;
const WASH_GAIN: f32 = 0.6;
const WANDER_HZ: f32 = 0.08;
const WANDER_DEPTH: f32 = 0.25;
const DROP_THRESH: f32 = 0.9995;
const DROP_DECAY: f32 = 30.0;
const DROP_GAIN: f32 = 0.18;
const ATTACK_SECS: f32 = 0.5;
const FADE_SECS: f32 = 0.5;

#[derive(Clone, Default)]
struct RainVoice {
    sample_rate: f32,
    gain: f32,
    t: f32,
    wander_phase: f32,
    drop_phase: f32,
    drop_freq: f32,
    drop_env: f32,
    samples_left: usize,
    noise: Noise,
    wash_lp: Svf,
}

impl Voice for RainVoice {
    fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sane_sample_rate(sample_rate);
    }

    fn trigger(&mut self, note: &NoteEvent) {
        let len = note.duration_secs.clamp(2.0, 64.0);
        self.samples_left = (len * self.sample_rate) as usize;
        self.gain = note.velocity;
        self.t = 0.0;
        self.drop_env = 0.0;
        self.wash_lp.reset();
        self.noise = Noise::new(note_seed(note, 0x2A1D));
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

        // Пелена с дыханием.
        let raw = self.noise.sample();
        let (wash, _, _) = self.wash_lp.process(raw, WASH_LP_HZ, 0.7, sample_rate);
        self.wander_phase = (self.wander_phase + WANDER_HZ / sample_rate) % 1.0;
        let wander = 1.0 + WANDER_DEPTH * (self.wander_phase * TAU).sin();

        // Капля: выброс шума запускает высокий пинг.
        if raw > DROP_THRESH {
            // Частота капли 2–5 кГц из битов шума — детерминировано.
            let bits = self.noise.sample().to_bits();
            self.drop_freq = 2000.0 + (bits % 3000) as f32;
            self.drop_env = 1.0;
            self.drop_phase = 0.0;
        }
        self.drop_env *= (-DROP_DECAY / sample_rate).exp();
        self.drop_phase = (self.drop_phase + self.drop_freq / sample_rate) % 1.0;
        let drop = (self.drop_phase * TAU).sin() * self.drop_env * DROP_GAIN;

        self.t += 1.0 / sample_rate;
        (wash * WASH_GAIN * wander + drop) * self.gain * attack * fade * 0.5
    }

    fn is_active(&self) -> bool {
        self.samples_left > 0
    }

    fn reset(&mut self) {
        self.samples_left = 0;
        self.gain = 0.0;
        self.drop_env = 0.0;
    }
}

/// Публичный инструмент: моно-текстура с кроссфейдом фраз.
pub struct Rain(Mono<RainVoice>);

impl Rain {
    pub fn new(id: usize) -> Self {
        Self(Mono::new(id, InstrumentRole::Texture))
    }
}

impl Instrument for Rain {
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
    fn rain_is_quiet_wash_with_drops() {
        let sr = 44_100.0;
        let mut rain = Rain::new(17);
        rain.set_sample_rate(sr);
        rain.trigger(&NoteEvent::new(17, 60, 0.8, 8.0));
        let n = (4.0 * sr) as usize;
        let mut sum_sq = 0.0;
        let mut loud = 0;
        for _ in 0..n {
            let s = rain.next_sample();
            assert!(s.is_finite());
            sum_sq += s * s;
            if s.abs() > 0.1 {
                loud += 1;
            }
        }
        let rms = (sum_sq / n as f32).sqrt();
        assert!(rms > 0.003 && rms < 0.2, "wash level wrong: rms={rms}");
        // Капли пробиваются поверх пелены.
        assert!(loud > 5, "no rain drops");
    }
}
