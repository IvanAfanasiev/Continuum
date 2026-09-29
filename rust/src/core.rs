//! Общие музыкальные примитивы и константы.
//!
//! Единственное место для формул, которые раньше дублировались
//! по инструментам и стилям (перевод MIDI в частоту, длительность
//! шага, свинг). Убирает "магические числа" из DSP-кода.

use std::sync::RwLock;

/// Чтение shared-состояния без паники на отравленном lock:
/// вместо паники отдаем последнее значение. Заменяет десятки
/// `unwrap_or_else(|e| e.into_inner())` по кодовой базе.
pub(crate) fn snapshot<T: Clone>(lock: &RwLock<T>) -> T {
    lock.read()
        .map(|g| g.clone())
        .unwrap_or_else(|e| e.into_inner().clone())
}

/// Драйв в мастер-клиппер: микс входит в мягкое насыщение вместо
/// жесткого `clamp` — это склеивает микс и добавляет плотности низам.
/// Держим низко (0.32): плотные секции иначе давят транзиенты ударных —
/// бит должен оставаться фундаментом, а не тонуть в миксе.
pub const MASTER_DRIVE: f32 = 0.32;

/// Верхняя граница числа одновременно активных треков.
/// С запасом над числом слотов (17): массив, не Vec — без аллокаций
/// в горячем пути.
pub const MAX_ACTIVE_TRACKS: usize = 24;

/// Емкость lock-free очереди нот (бывшее `1024` в `main.rs`).
pub const NOTE_QUEUE_CAPACITY: usize = 1024;

/// Сид по умолчанию, если не передан через CLI.
pub const DEFAULT_SEED: u64 = 424242;

/// Перевод MIDI-ноты в частоту, Гц. Формула равномерно-темперированного строя.
/// Раньше копировалась в `bass.rs` и `pad.rs` (в паде — с багом каста к `i8`).
pub fn midi_to_freq(midi: u8) -> f32 {
    440.0 * 2.0_f32.powf((midi as f32 - 69.0) / 12.0)
}

/// Длительность одного шага сетки в секундах.
pub fn base_step_secs(bpm: f32, steps_per_measure: usize) -> f32 {
    60.0 / bpm / (steps_per_measure as f32 / 4.0)
}

/// Длительность шага с учетом свинга: четные шаги удлиняются,
/// нечетные укорачиваются (сумма пары = 2 * base).
pub fn swing_step_secs(base: f32, swing_ratio: f32, step_in_measure: usize) -> f32 {
    base * if step_in_measure.is_multiple_of(2) {
        swing_ratio
    } else {
        2.0 - swing_ratio
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a4_is_440hz() {
        assert!((midi_to_freq(69) - 440.0).abs() < 0.01);
    }

    #[test]
    fn midi_extremes_are_finite() {
        assert!(midi_to_freq(0).is_finite());
        assert!(midi_to_freq(127).is_finite());
    }

    #[test]
    fn swing_pair_sums_to_double_base() {
        let base = 0.125;
        let even = swing_step_secs(base, 1.3, 0);
        let odd = swing_step_secs(base, 1.3, 1);
        assert!(((even + odd) - 2.0 * base).abs() < 1e-6);
    }
}
