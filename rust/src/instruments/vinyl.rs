use std::f32::consts::TAU;

use super::{
    dsp::{note_seed, release_fade, Noise, Svf},
    sane_sample_rate, Instrument, InstrumentRole, Mono, NoteEvent, Voice,
};

/// Виниловый треск + шипение пленки: приглушенный шумовой слой
/// с медленным дыханием громкости и редкими щелчками-пылинками.
/// Играет длинными нотами-фразами; сид треска от ноты (velocity
/// хуманизирована — каждая фраза трещит по-своему, повтор детерминирован).
const HISS_LP_HZ: f32 = 4500.0;
const HISS_GAIN: f32 = 0.5;
const WANDER_HZ: f32 = 0.1;
const WANDER_DEPTH: f32 = 0.3;
const POP_THRESH: f32 = 0.9999;
const POP_DECAY: f32 = 120.0;
const POP_GAIN: f32 = 0.9;
const ATTACK_SECS: f32 = 0.05;
const FADE_SECS: f32 = 0.1;

#[derive(Clone, Default)]
struct VinylVoice {
    sample_rate: f32,
    gain: f32,
    t: f32,
    wander_phase: f32,
    pop_env: f32,
    samples_left: usize,
    noise: Noise,
    hiss_lp: Svf,
}

impl Voice for VinylVoice {
    fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sane_sample_rate(sample_rate);
    }

    fn trigger(&mut self, note: &NoteEvent) {
        // Длинная нота-текстура; длительность ограничиваем парой фраз,
        // чтобы зависшая нота не пережила выключение слота.
        let len = note.duration_secs.clamp(1.0, 32.0);
        self.samples_left = (len * self.sample_rate) as usize;
        self.gain = note.velocity;
        self.t = 0.0;
        self.pop_env = 0.0;
        self.hiss_lp.reset();
        self.noise = Noise::new(note_seed(note, 0xB171));
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

        // Шипение с дыханием.
        let raw = self.noise.sample();
        let (hiss, _, _) = self.hiss_lp.process(raw, HISS_LP_HZ, 0.7, sample_rate);
        self.wander_phase = (self.wander_phase + WANDER_HZ / sample_rate) % 1.0;
        let wander = 1.0 + WANDER_DEPTH * (self.wander_phase * TAU).sin();

        // Пылинка: выброс шума включает быстрый спад.
        if raw > POP_THRESH {
            self.pop_env = raw;
        }
        self.pop_env *= (-POP_DECAY / sample_rate).exp();
        let pop = self.pop_env * POP_GAIN;

        self.t += 1.0 / sample_rate;
        (hiss * HISS_GAIN * wander + pop) * self.gain * attack * fade * 0.5
    }

    fn is_active(&self) -> bool {
        self.samples_left > 0
    }

    fn reset(&mut self) {
        self.samples_left = 0;
        self.gain = 0.0;
        self.pop_env = 0.0;
    }
}

/// Публичный инструмент: моно-текстура с кроссфейдом фраз.
pub struct Vinyl(Mono<VinylVoice>);

impl Vinyl {
    pub fn new(id: usize) -> Self {
        Self(Mono::new(id, InstrumentRole::Texture))
    }
}

impl Instrument for Vinyl {
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
    fn vinyl_is_quiet_bed_with_pops() {
        let sr = 44_100.0;
        let mut vinyl = Vinyl::new(10);
        vinyl.set_sample_rate(sr);
        vinyl.trigger(&NoteEvent::new(10, 60, 0.8, 4.0));
        let mut peak: f32 = 0.0;
        let mut sum_sq = 0.0;
        let mut loud = 0;
        let n = (2.0 * sr) as usize;
        for _ in 0..n {
            let s = vinyl.next_sample();
            assert!(s.is_finite());
            peak = peak.max(s.abs());
            sum_sq += s * s;
            if s.abs() > 0.15 {
                loud += 1;
            }
        }
        let rms = (sum_sq / n as f32).sqrt();
        // Слышимый слой, но не громкий шум.
        assert!(rms > 0.005 && rms < 0.2, "bed level wrong: rms={rms}");
        // Щелчки-пылинки пробиваются поверх шипения.
        assert!(loud > 5, "no crackle pops: peak={peak} rms={rms}");
    }

    #[test]
    fn same_note_same_crackle() {
        // Детерминизм текстуры: повтор фразы трещит так же.
        let sr = 44_100.0;
        let render = |vel: f32| {
            let mut vinyl = Vinyl::new(10);
            vinyl.set_sample_rate(sr);
            vinyl.trigger(&NoteEvent::new(10, 60, vel, 4.0));
            (0..5000).map(|_| vinyl.next_sample()).collect::<Vec<_>>()
        };
        assert_eq!(render(0.8), render(0.8));
        assert_ne!(render(0.8), render(0.81));
    }
}
