use std::f32::consts::TAU;

use super::{
    dsp::{note_seed, release_fade, Svf},
    sane_sample_rate, NoteEvent, Voice,
};
use crate::core::midi_to_freq;

/// Wavetable-лид: один период волны из таблицы + линейная интерполяция.
/// Таблица строится на триггер из гармоник (детерминирована нотой:
/// та же нота — та же краска), число гармоник режется Найквистом —
/// алиасинга нет по построению, BLEP не нужен. Дешевле всех
/// осцилляторных голосов (таблица вместо sin/exp на сэмпл) и ближе
/// всего к электро-низу: круговая волна, как в референсе с петлей.
/// Морф от сида: веса/фазы гармоник и наклон спектра — от хэша
/// (pitch, velocity): громкие ноты ярче, как живой инструмент.
const TABLE_LEN: usize = 1024;
const MAX_HARM: usize = 24;
const MAX_RING_SECS: f32 = 0.8;
const ATTACK_SECS: f32 = 0.003;
const FADE_SECS: f32 = 0.03;
const AMP_DECAY_RATE: f32 = 5.0;
const FILTER_FLOOR_HZ: f32 = 800.0;
const FILTER_SNAP_HZ: f32 = 4200.0;
const FILTER_SNAP_RATE: f32 = 9.0;
const FILTER_Q: f32 = 1.1;
const VOICE_GAIN: f32 = 0.55;

/// Детерминированный вес k-й гармоники: 0..1 из хэша ноты.
fn harm_weight(seed: u32, k: usize) -> f32 {
    let h = seed
        .wrapping_mul(0x9E37)
        .wrapping_add((k as u32).wrapping_mul(0x85EB));
    ((h >> 9) & 0xFF) as f32 / 255.0
}

/// Фаза k-й гармоники: 0..TAU из хэша (гребенка без когерентных пиков).
fn harm_phase(seed: u32, k: usize) -> f32 {
    let h = seed
        .wrapping_mul(0xC2B2)
        .wrapping_add((k as u32).wrapping_mul(0x27D4));
    ((h >> 7) & 0xFF) as f32 / 255.0 * TAU
}

#[derive(Clone)]
pub struct WavetableVoice {
    sample_rate: f32,
    /// Таблица на куче: голос живет в enum AnyVoice, и 4 КБ инлайном
    /// раздували бы каждый вариант пула. Клон при краже — тот же мемкопи.
    table: Box<[f32; TABLE_LEN]>,
    phase: f32,
    step: f32,
    filter: Svf,
    gain: f32,
    velocity: f32,
    t: f32,
    samples_left: usize,
}

impl Default for WavetableVoice {
    fn default() -> Self {
        Self {
            sample_rate: 44_100.0,
            table: Box::new([0.0; TABLE_LEN]),
            phase: 0.0,
            step: 0.0,
            filter: Svf::default(),
            gain: 0.0,
            velocity: 0.0,
            t: 0.0,
            samples_left: 0,
        }
    }
}

impl Voice for WavetableVoice {
    fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sane_sample_rate(sample_rate);
    }

    fn trigger(&mut self, note: &NoteEvent) {
        let freq = midi_to_freq(note.midi_pitch);
        // Полоса таблицы — до Найквиста и не выше: верхние гармоники
        // высокой ноты отрезаются, а не заворачиваются в свист.
        let nyquist_harm =
            (self.sample_rate * 0.45 / freq.max(20.0)).floor() as usize;
        let harms = nyquist_harm.clamp(2, MAX_HARM);
        let seed = note_seed(note, 0x1AB1E);
        // Наклон спектра от velocity: тихие — синус с теплом,
        // громкие — пила с зубьями.
        let tilt = 0.4 + 0.9 * note.velocity;
        let mut peak = 1e-6f32;
        for i in 0..TABLE_LEN {
            let x = i as f32 / TABLE_LEN as f32 * TAU;
            let mut s = 0.0;
            for k in 1..=harms {
                let amp = harm_weight(seed, k) * (k as f32).powf(-tilt);
                s += amp * (k as f32 * x + harm_phase(seed, k)).sin();
            }
            self.table[i] = s;
            peak = peak.max(s.abs());
        }
        // Нормировка таблицы: одинаковая громкость любой краски.
        let norm = 1.0 / peak;
        for s in self.table.iter_mut() {
            *s *= norm;
        }
        self.step = freq / self.sample_rate;
        self.phase = 0.0;
        let ring = note.duration_secs.clamp(0.05, MAX_RING_SECS);
        self.samples_left = (ring * self.sample_rate) as usize;
        self.velocity = note.velocity;
        self.gain = note.velocity;
        self.t = 0.0;
        self.filter.reset();
    }

    fn next_sample(&mut self) -> f32 {
        if self.samples_left == 0 {
            return 0.0;
        }
        self.samples_left -= 1;

        let sample_rate = self.sample_rate;
        // Табличный осциллятор с линейной интерполяцией: два чтения,
        // ноль трансцендентных функций в горячем пути.
        let pos = self.phase * TABLE_LEN as f32;
        let i0 = pos as usize % TABLE_LEN;
        let i1 = (i0 + 1) % TABLE_LEN;
        let frac = pos - pos.floor();
        let raw = self.table[i0] * (1.0 - frac) + self.table[i1] * frac;
        self.phase = (self.phase + self.step) % 1.0;

        let cutoff = FILTER_FLOOR_HZ
            + FILTER_SNAP_HZ * self.velocity * (-FILTER_SNAP_RATE * self.t).exp();
        let (low, _, _) = self.filter.process(raw, cutoff, FILTER_Q, sample_rate);

        let a = (self.t / ATTACK_SECS).min(1.0);
        let attack = a * a * (3.0 - 2.0 * a);
        let amp = (-AMP_DECAY_RATE * self.t).exp();
        let fade = release_fade(self.samples_left, (FADE_SECS * sample_rate) as usize);

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

    fn render(pitch: u8, vel: f32, secs: f32) -> Vec<f32> {
        let sr = 44_100.0;
        let mut pool = VoicePool::wavetables(19, 4);
        pool.set_sample_rate(sr);
        pool.trigger(&NoteEvent::new(19, pitch, vel, secs));
        (0..(secs * sr) as usize).map(|_| pool.next_sample()).collect()
    }

    #[test]
    fn wavetable_is_finite_and_audible() {
        for pitch in [36u8, 60, 72, 84] {
            let out = render(pitch, 0.9, 0.6);
            let peak = out.iter().fold(0.0f32, |m, s| {
                assert!(s.is_finite(), "pitch {pitch} non-finite");
                m.max(s.abs())
            });
            assert!(peak > 0.08, "pitch {pitch} too quiet: {peak}");
            assert!(peak < 0.8, "pitch {pitch} blows up: {peak}");
        }
    }

    #[test]
    fn same_note_same_table() {
        // Детерминизм краски: повтор — тот же поток сэмплов.
        let (a, b) = (render(64, 0.8, 0.3), render(64, 0.8, 0.3));
        assert_eq!(a.len(), b.len());
        for (x, y) in a.iter().zip(b.iter()) {
            assert_eq!(x.to_bits(), y.to_bits());
        }
    }

    #[test]
    fn tables_differ_across_pitches() {
        // Краска от ноты: разные питчи — разные таблицы
        // (иначе морф мертв и все звучит одним тембром).
        fn table(pitch: u8) -> Box<[f32; TABLE_LEN]> {
            let mut v = WavetableVoice::default();
            v.set_sample_rate(44_100.0);
            v.trigger(&NoteEvent::new(19, pitch, 0.9, 0.5));
            v.table.clone()
        }
        let (a, b) = (table(60), table(64));
        let diff: usize = a
            .iter()
            .zip(b.iter())
            .filter(|(x, y)| (*x - *y).abs() > 1e-4)
            .count();
        assert!(diff > TABLE_LEN / 2, "tables coincide: {diff}");
    }

    #[test]
    fn high_notes_stay_band_limited() {
        // Верхняя нота (84, ~1 кГц): гармоник мало, свиста нет —
        // спектр выше 12 кГц тише основного тона минимум в 10 раз.
        let out = render(84, 0.9, 0.5);
        let n = 8192;
        let seg: Vec<f32> = out.iter().take(n).copied().collect();
        let mut re = vec![0.0f32; n];
        let im = vec![0.0f32; n];
        for (i, s) in seg.iter().enumerate() {
            let w = 0.5 - 0.5 * (TAU * i as f32 / n as f32).cos();
            re[i] = s * w;
        }
        // Наивный DFT только нужных корзин.
        let mag = |bin: usize| {
            let mut rs = 0.0;
            let mut is = 0.0;
            for i in 0..n {
                let a = TAU * bin as f32 * i as f32 / n as f32;
                rs += re[i] * a.cos() + im[i] * a.sin();
                is += re[i] * a.sin() - im[i] * a.cos();
            }
            (rs * rs + is * is).sqrt()
        };
        let sr = 44_100.0;
        let fund_bin = (1046.0 / sr * n as f32).round() as usize;
        let fund = mag(fund_bin).max(1e-9);
        let mut hf = 0.0f32;
        let mut bin = ((12_000.0 / sr * n as f32).round() as usize).max(fund_bin + 1);
        while bin < n / 2 {
            hf = hf.max(mag(bin));
            bin += 7;
        }
        assert!(hf < fund * 0.3, "aliasing sings: hf={hf} fund={fund}");
    }

    #[test]
    fn goes_quiet_after_note_end() {
        let sr = 44_100.0;
        let mut pool = VoicePool::wavetables(19, 1);
        pool.set_sample_rate(sr);
        pool.trigger(&NoteEvent::new(19, 64, 1.0, 0.2));
        for _ in 0..(1.0 * sr) as usize {
            pool.next_sample();
        }
        assert!(!pool.is_active());
        assert_eq!(pool.next_sample(), 0.0);
    }
}
