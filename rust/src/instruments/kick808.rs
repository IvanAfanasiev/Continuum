use std::f32::consts::TAU;

use super::{
    dsp::{release_fade, Noise},
    sane_sample_rate, Instrument, InstrumentRole, Mono, NoteEvent, Voice,
};
use crate::core::midi_to_freq;

/// 808-кик: низкий спад с длинным круглым хвостом, сустейн на высоте
/// ноты (мелодический 808 для трэп-баслайнов) и портаменто между нотами
/// (фирменные слайды). Тихий щелчок, никакого панча в грудь.
const LEN_SECS: f32 = 0.7;
const FREQ_START_HZ: f32 = 120.0;
const TARGET_MIN_HZ: f32 = 32.0;
const SWEEP_RATE: f32 = 32.0;
const AMP_DECAY: f32 = 5.0;
const CLICK_DECAY: f32 = 300.0;
const CLICK_GAIN: f32 = 0.3;
const GLIDE_SECS: f32 = 0.045;
/// Драйв сдержанный: перегруженный 808 пердит, а не гудит.
const DRIVE: f32 = 1.15;
const ATTACK_SECS: f32 = 0.0015;
const FADE_SECS: f32 = 0.02;

#[derive(Clone, Default)]
struct Kick808Voice {
    sample_rate: f32,
    t: f32,
    phase: f32,
    gain: f32,
    target: f32,
    base_smooth: f32,
    glide_coeff: f32,
    samples_left: usize,
    noise: Noise,
    /// Первый удар после тишины: слайдить не с чего — стартуем с ноты.
    /// Иначе первый хит чирикает снизу вверх.
    snap: bool,
}

impl Voice for Kick808Voice {
    fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sane_sample_rate(sample_rate);
        self.glide_coeff = 1.0 - (-1.0 / (self.sample_rate * GLIDE_SECS)).exp();
    }

    fn set_glide_start(&mut self, freq_hz: f32) {
        // Слайд стартует с высоты прошлой ноты (ставит обертка).
        // Прошлой нет (тишина) — флаг: trigger поставит базу на ноту.
        if freq_hz <= 0.0 {
            self.snap = true;
        } else {
            self.snap = false;
            self.base_smooth = freq_hz.max(30.0);
        }
    }

    fn trigger(&mut self, note: &NoteEvent) {
        self.samples_left = (LEN_SECS * self.sample_rate) as usize;
        self.gain = note.velocity;
        self.target = midi_to_freq(note.midi_pitch).max(TARGET_MIN_HZ);
        if self.snap {
            self.base_smooth = self.target;
            self.snap = false;
        }
        self.t = 0.0;
        self.phase = 0.0;
        self.noise = Noise::new(super::dsp::note_seed(note, 0x808));
    }

    fn next_sample(&mut self) -> f32 {
        if self.samples_left == 0 {
            return 0.0;
        }
        self.samples_left -= 1;

        let sample_rate = self.sample_rate;
        let body_env = (-AMP_DECAY * self.t).exp();
        // База ползет к ноте (слайд), свип — от фиксированной атаки:
        // панч сохранен, сустейн мелодический.
        self.base_smooth += (self.target - self.base_smooth) * self.glide_coeff;
        let freq =
            self.base_smooth + (FREQ_START_HZ - self.base_smooth) * (-SWEEP_RATE * self.t).exp();
        self.phase = (self.phase + freq / sample_rate) % 1.0;
        let body = (self.phase * TAU).sin();

        let click = self.noise.sample() * (-CLICK_DECAY * self.t).exp() * CLICK_GAIN;

        let a = (self.t / ATTACK_SECS).min(1.0);
        let attack = a * a * (3.0 - 2.0 * a);
        let fade = release_fade(self.samples_left, (FADE_SECS * sample_rate) as usize);

        self.t += 1.0 / sample_rate;
        ((body + click) * DRIVE).tanh() * 0.9 * self.gain * body_env * attack * fade
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

/// Публичный инструмент: моно-голос с кроссфейдом хвоста + слайды
/// между нотами (высота прошлой ноты переживает своп в `last_freq`).
pub struct Kick808(Mono<Kick808Voice>, f32);

impl Kick808 {
    pub fn new(id: usize) -> Self {
        Self(Mono::new(id, InstrumentRole::Kick), 0.0)
    }
}

impl Instrument for Kick808 {
    fn set_sample_rate(&mut self, sample_rate: f32) {
        self.0.set_sample_rate(sample_rate);
    }

    fn trigger(&mut self, note: &NoteEvent) {
        let target = midi_to_freq(note.midi_pitch).max(TARGET_MIN_HZ);
        let from = self.1;
        self.0.trigger_with_setup(note, |v| v.set_glide_start(from));
        self.1 = target;
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
        self.1 = 0.0;
    }

    fn is_active(&self) -> bool {
        self.0.is_active()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_hit_starts_in_tune() {
        // Первый удар после тишины: база = нота сразу, без чирика снизу.
        let sr = 44_100.0;
        let mut v = Kick808Voice::default();
        v.set_sample_rate(sr);
        v.set_glide_start(0.0);
        v.trigger(&NoteEvent::new(8, 36, 1.0, 0.5));
        let target = crate::core::midi_to_freq(36).max(32.0);
        assert!((v.base_smooth - target).abs() < 0.01, "chirp: {}", v.base_smooth);
        // Второй удар слайдится с прошлого, а не снэпается.
        v.set_glide_start(40.0);
        v.trigger(&NoteEvent::new(8, 43, 1.0, 0.5));
        assert!((v.base_smooth - 40.0).abs() < 0.01, "glide broken");
    }

    #[test]
    fn rings_long_finite_and_round() {
        let sr = 44_100.0;
        let mut kick = Kick808::new(8);
        kick.set_sample_rate(sr);
        kick.trigger(&NoteEvent::new(8, 36, 1.0, 0.1));
        let mut peak: f32 = 0.0;
        for _ in 0..((LEN_SECS * sr) as usize) {
            let s = kick.next_sample();
            assert!(s.is_finite());
            peak = peak.max(s.abs());
        }
        assert!(peak > 0.3, "808 too quiet: {peak}");
        assert!(!kick.is_active());
    }

    #[test]
    fn slides_between_notes_without_step() {
        // Слайд 65 → 98 Гц: высота ползет, ступеньки нет.
        let sr = 44_100.0;
        let mut kick = Kick808::new(8);
        kick.set_sample_rate(sr);
        kick.trigger(&NoteEvent::new(8, 36, 1.0, 0.5));
        for _ in 0..5000 {
            kick.next_sample();
        }
        kick.trigger(&NoteEvent::new(8, 43, 1.0, 0.5));
        let mut prev = kick.next_sample();
        let mut max_step: f32 = 0.0;
        for _ in 0..500 {
            let s = kick.next_sample();
            assert!(s.is_finite());
            max_step = max_step.max((s - prev).abs());
            prev = s;
        }
        assert!(max_step < 0.5, "glide step: {max_step}");
    }
}
