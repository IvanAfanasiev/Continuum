use super::{
    dsp::{note_seed, release_fade, square_blep, ExpDecay, Noise, Svf},
    sane_sample_rate, Instrument, InstrumentRole, Mono, NoteEvent, Voice,
};

/// Хэт в духе 808: шесть негармонических BLEP-квадратов через крутой
/// SVF-highpass. Наивные квадраты давали алиасинговый металлический
/// «песок» — BLEP оставляет чистый металл. Открытость — длительностью
/// ноты, монофония с choke, как педаль живого хэта.
///
/// Осцилляторы стоят в ПЕРКУССИОННОМ регистре (7–14 кГц), а не в
/// диапазоне тарелки 808 (205–556 Гц): с фильтром на 6 кГц от тарелки
/// оставались одни слабые верхние гармоники, и хэт был на 16 дБ тише
/// кика — в миксе его не было слышно совсем (замер соло: пик −20 дБ).
const OSC_HZ: [f32; 6] = [7000.0, 8100.0, 9300.0, 10600.0, 12000.0, 13600.0];
/// Срез highpass: гармоники квадратов выше него — тело тика.
/// 5000 Гц: пропускает весь ударный диапазон хэта, но не батарею
/// (корпус ударных должен остаться в ударных).
const HP_HZ: f32 = 5000.0;
const HP_Q: f32 = 0.7;
const CLOSED_LEN_SECS: f32 = 0.05;
const OPEN_THRESHOLD_SECS: f32 = 0.12;
const MAX_OPEN_SECS: f32 = 0.5;
const CLOSED_DECAY: f32 = 90.0;
const OPEN_DECAY: f32 = 9.0;
/// Атака смягчена (1.5 мс): transient shaper против резких пиков,
/// как в спеке перкуссии. Было 0.5 мс — щелкало.
const ATTACK_SECS: f32 = 0.0015;
const FADE_SECS: f32 = 0.006;
/// Шумовой слой поверх BLEP-металла: живой хэт — это шум тарелки,
/// одни квадраты звучат стерильно (ср. v1: шум 0.65 + звон 0.35).
/// Тихо, чтобы не шипеть: воздух, а не песок.
const NOISE_GAIN: f32 = 0.18;
const METAL_GAIN: f32 = 0.82;

#[derive(Clone, Default)]
struct HatVoice {
    sample_rate: f32,
    phases: [f32; 6],
    t: f32,
    decay: f32,
    gain: f32,
    samples_left: usize,
    hp: Svf,
    /// Рекуррентная огибающая вместо посэмплового `exp`.
    env: ExpDecay,
    /// Шум тарелки: пересеивается нотой (одинаковые тики одинаковы).
    noise: Noise,
}

impl Voice for HatVoice {
    fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sane_sample_rate(sample_rate);
    }

    fn trigger(&mut self, note: &NoteEvent) {
        let open = note.duration_secs > OPEN_THRESHOLD_SECS;
        let len = if open {
            note.duration_secs.min(MAX_OPEN_SECS)
        } else {
            CLOSED_LEN_SECS
        };
        self.samples_left = (len * self.sample_rate).max(1.0) as usize;
        self.decay = if open { OPEN_DECAY } else { CLOSED_DECAY };
        self.gain = note.velocity;
        self.t = 0.0;
        self.env = ExpDecay::start(self.decay, self.sample_rate);
        self.hp.reset();
        // Фазы осцилляторов рассеиваем от ноты: металл живет перебиениями
        // (одинаковые фазы каждый хит дали бы machine-gun), choke-щелчок
        // давит attack-ramp, хвост прошлого хита — кроссфейд Mono.
        let seed = note_seed(note, 0x1A7);
        for (i, phase) in self.phases.iter_mut().enumerate() {
            let bits = seed.wrapping_add((i as u32).wrapping_mul(0x9E3779B9));
            *phase = (bits % 1000) as f32 / 1000.0;
        }
        self.noise = Noise::new(seed ^ 0x9E51);
    }

    fn next_sample(&mut self) -> f32 {
        if self.samples_left == 0 {
            return 0.0;
        }
        self.samples_left -= 1;

        let sample_rate = self.sample_rate;
        let mut metal = 0.0;
        for (phase, hz) in self.phases.iter_mut().zip(OSC_HZ) {
            *phase = (*phase + hz / sample_rate) % 1.0;
            metal += square_blep(*phase, hz, sample_rate);
        }
        metal /= OSC_HZ.len() as f32;
        let src = metal * METAL_GAIN + self.noise.sample() * NOISE_GAIN;

        let (_, _, bright) = self.hp.process(src, HP_HZ, HP_Q, sample_rate);
        let env = self.env.next();
        let attack_raw = (self.t / ATTACK_SECS).min(1.0);
        let attack = attack_raw * attack_raw * (3.0 - 2.0 * attack_raw);
        let fade = release_fade(self.samples_left, (FADE_SECS * sample_rate) as usize);

        self.t += 1.0 / sample_rate;
        bright * env * self.gain * attack * fade * 0.85
    }

    fn reset(&mut self) {
        self.samples_left = 0;
        self.gain = 0.0;
    }

    fn is_active(&self) -> bool {
        self.samples_left > 0
    }
}

/// Публичный инструмент: моно-голос с кроссфейдом хвоста.
pub struct HiHat(Mono<HatVoice>);

impl HiHat {
    pub fn new(id: usize) -> Self {
        Self(Mono::new(id, InstrumentRole::Hat))
    }
}

impl Instrument for HiHat {
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
    fn closed_hat_is_audible() {
        // Регрессия «хэта не слышно в миксе»: закрытый тик на полной
        // громкости обязан давать весомую энергию, а не шепот.
        let sr = 44_100.0;
        let mut hat = HiHat::new(5);
        hat.set_sample_rate(sr);
        hat.trigger(&NoteEvent::new(5, 42, 1.0, 0.05));
        let mut sum = 0.0;
        let mut n = 0;
        while hat.is_active() {
            let s = hat.next_sample();
            assert!(s.is_finite());
            sum += s.abs();
            n += 1;
        }
        assert!(n > 100, "tick too short: {n}");
        let mut peak: f32 = 0.0;
        // второй проход для пика — перетриггерим тот же тик
        hat.trigger(&NoteEvent::new(5, 42, 1.0, 0.05));
        while hat.is_active() {
            peak = peak.max(hat.next_sample().abs());
        }
        assert!(
            sum / n as f32 > 0.005 && peak > 0.1,
            "hat too quiet: mean={} peak={peak}",
            sum / n as f32
        );
    }

    #[test]
    fn closed_and_open_differ_in_length() {
        let sr = 44_100.0;
        let mut hat = HiHat::new(5);
        hat.set_sample_rate(sr);

        hat.trigger(&NoteEvent::new(5, 42, 1.0, 0.05));
        let mut closed_n = 0;
        while hat.is_active() {
            assert!(hat.next_sample().is_finite());
            closed_n += 1;
        }

        hat.trigger(&NoteEvent::new(5, 42, 1.0, 0.3));
        let mut open_n = 0;
        while hat.is_active() {
            assert!(hat.next_sample().is_finite());
            open_n += 1;
        }

        assert!(open_n > closed_n * 3, "open {open_n} vs closed {closed_n}");
    }
}
