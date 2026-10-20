use super::{
    dsp::{fast_tanh, release_fade, Noise, Svf},
    sane_sample_rate, NoteEvent, Voice,
};
use crate::core::midi_to_freq;

/// Электрогитара: струна Карплуса-Стронга + кусачий burst + лёгкий
/// драйв. Чёткая атака самой струны (не щелчок — это не акустика):
/// яркое возбуждение, собранный фронт, верх умирает сам по ходу звона.
/// От коротких чопов до длинных протяжных линий — диапазоном управляет
/// длительность ноты: короткие идут одиночными ударами, длинные
/// аккорды — веером. Для лоуфай-боя поверх Rhodes и рок-риффов.
/// Длительность сустейна струны (открытые струны звенят, глушеные — нет).
const RING_T60_SECS: f32 = 1.4;
const RING_T60_MUTED_SECS: f32 = 0.12;
const MAX_KS_LEN: usize = 2048;
const LP_HZ: f32 = 3800.0;
const LP_MUTED_HZ: f32 = 1800.0;
const LP_Q: f32 = 0.7;
const ATTACK_SECS: f32 = 0.006;
/// Тише этого — ghost-нота с приглушенными струнами.
const MUTE_VELOCITY: f32 = 0.6;
/// Расгеадо: сдвиги ударов и их акценты (сумма нормируется).
/// 16 мс между струнами — штрихи слышны раздельно, как бой,
/// а не один удар с тремоло.
const STRUM_GAP_SECS: f32 = 0.016;
const STRUM_GAP_MUTED_SECS: f32 = 0.008;
const STRUM_ACCENTS: [f32; 3] = [1.0, 0.7, 0.5];
const STRUM_NORM: f32 = 1.0 / (1.0 + 0.7 + 0.5);
const MAX_RING_SECS: f32 = 1.5;
const FADE_SECS: f32 = 0.05;
/// Выход normирован под микс: струна тише синта, но слышна (тест-пин).
/// Струна — это рифф (в rock она ведет), поэтому уровень держим рядом
/// со снейром: KS-усреднение гасит верх, и без компенсации рифф уезжал
/// на 6 дБ под ударные.
const VOICE_GAIN: f32 = 1.55;
/// Характер соло: звон дольше (сустейн-линия), верх открыт (читаемость
/// перебора), лёгкий драйв (поющая компрессия вместо плоского звона).
/// Характер риффа: кусачий burst, собранный драйв чопов.
const LEAD_RING_T60_SECS: f32 = 2.4;
const LEAD_LP_HZ: f32 = 5200.0;
const LEAD_DRIVE: f32 = 1.6;
/// Потолок длины ноты соло: протяжные линии не обрываются на 1.5 с.
const LEAD_MAX_RING_SECS: f32 = 3.0;
const RIFF_DRIVE: f32 = 1.4;
/// Яркость возбуждения (альфа однополюсника burst): было 0.5 —
/// глухой удар без верхов. Рифф кусается, соло звенит.
const RIFF_BURST_ALPHA: f32 = 0.7;
const LEAD_BURST_ALPHA: f32 = 0.85;

#[derive(Clone)]
pub struct GuitarVoice {
    sample_rate: f32,
    ks_buf: Vec<f32>,
    ks_pos: usize,
    ks_len: usize,
    damp: f32,
    filter: Svf,
    noise: Noise,
    frequency: f32,
    gain: f32,
    muted: bool,
    strums: bool,
    t: f32,
    samples_left: usize,
    /// Характер: сустейн струны, срез верха и драйв выхода.
    /// Рифф — сухо и собранно, соло — дольше, ярче, с лёгкой
    /// сатурацией (поющая сустейн-линия вместо плоского звона).
    ring_t60: f32,
    lp_hz: f32,
    drive: f32,
    /// Яркость burst возбуждения (см. RIFF/LEAD_BURST_ALPHA).
    burst_alpha: f32,
    /// Потолок длины ноты, с: короткие чопы обрываются рано,
    /// длинные линии тянутся (протяжные звуки соло).
    max_ring: f32,
}

impl GuitarVoice {
    /// Рифф: короткие чопы, ладонь, кусачий burst, собранный драйв.
    pub fn riff() -> Self {
        Self {
            sample_rate: 44_100.0,
            ks_buf: Vec::new(),
            ks_pos: 0,
            ks_len: 0,
            damp: 0.0,
            filter: Svf::default(),
            noise: Noise::default(),
            frequency: 0.0,
            gain: 0.0,
            muted: false,
            strums: false,
            t: 0.0,
            samples_left: 0,
            ring_t60: RING_T60_SECS,
            lp_hz: LP_HZ,
            drive: RIFF_DRIVE,
            burst_alpha: RIFF_BURST_ALPHA,
            max_ring: MAX_RING_SECS,
        }
    }

    /// Соло/перебор: длинный звон, открытый верх, поющий драйв.
    pub fn lead() -> Self {
        let mut v = Self::riff();
        v.ring_t60 = LEAD_RING_T60_SECS;
        v.lp_hz = LEAD_LP_HZ;
        v.drive = LEAD_DRIVE;
        v.burst_alpha = LEAD_BURST_ALPHA;
        v.max_ring = LEAD_MAX_RING_SECS;
        v
    }
}

impl Default for GuitarVoice {
    fn default() -> Self {
        Self::riff()
    }
}

impl Voice for GuitarVoice {
    fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sane_sample_rate(sample_rate);
    }

    fn trigger(&mut self, note: &NoteEvent) {
        let sample_rate = self.sample_rate;
        // Хуманизация строя и звона от ноты: ±4 цента и ±10% сустейна.
        // Живая струна чуть разная каждый удар (машинная одинаковость
        // = половина «синтетичности»). Детерминировано нотой.
        let seed = super::dsp::note_seed(note, 0x6174);
        let detune = 1.0 + ((seed % 200) as f32 / 100.0 - 1.0) * 0.0023;
        let ring_var = 0.9 + 0.2 * (((seed >> 9) % 100) as f32 / 100.0);
        self.frequency = midi_to_freq(note.midi_pitch) * detune;
        let ring = note.duration_secs.clamp(0.05, self.max_ring);
        self.samples_left = (ring * sample_rate) as usize;
        self.gain = note.velocity;
        // Тихие ghost-ноты играются с глушением струн.
        self.muted = note.velocity < MUTE_VELOCITY;
        // Бой веером — только длинные акцентированные аккорды: короткие
        // чопы идут одиночными ударами (иначе каждый 16-й чоп звучит
        // 32-миллисекундной двойкой и перебор мажется).
        self.strums = note.velocity > 0.75 && note.duration_secs >= 0.3;
        self.t = 0.0;
        // Струна: линия задержки на период тона + затухание под T60.
        // Глушеные звенят 0.12 с (ладонь), открытые — 1.1 с.
        self.ks_len = (sample_rate / self.frequency.max(20.0)).round() as usize;
        self.ks_len = self.ks_len.clamp(8, MAX_KS_LEN);
        let t60 = if self.muted {
            RING_T60_MUTED_SECS
        } else {
            // Хуманизация звона: сустейн дышит от ноты к ноте.
            self.ring_t60 * ring_var
        };
        self.damp = (-6.908 * self.ks_len as f32 / (t60 * sample_rate)).exp();
        // Щипок возбуждения: шум через лёгкий lowpass (яркость — характером).
        // Burst стабилен в пределах кванта velocity (иначе энергия прыгает
        // вдвое от удара к удару), дышат только строй и звон (ниже) —
        // живо, но не расхлябанно.
        // Нормируем к полному размаху: мягкость дает фильтр, а не тишина.
        let vq = (note.velocity * 4.0) as u32;
        self.noise = Noise::new(0x6174 ^ (note.midi_pitch as u32).wrapping_mul(0x9E37) ^ vq);
        self.ks_buf.resize(self.ks_len, 0.0);
        // Возбуждение: шум через лёгкий lowpass — кусачий фронт, верх
        // умирает сам по ходу звона (KS-усреднение). Было alpha 0.5 —
        // глухой удар без верхов (половина «калимбы»).
        let alpha = self.burst_alpha;
        let mut lp = 0.0f32;
        let mut peak = 0.0f32;
        for b in self.ks_buf.iter_mut() {
            let n = self.noise.sample();
            lp += alpha * (n - lp);
            *b = lp;
            peak = peak.max(lp.abs());
        }
        if peak > 1e-6 {
            let norm = 0.9 / peak;
            for b in self.ks_buf.iter_mut() {
                *b *= norm;
            }
        }
        self.ks_pos = 0;
        self.filter.reset();
    }

    fn next_sample(&mut self) -> f32 {
        if self.samples_left == 0 || self.ks_len == 0 {
            return 0.0;
        }
        self.samples_left -= 1;

        let sample_rate = self.sample_rate;
        let a = (self.t / ATTACK_SECS).min(1.0);
        let attack = a * a * (3.0 - 2.0 * a);
        let fade = release_fade(self.samples_left, (FADE_SECS * sample_rate) as usize);

        // Расгеадо только на акцентах: веер из трех ударов. Обычный удар —
        // одиночный щипок с той же атакой (иначе фингерпикинг двоится).
        let gap = if self.muted {
            STRUM_GAP_MUTED_SECS
        } else {
            STRUM_GAP_SECS
        };
        let mut strum = 0.0;
        if self.strums {
            for (k, accent) in STRUM_ACCENTS.iter().enumerate() {
                let tau = self.t - k as f32 * gap;
                if tau > 0.0 {
                    let u = (tau / ATTACK_SECS).min(1.0);
                    strum += accent * u * u * (3.0 - 2.0 * u);
                }
            }
            strum *= STRUM_NORM;
        } else {
            let u = (self.t / ATTACK_SECS).min(1.0);
            strum = u * u * (3.0 - 2.0 * u);
        }

        // Карплус-Стронг: усреднение соседей гасит верх само собой —
        // струна темнеет по мере звона, как живая.
        let cur = self.ks_buf[self.ks_pos];
        let nxt = self.ks_buf[(self.ks_pos + 1) % self.ks_len];
        self.ks_buf[self.ks_pos] = self.damp * 0.5 * (cur + nxt);
        self.ks_pos = (self.ks_pos + 1) % self.ks_len;

        let cutoff = if self.muted { LP_MUTED_HZ } else { self.lp_hz };
        let (warm, _, _) = self.filter.process(cur, cutoff, LP_Q, sample_rate);

        self.t += 1.0 / sample_rate;
        fast_tanh(warm * self.drive) * self.gain * attack * fade * strum * VOICE_GAIN
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

    fn guitars() -> VoicePool {
        VoicePool::guitars(16, 6)
    }

    #[test]
    fn short_chops_dont_fan() {
        // Короткие чопы — одиночные удары (32мс веер на каждом 16-м
        // мажет быстрый перебор), длинные аккорды — веером.
        fn is_strum(dur: f32) -> bool {
            let mut v = GuitarVoice::riff();
            v.set_sample_rate(44_100.0);
            v.trigger(&NoteEvent::new(14, 57, 0.9, dur));
            v.strums
        }
        assert!(!is_strum(0.1), "chop fanned");
        assert!(is_strum(1.0), "long chord not strummed");
    }

    #[test]
    fn lead_sustains_longer_than_riff() {
        // Соло держит звон дольше риффа (сустейн-линия vs чоп):
        // энергия второй половины у лида выше при той же ноте.
        fn late_energy(lead: bool) -> f32 {
            let sr = 44_100.0;
            let mut pool = if lead {
                VoicePool::guitar_leads(20, 1)
            } else {
                VoicePool::guitars(14, 1)
            };
            pool.set_sample_rate(sr);
            pool.trigger(&NoteEvent::new(14, 57, 0.9, 1.5));
            let mut sum = 0.0;
            for i in 0..(1.5 * sr) as usize {
                let s = pool.next_sample();
                assert!(s.is_finite());
                if i >= (0.75 * sr) as usize {
                    sum += s.abs();
                }
            }
            sum
        }
        let riff = late_energy(false);
        let lead = late_energy(true);
        assert!(lead > riff, "solo decays like riff: {lead} vs {riff}");
    }

    #[test]
    fn guitar_plucks_warm() {
        let sr = 44_100.0;
        let mut guitar = guitars();
        guitar.set_sample_rate(sr);
        guitar.trigger(&NoteEvent::new(16, 64, 0.9, 1.0));
        let mut peak: f32 = 0.0;
        let mut first = 0.0;
        let mut second = 0.0;
        for i in 0..(sr as usize) {
            let s = guitar.next_sample();
            assert!(s.is_finite());
            peak = peak.max(s.abs());
            if i < (0.5 * sr) as usize {
                first += s.abs();
            } else {
                second += s.abs();
            }
        }
        assert!(peak > 0.15, "guitar too quiet: {peak}");
        assert!(second < first, "guitar does not decay");
    }

    #[test]
    fn trigger_humanizes_tuning_and_ring() {
        // Та же высота при разной velocity (джиттер каждого удара) —
        // чуть разный строй (±4 цента) и звон; при той же velocity —
        // бит-в-бит. Машинная одинаковость уходит, детерминизм остается.
        fn ring(vel: f32) -> Vec<f32> {
            let sr = 44_100.0;
            let mut guitar = guitars();
            guitar.set_sample_rate(sr);
            guitar.trigger(&NoteEvent::new(14, 64, vel, 0.5));
            (0..(0.3 * sr) as usize)
                .map(|_| {
                    let s = guitar.next_sample();
                    assert!(s.is_finite());
                    s
                })
                .collect()
        }
        let a = ring(0.85);
        assert_eq!(a, ring(0.85), "same note must repeat");
        let c = ring(0.87);
        assert_ne!(a, c, "humanization dead");
        // Строй держится: энергии в пределах 3 дБ.
        let e = |v: &[f32]| v.iter().map(|s| s * s).sum::<f32>();
        let ratio = e(&a) / e(&c).max(1e-9);
        assert!((ratio - 1.0).abs() < 0.5, "tuning drifted: {ratio}");
    }

    #[test]
    fn muted_ghost_dies_fast() {
        // Тихая нота (ghost) глушится: во второй половине почти тишина,
        // тогда как открытая в том же окне еще звучит.
        let sr = 44_100.0;
        let energy = |vel: f32| {
            let mut guitar = guitars();
            guitar.set_sample_rate(sr);
            guitar.trigger(&NoteEvent::new(16, 64, vel, 1.0));
            let mut late = 0.0;
            for i in 0..(sr as usize) {
                let s = guitar.next_sample();
                assert!(s.is_finite());
                if i >= (0.5 * sr) as usize {
                    late += s.abs();
                }
            }
            late
        };
        let open = energy(0.9);
        let muted = energy(0.4);
        assert!(muted < open * 0.2, "mute does not mute: {muted} vs {open}");
    }
}
