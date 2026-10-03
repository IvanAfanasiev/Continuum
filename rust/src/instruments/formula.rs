use super::{
    dsp::{release_fade, Svf},
    sane_sample_rate, NoteEvent, Voice,
};
use crate::core::midi_to_freq;
use std::f32::consts::TAU;

/// Formula-voice: оммаж «math functions that go hard» — тембр как чистая
/// функция времени, без осцилляторов и сэмплов (пункт №9 цел).
/// 4 формулы выбираются классом питча (`pitch % 4`): та же нота — та же
/// формула (мотив держит лицо), разные сиды играют разные наборы питчей —
/// разный характер. Все детерминировано нотой: тот же сид — тот же звук.
const MAX_RING_SECS: f32 = 0.6;
const ATTACK_SECS: f32 = 0.002;
const FADE_SECS: f32 = 0.02;
const AMP_DECAY_RATE: f32 = 7.0;
const FILTER_FLOOR_HZ: f32 = 700.0;
const FILTER_SNAP_HZ: f32 = 3800.0;
const FILTER_SNAP_RATE: f32 = 8.0;
const FILTER_Q: f32 = 1.1;
const VOICE_GAIN: f32 = 0.5;

/// Чистые интервалы фонк-шага внутри ноты: унисон, 10/9, 4/3, 3/2.
const PHONK_RATIOS: [f32; 4] = [1.0, 10.0 / 9.0, 4.0 / 3.0, 3.0 / 2.0];

/// Сырая формула: `x` — циклы от старта ноты, `tune` — детерминированная
/// приправа питча (для bytebeat-множителя). Чистая функция — тестируется
/// напрямую, без голоса вокруг.
fn raw(mode: u8, x: f32, tune: u32) -> f32 {
    match mode {
        // Эйсид из видео буквально: y = −1 + ¼m₂ + m₄·m₁.
        // Среднее за 2 цикла −0.625 — вычитаем, иначе постоянка
        // жрет хедрум и косит tanh.
        0 => {
            let m1 = ((x).floor() as i64 & 1) as f32;
            let m2 = ((x * 2.0).floor() as i64 & 1) as f32;
            let m4 = ((x * 4.0).floor() as i64 & 1) as f32;
            (-1.0 + 0.25 * m2 + m4 * m1 + 0.625) * 0.7
        }
        // Фонк-строй: высота шагает чистыми отношениями каждые полцикла.
        1 => {
            let r = PHONK_RATIOS[(x * 2.0).floor().rem_euclid(4.0) as usize];
            (TAU * x * r).sin()
        }
        // Микротональный стек дропа: унисон + квинта 3/2 + тон 9/8.
        // Только синусы — алиасинга нет по построению.
        2 => {
            (TAU * x).sin() * 0.6 + (TAU * x * 1.5).sin() * 0.3 + (TAU * x * 1.125).sin() * 0.25
        }
        // Bytebeat с периодом ноты: 256 шагов на цикл, грязь цифры.
        // Период привязан к высоте (а не к частоте дискретизации) —
        // формула поет ноту, а не визжит. Приглушен против остальных
        // режимов: соло звучит специей, а не бензопилой.
        _ => {
            let c = (x * 256.0).floor() as u64;
            let a = 3 + (tune % 5) as u64;
            let v = (c.wrapping_mul(a) ^ (c >> 2)) & 0x3F;
            (v as f32 / 32.0 - 1.0) * 0.55
        }
    }
}

#[derive(Clone, Default)]
pub struct FormulaVoice {
    sample_rate: f32,
    filter: Svf,
    frequency: f32,
    velocity: f32,
    gain: f32,
    mode: u8,
    tune: u32,
    t: f32,
    samples_left: usize,
}

impl Voice for FormulaVoice {
    fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sane_sample_rate(sample_rate);
    }

    fn trigger(&mut self, note: &NoteEvent) {
        self.frequency = midi_to_freq(note.midi_pitch);
        self.mode = note.midi_pitch % 4;
        self.tune = note.midi_pitch as u32;
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
        let x = self.t * self.frequency;
        let r = raw(self.mode, x, self.tune);

        // Велосити открывает фильтр: тихие ноты темнее, громкие злее.
        let cutoff =
            FILTER_FLOOR_HZ + FILTER_SNAP_HZ * self.velocity * (-FILTER_SNAP_RATE * self.t).exp();
        let (low, _, _) = self.filter.process(r, cutoff, FILTER_Q, sample_rate);

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
        let mut pool = VoicePool::formulas(19, 4);
        pool.set_sample_rate(sr);
        pool.trigger(&NoteEvent::new(19, pitch, vel, secs));
        (0..(secs * sr) as usize).map(|_| pool.next_sample()).collect()
    }

    #[test]
    fn all_modes_finite_and_audible() {
        // Питчи 60–63 покрывают все 4 формулы: конечность + слышимый
        // пик без взрыва.
        for pitch in 60..64u8 {
            let out = render(pitch, 0.9, 0.6);
            let peak = out.iter().fold(0.0f32, |m, s| {
                assert!(s.is_finite(), "mode {} non-finite", pitch % 4);
                m.max(s.abs())
            });
            assert!(peak > 0.08, "mode {} too quiet: {peak}", pitch % 4);
            assert!(peak < 0.8, "mode {} blows up: {peak}", pitch % 4);
        }
    }

    #[test]
    fn same_note_same_sound() {
        // Детерминизм ноты: повтор — тот же поток сэмплов.
        let (a, b) = (render(62, 0.8, 0.3), render(62, 0.8, 0.3));
        assert_eq!(a.len(), b.len());
        for (x, y) in a.iter().zip(b.iter()) {
            assert_eq!(x.to_bits(), y.to_bits());
        }
    }

    #[test]
    fn raw_modes_differ_and_centered() {
        // Формулы — разные волны, а не один синус под четырьмя именами.
        let xs = [0.13, 0.37, 0.61, 1.17];
        for &x in &xs {
            let vals = [raw(0, x, 62), raw(1, x, 62), raw(2, x, 62), raw(3, x, 62)];
            let spread = vals.iter().fold(0.0f32, |m, v| m.max(*v))
                - vals.iter().fold(f32::MAX, |m, v| m.min(*v));
            assert!(spread > 0.05, "modes coincide at x={x}: {vals:?}");
        }
        // Битовая формула без постоянки (среднее ~0 за 2 цикла).
        let mut sum = 0.0;
        let n = 2000;
        for i in 0..n {
            sum += raw(0, i as f32 / 1000.0, 60);
        }
        assert!((sum / n as f32).abs() < 0.05, "bitacid has DC: {}", sum / n as f32);
    }

    #[test]
    fn goes_quiet_after_note_end() {
        let sr = 44_100.0;
        let mut pool = VoicePool::formulas(19, 1);
        pool.set_sample_rate(sr);
        pool.trigger(&NoteEvent::new(19, 64, 1.0, 0.2));
        for _ in 0..(0.7 * sr) as usize {
            pool.next_sample();
        }
        assert!(!pool.is_active());
        assert_eq!(pool.next_sample(), 0.0);
    }
}
