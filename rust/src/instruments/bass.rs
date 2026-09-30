use super::{
    dsp::{release_fade, Svf},
    sane_sample_rate, Instrument, InstrumentRole, Mono, NoteEvent, Voice,
};
use crate::core::midi_to_freq;

/// Глубокий саб-бас: один синусный осциллятор, два тракта.
/// Чистый саб дает глубину (синус не умеет дребезжать и алиасить),
/// мягкое насыщение той же волны — теплое присутствие на мелких
/// динамиках (мозг восстанавливает фундамент по гармоникам).
/// Четная октава (+12 полутонов, ~0.3 от фундамента) — ламповая
/// теплота как в референсах: нечетный tanh в одиночку дает
/// только «пустой» квадратный тембр.
/// Никаких расстроенных пил и резонанса: именно они давали
/// «клавишный» дребезг прошлой версии.
const SUB_GAIN: f32 = 0.9;
const OCTAVE_GAIN: f32 = 0.5;
const PRESENCE_GAIN: f32 = 0.45;
const DRIVE_BASE: f32 = 1.5;
const DRIVE_VEL: f32 = 2.5;
const PRESENCE_Q: f32 = 0.7;
/// Легкое проседание питча в атаке — аналоговая «плотность» удара.
const PITCH_DROP: f32 = 0.025;
const PITCH_DROP_RATE: f32 = 80.0;
/// Дефолтная атака 6 мс: короче — резкий щелчок в миксе (замер lofi
/// против референсов), сиды умножают сверху свою мягкость.
const ATTACK_SECS: f32 = 0.006;
const AMP_DECAY_RATE: f32 = 8.0;
const SUSTAIN_LEVEL: f32 = 0.85;
const FADE_SECS: f32 = 0.012;

#[derive(Clone, Default)]
struct BassVoice {
    sample_rate: f32,
    phase: f32,
    frequency: f32,
    gain: f32,
    velocity: f32,
    elapsed: usize,
    samples_left: usize,
    presence_lp: Svf,
    attack_mul: f32,
    release_mul: f32,
}

impl Voice for BassVoice {
    fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sane_sample_rate(sample_rate);
    }

    fn trigger(&mut self, note: &NoteEvent) {
        self.frequency = midi_to_freq(note.midi_pitch);
        self.samples_left = (note.duration_secs * self.sample_rate).max(1.0) as usize;
        self.gain = note.velocity;
        self.velocity = note.velocity;
        self.phase = 0.0;
        self.elapsed = 0;
        self.attack_mul = note.attack_mul;
        self.release_mul = note.release_mul;
        self.presence_lp.reset();
    }

    fn next_sample(&mut self) -> f32 {
        if self.samples_left == 0 {
            return 0.0;
        }
        self.samples_left -= 1;

        let sample_rate = self.sample_rate;
        let t = self.elapsed as f32 / sample_rate;
        self.elapsed += 1;

        // Мягкая S-атака вместо линейной: ни щелчка, ни ватности.
        // Длина атаки — от сида: одни сиды punchier, другие ватнее.
        let attack_secs = ATTACK_SECS * self.attack_mul.max(0.25);
        let a = (t / attack_secs).min(1.0);
        let attack = a * a * (3.0 - 2.0 * a);
        let amp = SUSTAIN_LEVEL + (1.0 - SUSTAIN_LEVEL) * (-AMP_DECAY_RATE * t).exp();
        let fade_len = (FADE_SECS * self.release_mul.max(0.25) * sample_rate) as usize;
        let fade = release_fade(self.samples_left, fade_len);

        let freq = self.frequency * (1.0 - PITCH_DROP * (-PITCH_DROP_RATE * t).exp());
        self.phase = (self.phase + freq / sample_rate) % 1.0;
        let wave = (self.phase * std::f32::consts::TAU).sin();
        // Четная октава строго в фазе с фундаментом (та же фаза ×2):
        // тепло без биений — гармоники совпадают, а не спорят.
        let octave = (self.phase * 2.0 % 1.0 * std::f32::consts::TAU).sin() * OCTAVE_GAIN;

        let sub = wave * SUB_GAIN;
        // Та же волна через драйв: гармоники идеально когерентны
        // фундаменту — тепло без биений и хоруса.
        let drive = DRIVE_BASE + DRIVE_VEL * self.velocity;
        let cutoff = (self.frequency * 6.0).clamp(300.0, 1600.0);
        let (presence, _, _) =
            self.presence_lp
                .process((wave * drive).tanh() + octave, cutoff, PRESENCE_Q, sample_rate);

        (sub + presence * PRESENCE_GAIN) * self.gain * amp * attack * fade
    }

    fn reset(&mut self) {
        self.samples_left = 0;
        self.elapsed = 0;
        self.gain = 0.0;
        self.presence_lp.reset();
    }

    fn is_active(&self) -> bool {
        self.samples_left > 0
    }
}

/// Публичный инструмент: моно-голос с кроссфейдом хвоста.
/// Ретриггер на 16-х дропа не рубит сустейн ступенькой.
pub struct BassGuitar(Mono<BassVoice>);

impl BassGuitar {
    pub fn new(id: usize) -> Self {
        Self(Mono::new(id, InstrumentRole::Bass))
    }
}

impl Instrument for BassGuitar {
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

    fn goertzel(samples: &[f32], target_hz: f32, sample_rate: f32) -> f32 {
        let n = samples.len() as f32;
        let k = (target_hz / sample_rate * n).round();
        let omega = 2.0 * std::f32::consts::PI * k / n;
        let coeff = 2.0 * omega.cos();
        let (mut s1, mut s2) = (0.0, 0.0);
        for &x in samples {
            let s0 = x + coeff * s1 - s2;
            s2 = s1;
            s1 = s0;
        }
        (s1 * s1 + s2 * s2 - coeff * s1 * s2).sqrt()
    }

    fn render_bass(pitch: u8, vel: f32, secs: f32) -> Vec<f32> {
        let sr = 44_100.0;
        let mut bass = BassGuitar::new(1);
        bass.set_sample_rate(sr);
        bass.trigger(&NoteEvent::new(1, pitch, vel, secs));
        (0..(secs * sr) as usize)
            .map(|_| bass.next_sample())
            .collect()
    }

    #[test]
    fn long_note_sustains_beyond_old_decay() {
        let sr = 44_100.0;
        let out = render_bass(40, 1.0, 1.0);
        let mut sum_sq = 0.0;
        let mut n = 0;
        for (i, &s) in out.iter().enumerate() {
            assert!(s.is_finite());
            if i >= (0.5 * sr) as usize {
                sum_sq += s * s;
                n += 1;
            }
        }
        let rms = (sum_sq / n as f32).sqrt();
        assert!(rms > 0.05, "bass does not sustain: rms={rms}");
    }

    #[test]
    fn low_c2_root_is_loud() {
        // Регрессия «в басу нет басов»: корень C2 (65 Гц) обязан давать
        // весомый сустейн, а не шепот.
        let sr = 44_100.0;
        let out = render_bass(36, 1.0, 1.0);
        let mut sum_sq = 0.0;
        let mut n = 0;
        for (i, &s) in out.iter().enumerate() {
            assert!(s.is_finite());
            if i >= (0.3 * sr) as usize {
                sum_sq += s * s;
                n += 1;
            }
        }
        let rms = (sum_sq / n as f32).sqrt();
        assert!(rms > 0.08, "no bass weight: rms={rms}");
    }

    #[test]
    fn sub_fundamental_dominates_spectrum() {
        // A2 = ровно 110 Гц (целая корзина гёртцеля 1 Гц): энергия
        // фундамента обязана кратно превышать верхнюю середину.
        let sr = 44_100.0;
        let out = render_bass(45, 1.0, 1.2);
        let sub = goertzel(&out, 110.0, sr);
        let upper = goertzel(&out, 1000.0, sr);
        assert!(
            sub > upper * 3.0,
            "weak fundamental: sub={sub} upper={upper}"
        );
    }

    #[test]
    fn even_harmonic_adds_tube_warmth() {
        // Референс (Bad Angel, C2 65 Гц): 2-я гармоника ~0.3 от фундамента.
        // Чистый синус + tanh дают только нечетные — отсюда «плоскость».
        // Меряем на A2 (110 Гц, целая корзина): 2-я = ровно 220 Гц.
        let sr = 44_100.0;
        let out = render_bass(45, 1.0, 1.0);
        let fund = goertzel(&out, 110.0, sr);
        let second = goertzel(&out, 220.0, sr);
        assert!(
            second > fund * 0.15,
            "no even warmth: 2nd={second} fund={fund}"
        );
        assert!(
            second < fund * 0.45,
            "too honky: 2nd={second} fund={fund}"
        );
    }

    #[test]
    fn no_buzzy_harmonics() {
        // A2 = ровно 110 Гц: третья гармоника (330 Гц, тоже целый бин)
        // обязана быть умеренной. У пилы она 0.33 от фундамента —
        // отсюда и дребезг; у мягкого насыщения кратно меньше.
        let sr = 44_100.0;
        let out = render_bass(45, 1.0, 1.0);
        let fund = goertzel(&out, 110.0, sr);
        let third = goertzel(&out, 330.0, sr);
        assert!(third < fund * 0.45, "buzzy bass: fund={fund} 3rd={third}");
    }

    #[test]
    fn short_note_is_short() {
        let sr = 44_100.0;
        let mut bass = BassGuitar::new(1);
        bass.set_sample_rate(sr);
        bass.trigger(&NoteEvent::new(1, 40, 1.0, 0.1));
        for _ in 0..((0.1 * sr) as usize + 10) {
            bass.next_sample();
        }
        assert!(!bass.is_active());
    }

    #[test]
    fn seed_articulation_softens_attack() {
        // Мягкий сид (длинная атака) дает меньше энергии в первые 20 мс,
        // чем резкий — артикуляция реально работает, а не игнорируется.
        fn early_energy(atk: f32, rel: f32) -> f32 {
            let sr = 44_100.0;
            let mut bass = BassGuitar::new(1);
            bass.set_sample_rate(sr);
            bass.trigger(&NoteEvent::new(1, 40, 0.9, 0.5).shaped(atk, rel));
            (0..(0.02 * sr) as usize)
                .map(|_| {
                    let s = bass.next_sample();
                    assert!(s.is_finite());
                    s.abs()
                })
                .sum()
        }
        let sharp = early_energy(0.8, 0.8);
        let soft = early_energy(1.8, 1.6);
        assert!(soft < sharp, "articulation ignored: soft={soft} sharp={sharp}");
    }

    #[test]
    fn retrigger_has_no_audible_step() {
        // Ретриггер в громкий сустейн (16-е дропа) не должен давать
        // ступеньку: хвост гаснет кроссфейдом поверх новой атаки.
        let sr = 44_100.0;
        let mut bass = BassGuitar::new(1);
        bass.set_sample_rate(sr);
        bass.trigger(&NoteEvent::new(1, 36, 1.0, 0.5));
        for _ in 0..5000 {
            bass.next_sample();
        }
        bass.trigger(&NoteEvent::new(1, 36, 1.0, 0.5));
        let mut prev = bass.next_sample();
        assert!(prev.is_finite());
        let mut max_step: f32 = 0.0;
        for _ in 0..500 {
            let s = bass.next_sample();
            assert!(s.is_finite());
            max_step = max_step.max((s - prev).abs());
            prev = s;
        }
        assert!(max_step < 0.5, "retrigger step: {max_step}");
    }
}
