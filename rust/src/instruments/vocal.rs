use super::{
    dsp::{release_fade, sin01, ExpDecay, Noise, Svf},
    sane_sample_rate, NoteEvent, Voice,
};
use crate::core::midi_to_freq;

/// Оперный голос: сольный певческий тембр (не хор) с богатым
/// спектром, поздним широким вибрато и портаменто между нотами.
/// Гласная «ah» медленно плывёт в «oh» внутри ноты. Дыхание — едва
/// слышный воздух. Узнаваемость дает сильный дилэй на мастере
/// (сенд роли Vocal).
///
/// Вибрато оперное: шире и медленнее хорового и приходит не сразу —
/// сначала голос стоит прямо, вибрато разливается к концу фразы.
const VIBRATO_HZ: f32 = 5.6;
const VIBRATO_DEPTH: f32 = 0.013;
/// Сколько секунд проходит до полного размаха вибрато.
const VIBRATO_RAMP_SECS: f32 = 0.45;
/// Спектр сольного голоса: основной тон плюс естественные обертоны.
/// Хора тут нет (несколько голосов с квинтой) — один певец.
const SOLO_PARTIALS: [(f32, f32); 3] = [(1.0, 0.55), (2.0, 0.22), (3.0, 0.1)];
/// Портаменто: голос идёт к новой высоте, а не начинает заново.
const PORTAMENTO_SECS: f32 = 0.09;
/// Гласная плывет ah → oh за ноту: голос живой, а не сэмплерный.
const AH: (f32, f32) = (730.0, 1090.0);
const OH: (f32, f32) = (500.0, 860.0);
/// Формантный резонанс: гласная читается, но не свистит.
/// Было 3.5 — гнусавый свист на верхах (резкость вокала).
const FORMANT_Q: f32 = 2.0;
/// Доля дыхательного шума: слышно воздух, не шипение.
const BREATH_GAIN: f32 = 0.022;
const BREATH_HZ: f32 = 2400.0;
const ATTACK_SECS: f32 = 0.09;
const AMP_DECAY_RATE: f32 = 1.7;
/// Длинный релиз вместо эха: хор плывёт сам, без дилэя-прослойки.
/// Было 1.8 — обрезанный хвост просил эхо; теперь хвост живёт
/// в голосе (плюс растяжка длительности ниже).
const MAX_RING_SECS: f32 = 3.0;
const FADE_SECS: f32 = 0.2;
/// Растяжка длительности ноты: вокальные фразы перекрываются,
/// линия плывёт, а не тычет отдельными чопами. Пула 4 голосов
/// хватает (вокал редкий, краж почти нет).
const RING_STRETCH: f32 = 1.6;
const VOICE_GAIN: f32 = 0.5;

#[derive(Clone, Default)]
pub struct VocalVoice {
    sample_rate: f32,
    /// Циклы от старта (общая высота стека).
    phase: f32,
    vib_phase: f32,
    form1: Svf,
    form2: Svf,
    breath_bp: Svf,
    noise: Noise,
    frequency: f32,
    /// Куда голос идет портаменто (текущая высота — frequency).
    target_frequency: f32,
    gain: f32,
    t: f32,
    ring_secs: f32,
    samples_left: usize,
    pub(crate) f1_hz: f32,
    pub(crate) f2_hz: f32,
    /// Рекуррентные огибающие вместо посэмпловых `exp`.
    amp_env: ExpDecay,
    scoop_env: ExpDecay,
}

impl Voice for VocalVoice {
    fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sane_sample_rate(sample_rate);
    }

    fn trigger(&mut self, note: &NoteEvent) {
        self.frequency = midi_to_freq(note.midi_pitch);
        self.target_frequency = self.frequency;
        let ring = (note.duration_secs * RING_STRETCH).clamp(0.05, MAX_RING_SECS);
        self.ring_secs = ring;
        self.samples_left = (ring * self.sample_rate) as usize;
        self.gain = note.velocity;
        self.t = 0.0;
        self.phase = 0.0;
        self.form1.reset();
        self.form2.reset();
        self.breath_bp.reset();
        // Гласная одна на всех («ah» → «oh» внутри ноты): певец держит
        // тембр, а не меняет слоги по нотам. Шум детерминирован нотой.
        self.f1_hz = AH.0;
        self.f2_hz = AH.1;
        self.amp_env = ExpDecay::start(AMP_DECAY_RATE, self.sample_rate);
        // Scoop: константа времени 30 мс → rate 1/0.03.
        self.scoop_env = ExpDecay::start(1.0 / 0.03, self.sample_rate);
        self.noise = Noise::new(super::dsp::note_seed(note, 0xB0CA));
    }

    fn next_sample(&mut self) -> f32 {
        if self.samples_left == 0 {
            return 0.0;
        }
        self.samples_left -= 1;

        let sample_rate = self.sample_rate;
        let a = (self.t / ATTACK_SECS).min(1.0);
        let attack = a * a * (3.0 - 2.0 * a);
        let amp = self.amp_env.next();
        let fade = release_fade(self.samples_left, (FADE_SECS * sample_rate) as usize);

        self.vib_phase = (self.vib_phase + VIBRATO_HZ / sample_rate) % 1.0;
        // Вибрато разливается к концу фразы: сначала голос стоит прямо.
        let vib_ramp = (self.t / VIBRATO_RAMP_SECS).min(1.0);
        let wobble = 1.0 + VIBRATO_DEPTH * vib_ramp * sin01(self.vib_phase);
        // Портаменто: голос идет к цели, а не прыгает на новую высоту.
        let glide_k = (-1.0 / (sample_rate * PORTAMENTO_SECS)).exp();
        self.frequency += (self.target_frequency - self.frequency) * (1.0 - glide_k);
        // Scoop: голос въезжает снизу (−4% за 30 мс), как живой onset.
        let scoop = 1.0 - 0.04 * self.scoop_env.next();
        let freq = self.frequency * wobble * scoop;

        self.phase = (self.phase + freq / sample_rate) % 4096.0;
        // Спектр сольного голоса: основной тон и обертоны, без хора.
        // Только синусы — алиасинга нет по построению.
        let mut raw = 0.0;
        for (ratio, amp) in SOLO_PARTIALS {
            raw += sin01(self.phase * ratio) * amp;
        }

        // Гласная плывет ah → oh за время ноты (доля пройденного кольца).
        // Срез квантуем до 1 Гц (0.14%, неслышно): кэш коэффициентов
        // SVF попадает сотни сэмплов подряд вместо пересчёта `tan`.
        let morph = (self.t / self.ring_secs).clamp(0.0, 1.0);
        let f1 = (AH.0 + (OH.0 - AH.0) * morph).round();
        let f2 = (AH.1 + (OH.1 - AH.1) * morph).round();
        let (_, vow1, _) = self.form1.process(raw, f1, FORMANT_Q, sample_rate);
        let (_, vow2, _) = self.form2.process(raw, f2, FORMANT_Q, sample_rate);
        // Дыхание: шум через полосу 2.4 кГц поверх гласных.
        let (_, breath, _) =
            self.breath_bp
                .process(self.noise.sample(), BREATH_HZ, 1.2, sample_rate);

        self.t += 1.0 / sample_rate;
        (vow1 + vow2 + breath * BREATH_GAIN) * self.gain * amp * attack * fade * VOICE_GAIN
    }

    fn is_active(&self) -> bool {
        self.samples_left > 0
    }

    fn has_legato(&self) -> bool {
        true
    }

    fn legato(&mut self, note: &NoteEvent) {
        // Портаменто: голос поет следующую ноту, не начиная заново —
        // оперная фраза едет голосом, а не отдельными слогами.
        self.target_frequency = midi_to_freq(note.midi_pitch);
        let ring = (note.duration_secs * RING_STRETCH).clamp(0.05, MAX_RING_SECS);
        self.ring_secs = ring;
        self.samples_left = self.samples_left.max((ring * self.sample_rate) as usize);
        self.gain = self.gain.max(note.velocity);
    }

    fn reset(&mut self) {
        self.samples_left = 0;
        self.t = 0.0;
        self.gain = 0.0;
        self.form1.reset();
        self.form2.reset();
        self.breath_bp.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Instrument, VoicePool};
    use super::*;

    #[test]
    fn onset_swells_softly_and_holds_tune() {
        // Хор входит мягко (атака 90 мс: начало тише середины —
        // никакого чоп-щелчка), высота держит строй со scoop-въездом.
        fn peak_near(samples: &[f32], sr: f32, lo: f32, hi: f32) -> f32 {
            let mut best = (0.0f32, 0.0f32);
            let mut f = lo;
            while f <= hi {
                let n = samples.len() as f32;
                let k = f / sr * n;
                let om = 2.0 * std::f32::consts::PI * k / n;
                let c = 2.0 * om.cos();
                let (mut s1, mut s2) = (0.0f32, 0.0f32);
                for &v in samples {
                    let s0 = v + c * s1 - s2;
                    s2 = s1;
                    s1 = s0;
                }
                let mag = (s1 * s1 + s2 * s2 - c * s1 * s2).sqrt() / n;
                if mag > best.1 {
                    best = (f, mag);
                }
                f += 2.0;
            }
            best.0
        }
        let sr = 44_100.0;
        let mut v = VocalVoice::default();
        v.set_sample_rate(sr);
        v.trigger(&NoteEvent::new(11, 57, 0.9, 1.2));
        let out: Vec<f32> = (0..(1.0 * sr) as usize).map(|_| v.next_sample()).collect();
        let rms = |lo: usize, hi: usize| {
            (out[lo..hi].iter().map(|s| s * s).sum::<f32>() / (hi - lo) as f32).sqrt()
        };
        // Мягкий вход: первые 30 мс заметно тише середины ноты.
        assert!(
            rms(0, 1323) < rms(8820, 22050) * 0.5,
            "voice barks instead of swelling"
        );
        // Строй: середина ноты — ля (220 Гц ± толерантность).
        let late = peak_near(&out[22050..44100], sr, 200.0, 240.0);
        assert!((late - 220.0).abs() < 12.0, "out of tune: {late}");
    }

    #[test]
    fn vocal_chop_sings_and_decays() {
        let sr = 44_100.0;
        let mut vocal = VoicePool::vocals(11, 4);
        vocal.set_sample_rate(sr);
        vocal.trigger(&NoteEvent::new(11, 69, 0.9, 1.2));
        let mut peak: f32 = 0.0;
        let mut first = 0.0;
        let mut second = 0.0;
        for i in 0..(2.0 * sr) as usize {
            let s = vocal.next_sample();
            assert!(s.is_finite());
            peak = peak.max(s.abs());
            if i < (1.0 * sr) as usize {
                first += s.abs();
            } else {
                second += s.abs();
            }
        }
        assert!(peak > 0.1, "vocal too quiet: {peak}");
        assert!(peak < 0.8, "vocal blows up: {peak}");
        assert!(second < first, "vocal does not decay");
        assert!(!vocal.is_active());
    }

    #[test]
    fn solo_voice_sings_one_vowel() {
        // Голос, а не чопы: гласная одна на всех нотах («ah» на старте),
        // спектр сольный (основной тон и обертоны).
        let sr = 44_100.0;
        for pitch in 60..72u8 {
            let mut v = VocalVoice::default();
            v.set_sample_rate(sr);
            v.trigger(&NoteEvent::new(11, pitch, 0.9, 1.0));
            assert_eq!((v.f1_hz, v.f2_hz), (AH.0, AH.1));
        }
        // Спектр ноты A3: пики на 220 (основной), 440 (октава), 660.
        // Сольный голос: хоровой квинты 3/2 в спектре нет.
        let mut v = VocalVoice::default();
        v.set_sample_rate(sr);
        v.trigger(&NoteEvent::new(11, 57, 0.9, 1.5));
        let out: Vec<f32> = (0..(1.0 * sr) as usize).map(|_| v.next_sample()).collect();
        let mag_at = |f: f32| {
            let n = out.len() as f32;
            let k = f / sr * n;
            let om = 2.0 * std::f32::consts::PI * k / n;
            let c = 2.0 * om.cos();
            let (mut s1, mut s2) = (0.0f32, 0.0f32);
            for &x in &out {
                let s0 = x + c * s1 - s2;
                s2 = s1;
                s1 = s0;
            }
            (s1 * s1 + s2 * s2 - c * s1 * s2).sqrt() / n
        };
        let root = mag_at(220.0);
        assert!(root > 1e-4, "no fundamental: {root}");
        assert!(mag_at(440.0) > root * 0.05, "no octave in solo stack");
        assert!(
            mag_at(330.0) < root * 0.05,
            "хоровая квинта в сольном голосе"
        );
    }
}
