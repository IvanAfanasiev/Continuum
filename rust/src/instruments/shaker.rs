use super::{
    dsp::{release_fade, ExpDecay, Noise, Svf},
    sane_sample_rate, Instrument, InstrumentRole, Mono, NoteEvent, Voice,
};

/// Шейкер deep house: короткий шумовой всплеск через высокий highpass.
/// Металлика хэта тут нет — только песок атаки. Монофония с choke:
/// частые 16-е не копятся в кашу.
const HP_HZ: f32 = 7000.0;
const HP_Q: f32 = 0.7;
const BODY_DECAY: f32 = 90.0;
const ATTACK_SECS: f32 = 0.0005;
const FADE_SECS: f32 = 0.004;
const VOICE_GAIN: f32 = 0.8;

#[derive(Clone, Default)]
struct ShakerVoice {
    sample_rate: f32,
    t: f32,
    gain: f32,
    samples_left: usize,
    noise: Noise,
    hp: Svf,
    /// Рекуррентная огибающая вместо посэмплового `exp`.
    env: ExpDecay,
}

impl Voice for ShakerVoice {
    fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sane_sample_rate(sample_rate);
    }

    fn trigger(&mut self, note: &NoteEvent) {
        // Шейкер короткий по природе: длина от ноты, но не длиннее
        // 120 мс — иначе хвост тишины жрет DSP и держит choke.
        let len = note.duration_secs.clamp(0.02, 0.12) * self.sample_rate;
        self.samples_left = len.max(1.0) as usize;
        self.gain = note.velocity;
        self.t = 0.0;
        self.env = ExpDecay::start(BODY_DECAY, self.sample_rate);
        self.hp.reset();
        self.noise = Noise::new(super::dsp::note_seed(note, 0x54A4));
    }

    fn next_sample(&mut self) -> f32 {
        if self.samples_left == 0 {
            return 0.0;
        }
        self.samples_left -= 1;

        let sample_rate = self.sample_rate;
        let a = (self.t / ATTACK_SECS).min(1.0);
        let attack = a * a * (3.0 - 2.0 * a);
        let body = self.noise.sample() * self.env.next();
        let (_, _, bright) = self.hp.process(body, HP_HZ, HP_Q, sample_rate);
        let fade = release_fade(self.samples_left, (FADE_SECS * sample_rate) as usize);

        self.t += 1.0 / sample_rate;
        bright * self.gain * attack * fade * VOICE_GAIN
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

/// Публичный инструмент: моно-голос с кроссфейдом хвоста.
pub struct Shaker(Mono<ShakerVoice>);

impl Shaker {
    pub fn new(id: usize) -> Self {
        Self(Mono::new(id, InstrumentRole::Hat))
    }
}

impl Instrument for Shaker {
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
    fn shaker_burst_is_short_and_finite() {
        // Всплеск короче хэта: тишина уже через 120 мс при ноте 0.3 с.
        let sr = 44_100.0;
        let mut shaker = Shaker::new(18);
        shaker.set_sample_rate(sr);
        shaker.trigger(&NoteEvent::new(18, 60, 1.0, 0.3));
        let mut peak: f32 = 0.0;
        for _ in 0..(0.12 * sr) as usize {
            let s = shaker.next_sample();
            assert!(s.is_finite());
            peak = peak.max(s.abs());
        }
        assert!(peak > 0.1, "shaker too quiet: {peak}");
        for _ in 0..(0.2 * sr) as usize {
            shaker.next_sample();
        }
        assert!(!shaker.is_active(), "shaker rings too long");
    }
}
