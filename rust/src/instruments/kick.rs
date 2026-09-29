use std::f32::consts::TAU;

use super::{
    dsp::{note_seed, release_fade, Noise, Svf},
    sane_sample_rate, Instrument, InstrumentRole, Mono, NoteEvent, Voice,
};

/// Электро-кик в духе 909: тело падает 170 → 44 Гц за ~6 мс
/// (быстрый свип = панч сидит точно на сетке, а не плывет),
/// щелчок-битер через полосу, короткий «knock» на 280 Гц.
/// Хвост короткий (0.30с): длинный суб-хвост бьется с басом
/// (44 Гц против ~55 Гц = 11 Гц «жабьего» рипла).
const CLICK_BP_HZ: f32 = 3500.0;
const CLICK_BP_Q: f32 = 0.8;
const KICK_LEN_SECS: f32 = 0.30;
const FREQ_START_HZ: f32 = 170.0;
const FREQ_END_HZ: f32 = 44.0;
const SWEEP_RATE: f32 = 160.0;
const AMP_DECAY: f32 = 7.5;
const CLICK_DECAY: f32 = 380.0;
const CLICK_GAIN: f32 = 0.6;
const KNOCK_HZ: f32 = 280.0;
const KNOCK_DECAY: f32 = 55.0;
const KNOCK_GAIN: f32 = 0.3;
const DRIVE: f32 = 1.7;
const ATTACK_SECS: f32 = 0.001;
const FADE_SECS: f32 = 0.012;

#[derive(Clone, Default)]
struct KickVoice {
    sample_rate: f32,
    t: f32,
    phase: f32,
    knock_phase: f32,
    gain: f32,
    samples_left: usize,
    noise: Noise,
    click_bp: Svf,
}

impl Voice for KickVoice {
    fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sane_sample_rate(sample_rate);
    }

    fn trigger(&mut self, note: &NoteEvent) {
        self.samples_left = (KICK_LEN_SECS * self.sample_rate) as usize;
        self.gain = note.velocity;
        self.t = 0.0;
        self.phase = 0.0;
        self.knock_phase = 0.0;
        self.click_bp.reset();
        // Шум щелчка пересеивается от ноты: одинаковые удары одинаковы,
        // разная velocity дает живую вариацию вместо machine-gun.
        self.noise = Noise::new(note_seed(note, 0xC10C));
    }

    fn next_sample(&mut self) -> f32 {
        if self.samples_left == 0 {
            return 0.0;
        }
        self.samples_left -= 1;

        let sample_rate = self.sample_rate;
        let body_env = (-AMP_DECAY * self.t).exp();
        let freq = FREQ_END_HZ + (FREQ_START_HZ - FREQ_END_HZ) * (-SWEEP_RATE * self.t).exp();
        self.phase = (self.phase + freq / sample_rate) % 1.0;
        let body = (self.phase * TAU).sin();

        self.knock_phase = (self.knock_phase + KNOCK_HZ / sample_rate) % 1.0;
        let knock = (self.knock_phase * TAU).sin() * (-KNOCK_DECAY * self.t).exp() * KNOCK_GAIN;

        // Щелчок-битер живет первые ~5 мс: шум через полосу 3.5 кГц —
        // удар колотушки, а не сырой белый шум.
        let raw_click = self.noise.sample() * (-CLICK_DECAY * self.t).exp();
        let (_, click_band, _) =
            self.click_bp
                .process(raw_click, CLICK_BP_HZ, CLICK_BP_Q, sample_rate);
        let click = click_band * CLICK_GAIN;

        let attack_raw = (self.t / ATTACK_SECS).min(1.0);
        // S-атака: у линейной остаются углы (микрощелчки на частых хитах).
        let attack = attack_raw * attack_raw * (3.0 - 2.0 * attack_raw);
        let fade = release_fade(self.samples_left, (FADE_SECS * sample_rate) as usize);

        self.t += 1.0 / sample_rate;
        ((body + knock + click) * DRIVE).tanh() * 0.9 * self.gain * body_env * attack * fade
    }

    fn reset(&mut self) {
        self.samples_left = 0;
        self.t = 0.0;
        self.phase = 0.0;
        self.gain = 0.0;
    }

    fn is_active(&self) -> bool {
        self.samples_left > 0
    }
}

/// Публичный инструмент: моно-голос с кроссфейдом хвоста.
pub struct Kick(Mono<KickVoice>);

impl Kick {
    pub fn new(id: usize) -> Self {
        Self(Mono::new(id, InstrumentRole::Kick))
    }
}

impl Instrument for Kick {
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
    fn kick_rings_finite_and_loud() {
        let sr = 44_100.0;
        let mut kick = Kick::new(2);
        kick.set_sample_rate(sr);
        kick.trigger(&NoteEvent::new(2, 36, 1.0, 0.1));
        let mut peak: f32 = 0.0;
        for _ in 0..((KICK_LEN_SECS * sr) as usize) {
            let s = kick.next_sample();
            assert!(s.is_finite());
            peak = peak.max(s.abs());
        }
        assert!(peak > 0.3, "kick too quiet: {peak}");
        assert!(!kick.is_active());
    }

    #[test]
    fn retrigger_has_no_audible_step() {
        // Регрессия щелчка на частых повторах: ретриггер в громкий хвост
        // не должен давать ступеньку — хвост гаснет кроссфейдом.
        let sr = 44_100.0;
        let mut kick = Kick::new(2);
        kick.set_sample_rate(sr);
        kick.trigger(&NoteEvent::new(2, 36, 1.0, 0.4));
        for _ in 0..2000 {
            kick.next_sample();
        }
        kick.trigger(&NoteEvent::new(2, 36, 1.0, 0.4));
        let mut prev = kick.next_sample();
        assert!(prev.is_finite());
        let mut max_step: f32 = 0.0;
        for _ in 0..500 {
            let s = kick.next_sample();
            assert!(s.is_finite());
            max_step = max_step.max((s - prev).abs());
            prev = s;
        }
        assert!(max_step < 0.5, "retrigger step: {max_step}");
    }
}
