use super::{
    dsp::{fast_tanh, note_seed, release_fade, sin01, ExpDecay, Noise, Svf},
    sane_sample_rate, Instrument, InstrumentRole, Mono, NoteEvent, Voice,
};

/// Электро-снейр в духе 808: тональное тело + шум через резонансную
/// полосу SVF (вместо грубой HP+LP связки — панч точнее, шипения меньше)
/// + «snap» в первую миллисекунду.
///
/// Два характера по высоте ноты: низкая (MIDI ниже `CLAP_PITCH`) —
/// барабан с корпусом, высокая — клэп 909 (корпуса почти нет, шум
/// ярче и короче). Раньше высота игнорировалась, и клубный «клэп»
/// звучал низким барабаном: 67% энергии ниже 120 Гц.
const SNARE_LEN_SECS: f32 = 0.22;
/// Порог «это клэп, а не барабан» (MIDI).
const CLAP_PITCH: u8 = 55;
const BODY_START_HZ: f32 = 190.0;
const BODY_END_HZ: f32 = 120.0;
const BODY_SWEEP: f32 = 30.0;
const BODY_DECAY: f32 = 38.0;
const NOISE_CENTER_HZ: f32 = 2200.0;
/// Центр шума клэпа: выше и ярче, чем у барабана.
const CLAP_NOISE_CENTER_HZ: f32 = 3000.0;
const NOISE_Q: f32 = 0.9;
const NOISE_DECAY: f32 = 22.0;
const NOISE_GAIN: f32 = 1.0;
const SNAP_DECAY: f32 = 300.0;
/// Snap — короткая яркая вспышка атаки. Раньше это был СЫРОЙ белый
/// шум: полоса от 0 Гц, и на каждом ударе микротреск. Теперь только
/// верх (4 кГц и выше) — ближе к 909, где «шкок» узкий.
const SNAP_HP_HZ: f32 = 4000.0;
const SNAP_GAIN: f32 = 0.45;
const ATTACK_SECS: f32 = 0.0005;
const FADE_SECS: f32 = 0.008;

#[derive(Clone, Default)]
struct SnareVoice {
    sample_rate: f32,
    t: f32,
    phase: f32,
    gain: f32,
    samples_left: usize,
    noise: Noise,
    bp: Svf,
    /// Отдельный highpass для snap: вспышка атаки должна быть узкой.
    snap_hp: Svf,
    /// Рекуррентные огибающие вместо посэмпловых `exp`.
    sweep_env: ExpDecay,
    body_env: ExpDecay,
    snap_env: ExpDecay,
    hiss_env: ExpDecay,
    /// Клэп (высокая нота) или барабан: корпус и центр шума разные.
    clap: bool,
}

impl Voice for SnareVoice {
    fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sane_sample_rate(sample_rate);
    }

    fn trigger(&mut self, note: &NoteEvent) {
        self.samples_left = (SNARE_LEN_SECS * self.sample_rate) as usize;
        self.gain = note.velocity;
        self.t = 0.0;
        self.phase = 0.0;
        self.sweep_env = ExpDecay::start(BODY_SWEEP, self.sample_rate);
        self.body_env = ExpDecay::start(BODY_DECAY, self.sample_rate);
        self.snap_env = ExpDecay::start(SNAP_DECAY, self.sample_rate);
        self.hiss_env = ExpDecay::start(NOISE_DECAY, self.sample_rate);
        self.bp.reset();
        self.snap_hp.reset();
        // Клэп или барабан: решает высота ноты (стиль задает характер
        // инструмента, а не только тембр).
        self.clap = note.midi_pitch >= CLAP_PITCH;
        self.noise = Noise::new(note_seed(note, 0x5A4E));
    }

    fn next_sample(&mut self) -> f32 {
        if self.samples_left == 0 {
            return 0.0;
        }
        self.samples_left -= 1;

        let sample_rate = self.sample_rate;
        let freq = BODY_END_HZ + (BODY_START_HZ - BODY_END_HZ) * self.sweep_env.next();
        self.phase = (self.phase + freq / sample_rate) % 1.0;
        let body = sin01(self.phase) + 0.3 * sin01(self.phase * 2.0);
        // У клэпа корпуса почти нет: хлопок — это шум, а не бочка.
        let body_gain = if self.clap { 0.12 } else { 0.7 };
        let body = body * self.body_env.next() * body_gain;

        let raw_noise = self.noise.sample();
        let snap = raw_noise * self.snap_env.next();
        // Snap — только верх: узкая вспышка атаки вместо белого треска.
        let (_, _, snap_bright) = self.snap_hp.process(snap, SNAP_HP_HZ, NOISE_Q, sample_rate);
        let snap = snap_bright * SNAP_GAIN;
        let center = if self.clap {
            CLAP_NOISE_CENTER_HZ
        } else {
            NOISE_CENTER_HZ
        };
        let (_, hiss, _) = self.bp.process(raw_noise, center, NOISE_Q, sample_rate);
        let hiss = hiss * self.hiss_env.next() * NOISE_GAIN;

        let attack_raw = (self.t / ATTACK_SECS).min(1.0);
        let attack = attack_raw * attack_raw * (3.0 - 2.0 * attack_raw);
        let fade = release_fade(self.samples_left, (FADE_SECS * sample_rate) as usize);

        self.t += 1.0 / sample_rate;
        fast_tanh((body + hiss + snap) * 1.4) * 0.85 * self.gain * attack * fade
    }

    fn reset(&mut self) {
        self.samples_left = 0;
        self.t = 0.0;
        self.gain = 0.0;
    }

    fn is_active(&self) -> bool {
        self.samples_left > 0
    }
}

/// Публичный инструмент: моно-голос с кроссфейдом хвоста.
pub struct SnareDrum(Mono<SnareVoice>);

impl SnareDrum {
    pub fn new(id: usize) -> Self {
        Self(Mono::new(id, InstrumentRole::Snare))
    }
}

impl Instrument for SnareDrum {
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
    fn snare_rings_finite_and_loud() {
        let sr = 44_100.0;
        let mut snare = SnareDrum::new(4);
        snare.set_sample_rate(sr);
        snare.trigger(&NoteEvent::new(4, 38, 1.0, 0.1));
        let mut peak: f32 = 0.0;
        for _ in 0..((SNARE_LEN_SECS * sr) as usize) {
            let s = snare.next_sample();
            assert!(s.is_finite());
            peak = peak.max(s.abs());
        }
        assert!(peak > 0.3, "snare too quiet: {peak}");
        assert!(!snare.is_active());
    }
}
