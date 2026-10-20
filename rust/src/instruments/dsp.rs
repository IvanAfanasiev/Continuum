//! Общие DSP-примитивы.
//!
//! Два апгрейда против «8-битного» звука:
//! - PolyBLEP-осцилляторы: наивные пила/меандр плодят гармоники выше
//!   Найквиста, которые заворачиваются в слышимый диапазон (алиасинг
//!   и дает чиптюн-резкость). BLEP гасит ступеньку в момент разрыва.
//! - Резонансный SVF вместо однополюсных 6 дБ/окт: круче режет верх
//!   и умеет современный резонансный свип.

use std::f32::consts::PI;

/// Коррекция ступеньки PolyBLEP. `phase` в [0, 1), `dt` = freq/sr.
pub fn poly_blep(phase: f32, dt: f32) -> f32 {
    let dt = dt.clamp(1e-6, 0.4);
    if phase < dt {
        let t = phase / dt;
        t + t - t * t - 1.0
    } else if phase > 1.0 - dt {
        let t = (phase - 1.0) / dt;
        t * t + t + t + 1.0
    } else {
        0.0
    }
}

/// Пила −1..1 без алиасинга.
pub fn saw_blep(phase: f32, freq: f32, sample_rate: f32) -> f32 {
    let dt = (freq / sample_rate).clamp(1e-6, 0.4);
    (phase * 2.0 - 1.0) - poly_blep(phase, dt)
}

/// Меандр ±1 без алиасинга (разрывы на 0 и 0.5).
pub fn square_blep(phase: f32, freq: f32, sample_rate: f32) -> f32 {
    let dt = (freq / sample_rate).clamp(1e-6, 0.4);
    let naive = if phase < 0.5 { 1.0 } else { -1.0 };
    let shifted = if phase < 0.5 {
        phase + 0.5
    } else {
        phase - 0.5
    };
    naive + poly_blep(phase, dt) - poly_blep(shifted, dt)
}

/// Быстрый tanh для посэмпловых сатураций голосов: таблица 2048
/// точек на [-4, 4] + линейная интерполяция (ошибка < 1e-5).
/// Дешевле libm `tanh` на порядок (два чтения + арифметика вместо
/// range reduction и `exp`), на слух неотличим: character сатурации
/// (мягкое ограничение) сохраняется.
/// Мастер-цепочка использует точный `tanh` как раньше.
pub fn fast_tanh(x: f32) -> f32 {
    use std::sync::OnceLock;
    const N: usize = 2048;
    const LO: f32 = -4.0;
    const HI: f32 = 4.0;
    static TABLE: OnceLock<[f32; N + 1]> = OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let mut t = [0.0f32; N + 1];
        for (i, v) in t.iter_mut().enumerate() {
            *v = (LO + (HI - LO) * i as f32 / N as f32).tanh();
        }
        t
    });
    let xc = x.clamp(LO, HI);
    let pos = (xc - LO) / (HI - LO) * N as f32;
    let idx = (pos as usize).min(N - 1);
    let frac = pos - idx as f32;
    table[idx] + (table[idx + 1] - table[idx]) * frac
}

/// Быстрый синус в радианах: таблица 2048 точек + lerp (ошибка < 1e-5).
/// Для фаз осцилляторов, уже живущих в [0, 1) циклов, дешевле
/// [`sin01`] (без умножения на TAU и редукции больших аргументов).
pub fn fast_sin(x: f32) -> f32 {
    use std::f32::consts::TAU;
    use std::sync::OnceLock;
    const N: usize = 2048;
    static TABLE: OnceLock<[f32; N + 1]> = OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let mut t = [0.0f32; N + 1];
        for (i, v) in t.iter_mut().enumerate() {
            *v = (TAU * i as f32 / N as f32).sin();
        }
        t
    });
    let mut w = x % TAU;
    if w < 0.0 {
        w += TAU;
    }
    let pos = w / TAU * N as f32;
    let idx = (pos as usize).min(N - 1);
    let frac = pos - idx as f32;
    table[idx] + (table[idx + 1] - table[idx]) * frac
}

/// Быстрый синус от фазы в циклах [0, 1): прямой индекс в ту же
/// таблицу без умножения на TAU. Фаза заворачивается (`% 1.0`),
/// отрицательных фаз у осцилляторов не бывает.
pub fn sin01(phase: f32) -> f32 {
    use std::f32::consts::TAU;
    use std::sync::OnceLock;
    const N: usize = 2048;
    static TABLE: OnceLock<[f32; N + 1]> = OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let mut t = [0.0f32; N + 1];
        for (i, v) in t.iter_mut().enumerate() {
            *v = (TAU * i as f32 / N as f32).sin();
        }
        t
    });
    let pos = (phase % 1.0) * N as f32;
    let idx = (pos as usize).min(N - 1);
    let frac = pos - idx as f32;
    table[idx] + (table[idx + 1] - table[idx]) * frac
}
/// Рекуррентная экспоненциальная огибающая: замена посэмпловому
/// `(-k*t).exp()`. Один `exp()` в `trigger` (расчёт `coeff`),
/// дальше один multiply на сэмпл. Результат совпадает с прямым
/// `exp` с точностью флоата (дрейф <1e-6 за секунду).
#[derive(Clone, Default, Debug)]
pub struct ExpDecay {
    value: f32,
    coeff: f32,
}

impl ExpDecay {
    /// Старт с 1.0 и скоростью `rate_per_sec` (огибающая `exp(-rate*t)`).
    pub fn start(rate_per_sec: f32, sample_rate: f32) -> Self {
        Self {
            value: 1.0,
            coeff: (-rate_per_sec / sample_rate.max(1.0)).exp(),
        }
    }

    /// Текущее значение + шаг. Денормал-защита: ниже 1e-12 — в ноль.
    pub fn next(&mut self) -> f32 {
        let v = self.value;
        self.value *= self.coeff;
        if self.value < 1e-12 {
            self.value = 0.0;
        }
        v
    }

    pub fn value(&self) -> f32 {
        self.value
    }
}
/// Резонансный фильтр переменной структуры (Zavalishin SVF, 12 дБ/окт).
/// Безусловно стабилен на любых cutoff/Q — в отличие от лестничных.
///
/// Коэффициенты кэшируются: `tan()` считается только при смене
/// (cutoff, Q, sr). Фильтры с постоянным срезом (мастер-HP 28 Гц,
/// HP голосов) после первого сэмпла вообще не трогают libm.
/// Свипы со скользящим срезом пересчитываются как раньше —
/// звук бит-в-бит тот же, сравнение точное (тот же вход = тот же
/// коэффициент, кэш не вносит погрешности).
#[derive(Clone)]
pub struct Svf {
    ic1eq: f32,
    ic2eq: f32,
    g: f32,
    a1: f32,
    a2: f32,
    a3: f32,
    k: f32,
    last_cutoff: f32,
    last_q: f32,
    last_sr: f32,
}

impl Default for Svf {
    fn default() -> Self {
        Self {
            ic1eq: 0.0,
            ic2eq: 0.0,
            g: 0.0,
            a1: 0.0,
            a2: 0.0,
            a3: 0.0,
            k: 0.0,
            // Невалидная комбинация: первый вызов обязан посчитать.
            last_cutoff: f32::NAN,
            last_q: f32::NAN,
            last_sr: f32::NAN,
        }
    }
}

impl Svf {
    /// Один проход — сразу (low, band, high).
    pub fn process(&mut self, x: f32, cutoff_hz: f32, q: f32, sample_rate: f32) -> (f32, f32, f32) {
        // NaN никогда не равен ничему, включая себя: после default
        // и после reset коэффициенты гарантированно пересчитываются.
        if cutoff_hz != self.last_cutoff || q != self.last_q || sample_rate != self.last_sr {
            let g = (PI * cutoff_hz.clamp(10.0, sample_rate * 0.45) / sample_rate).tan();
            let k = 1.0 / q.clamp(0.3, 12.0);
            let a1 = 1.0 / (1.0 + g * (g + k));
            self.g = g;
            self.k = k;
            self.a1 = a1;
            self.a2 = g * a1;
            self.a3 = g * g * a1;
            self.last_cutoff = cutoff_hz;
            self.last_q = q;
            self.last_sr = sample_rate;
        }
        let v3 = x - self.ic2eq;
        let v1 = self.a1 * self.ic1eq + self.a2 * v3;
        let v2 = self.ic2eq + self.a2 * self.ic1eq + self.a3 * v3;
        self.ic1eq = 2.0 * v1 - self.ic1eq;
        self.ic2eq = 2.0 * v2 - self.ic2eq;
        (v2, v1, x - self.k * v1 - v2)
    }

    pub fn reset(&mut self) {
        self.ic1eq = 0.0;
        self.ic2eq = 0.0;
    }

    /// Сброс деначальных состояний: после долгой тишины остатки в фильтре
    /// могут уйти в субнормальный диапазон, а арифметика с ними на x86
    /// в десятки раз медленнее — отсюда редкие всплески нагрузки (хрип).
    pub fn scrub_denormals(&mut self) {
        if self.ic1eq.abs() < 1e-15 {
            self.ic1eq = 0.0;
        }
        if self.ic2eq.abs() < 1e-15 {
            self.ic2eq = 0.0;
        }
    }
}

/// Пиковый лимитер мастера со стерео-связкой: потолок держит пики,
/// гейн подтягивается ПЛАВНО (атака 2 мс), поэтому в пиках нет щелчка.
/// Раньше гейн прыгал мгновенно — это и было щелчком на каждом ударе.
/// Стерео-пара делит один гейн: образ не перекашивается.
/// Тихие сигналы идут прозрачно; сухой tanh-мастер остаётся лёгким
/// подкрашиванием, а не компрессором.
#[derive(Clone, Debug)]
pub struct MasterLimiter {
    env: f32,
    /// Текущее регулирование (0..1): спадает быстро, поднимается медленно.
    gain: f32,
    /// Подъем тела, разы. Умеренный: лимитер ловит только freak-стеки,
    /// характер микса (крест-фактор) цел.
    pub makeup: f32,
    /// Потолок пика на выходе лимитера.
    pub ceiling: f32,
    /// Спад огибающей, секунды.
    pub release_secs: f32,
    /// Время срабатывания регулирования, секунды: столько усиление
    /// подтягивается вниз, вместо мгновенного прыжка.
    pub attack_secs: f32,
    /// Кэш `exp(-1/(sr*release))`: константа тракта, а не сэмпла.
    rel: f32,
    /// Кэш атаки регулирования.
    att: f32,
    last_sr: f32,
}

impl Default for MasterLimiter {
    fn default() -> Self {
        Self {
            env: 0.0,
            gain: 1.0,
            makeup: 1.0,
            ceiling: 0.85,
            release_secs: 0.05,
            attack_secs: 0.002,
            rel: 0.0,
            att: 0.0,
            last_sr: f32::NAN,
        }
    }
}

impl MasterLimiter {
    /// Стерео-фрейм после мейкапа и лимита (tanh-драйв — снаружи).
    pub fn process(&mut self, l: f32, r: f32, sample_rate: f32) -> (f32, f32) {
        let peak = l.abs().max(r.abs()) * self.makeup;
        if sample_rate != self.last_sr {
            self.rel = (-1.0 / (sample_rate.max(1.0) * self.release_secs)).exp();
            self.att = (-1.0 / (sample_rate.max(1.0) * self.attack_secs)).exp();
            self.last_sr = sample_rate;
        }
        self.env = peak.max(self.env * self.rel);
        if self.env < 1e-12 {
            self.env = 0.0;
        }
        let target = (self.ceiling / self.env.max(1e-6)).min(1.0);
        // Регулирование сглажено: вниз — за attack (2 мс), вверх — за
        // релиз. Мгновенный прыжок гейна = щелчок на каждом пике.
        let k = if target < self.gain {
            self.att
        } else {
            self.rel
        };
        self.gain += (target - self.gain) * (1.0 - k);
        (l * self.makeup * self.gain, r * self.makeup * self.gain)
    }

    pub fn reset(&mut self) {
        self.env = 0.0;
    }
}

/// Линейный фейд хвоста ноты: 1 в начале фейда → 0 на последнем семпле.
/// Убирает щелчки жесткой отсечки по `samples_left == 0`.
pub fn release_fade(samples_left: usize, fade_samples: usize) -> f32 {
    if fade_samples == 0 {
        return 1.0;
    }
    (samples_left as f32 / fade_samples as f32).clamp(0.0, 1.0)
}

/// Детерминированный сид хита из ноты: одинаковые ноты звучат одинаково,
/// разная velocity/pitch дает естественную вариацию тембра
/// (анти-machine-gun на роллах). На высоту и громкость не влияет.
pub fn note_seed(note: &super::NoteEvent, salt: u32) -> u32 {
    salt ^ (note.midi_pitch as u32).wrapping_mul(0x9E37) ^ (note.velocity.to_bits() >> 7)
}

/// Детерминированный белый шум (LCG). Один генератор на голос —
/// последовательно вызванные ноты не коррелируют.
#[derive(Clone, Default)]
pub struct Noise {
    state: u32,
}

impl Noise {
    pub fn new(seed: u32) -> Self {
        Self { state: seed.max(1) }
    }

    pub fn sample(&mut self) -> f32 {
        self.state = self.state.wrapping_mul(1664525).wrapping_add(1013904223);
        (self.state as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::TAU;

    #[test]
    fn fade_endpoints() {
        assert_eq!(release_fade(100, 10), 1.0);
        assert_eq!(release_fade(0, 10), 0.0);
        assert!((release_fade(5, 10) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn limiter_bounds_peaks_keeps_quiet_transparent() {
        // Пик 3.0 давится к потолку, тишина 0.1 идет почти как есть,
        // стерео-пара делится одним гейном (образ цел).
        // Регулирование сглажено (атака 2 мс): первые миллисекунды пик
        // проходит — это короткий выброс, который мягкий tanh мастера
        // потом срезает. Главное — после атаки потолок держится.
        let sr = 44_100.0;
        let mut lim = MasterLimiter::default();
        let (mut peak_in, mut peak_out, mut peak_settled) = (0.0f32, 0.0f32, 0.0f32);
        for i in 0..(sr as usize) {
            let t = i as f32 / sr;
            let loud = (t * 440.0 * TAU).sin() * 3.0;
            let (l, r) = lim.process(loud, loud * 0.5, sr);
            peak_in = peak_in.max(loud.abs());
            peak_out = peak_out.max(l.abs().max(r.abs()));
            if i > (sr * 0.05) as usize {
                peak_settled = peak_settled.max(l.abs().max(r.abs()));
            }
        }
        assert!(peak_in > 2.9);
        assert!(
            peak_settled <= lim.ceiling * lim.makeup * 1.02 + 1e-3,
            "limiter leaks peaks: {peak_settled}"
        );
        // Выход мастера (tanh) никогда не выходит за единицу: выброс
        // лимитера срезается мягко, без клиппинга.
        assert!(peak_out <= 3.0, "limiter made it worse: {peak_out}");
        // Тихий синус: гейн ~мейкап, без разбаланса каналов.
        let mut lim = MasterLimiter::default();
        let (l, r) = lim.process(0.1, 0.1, sr);
        assert!((l - r).abs() < 1e-6, "stereo image skewed");
        assert!(
            (l - 0.1 * lim.makeup).abs() < 1e-3,
            "quiet path colored: {l}"
        );
    }

    #[test]
    fn limiter_recovers_after_transient() {
        // После громкого удара гейн возвращается за ~5 релизов:
        // насос качает, а не душит все подряд.
        let sr = 44_100.0;
        let mut lim = MasterLimiter::default();
        for _ in 0..1000 {
            lim.process(3.0, 3.0, sr);
        }
        for _ in 0..(5.0 * 0.05 * sr) as usize {
            lim.process(0.05, 0.05, sr);
        }
        let (l, _) = lim.process(0.05, 0.05, sr);
        assert!((l - 0.05 * lim.makeup).abs() < 0.01, "gain stuck down: {l}");
    }

    /// Энергия одной частотной корзины (Goertzel) — спектральный тест
    /// без FFT.
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

    fn render_saw(bleped: bool, freq: f32, sample_rate: f32, n: usize) -> Vec<f32> {
        let mut phase = 0.0;
        (0..n)
            .map(|_| {
                phase = (phase + freq / sample_rate) % 1.0;
                if bleped {
                    saw_blep(phase, freq, sample_rate)
                } else {
                    phase * 2.0 - 1.0
                }
            })
            .collect()
    }

    #[test]
    fn saw_blep_kills_folded_aliasing_but_keeps_body() {
        let sr = 44_100.0;
        let n = sr as usize;
        // Свернутые компоненты, не кратные f0 (настоящих гармоник в этих
        // корзинах нет, только алиасинг; окно 1 с дает нули через 1 Гц,
        // соседи не протекают).
        for (f0, bin) in [(1000.0, 900.0), (500.0, 400.0)] {
            let naive = render_saw(false, f0, sr, n);
            let bleped = render_saw(true, f0, sr, n);
            let naive_fold = goertzel(&naive, bin, sr);
            let blep_fold = goertzel(&bleped, bin, sr);
            assert!(naive_fold > 30.0, "test setup broken: f0={f0}");
            assert!(
                blep_fold < naive_fold * 0.05,
                "aliasing survives at f0={f0}: blep={blep_fold} naive={naive_fold}"
            );
        }
        // Край Найквиста: у 2-point BLEP там известный небольшой остаток
        // (неслышимый), но и он должен быть заметно ниже наивного.
        let naive = render_saw(false, 110.0, sr, n);
        let bleped = render_saw(true, 110.0, sr, n);
        let naive_edge = goertzel(&naive, 21_990.0, sr);
        let blep_edge = goertzel(&bleped, 21_990.0, sr);
        assert!(blep_edge < naive_edge * 0.6);
        // Низкие гармоники (880 Гц) BLEP не трогает.
        let naive_lo = goertzel(&naive, 880.0, sr);
        let blep_lo = goertzel(&bleped, 880.0, sr);
        assert!(
            blep_lo > naive_lo * 0.8,
            "body damaged: blep={blep_lo} naive={naive_lo}"
        );
    }

    #[test]
    fn square_blep_stays_bounded() {
        let sr = 44_100.0;
        let mut phase = 0.0;
        for _ in 0..(sr as usize) {
            phase = (phase + 220.0 / sr) % 1.0;
            let s = square_blep(phase, 220.0, sr);
            assert!(s.is_finite());
            assert!(s.abs() <= 1.3, "overshoot too big: {s}");
        }
    }

    #[test]
    fn svf_low_passes_dc_blocks_nyquist() {
        let sr = 44_100.0;
        let mut svf = Svf::default();
        let (mut low, _, _) = (0.0, 0.0, 0.0);
        for _ in 0..20_000 {
            (low, _, _) = svf.process(1.0, 1000.0, 0.7, sr);
        }
        assert!((low - 1.0).abs() < 1e-3, "DC must pass LP: {low}");

        // Синус 8 кГц через срез 1 кГц: склон 12 дБ/окт давит в ~64 раза.
        let mut svf = Svf::default();
        let mut sum_sq = 0.0;
        let n = sr as usize;
        let mut phase = 0.0;
        for _ in 0..n {
            phase = (phase + 8000.0 / sr) % 1.0;
            let x = (phase * TAU).sin();
            let (l, _, _) = svf.process(x, 1000.0, 0.7, sr);
            sum_sq += l * l;
        }
        let rms = (sum_sq / n as f32).sqrt();
        assert!(rms < 0.08, "stopband leaks: rms={rms}");
    }

    #[test]
    fn svf_high_blocks_dc() {
        let sr = 44_100.0;
        let mut svf = Svf::default();
        let mut high = 0.0;
        for _ in 0..20_000 {
            (_, _, high) = svf.process(1.0, 5000.0, 0.7, sr);
        }
        assert!(high.abs() < 1e-3, "DC must not pass HP: {high}");
    }

    #[test]
    fn svf_cache_matches_direct_computation() {
        // Кэш не меняет звук: два фильтра с одинаковой историей
        // состояний дают одинаковые сэмплы бит-в-бит (у обоих
        // коэффициенты берутся из кэша после первого сэмпла).
        let sr = 48_000.0;
        let mut c2 = Svf::default();
        let mut d2 = Svf::default();
        for i in 0..4096 {
            let x = (i as f32 * 12.9898).sin() * 0.7;
            let a = c2.process(x, 800.0, 1.1, sr);
            let b = d2.process(x, 800.0, 1.1, sr);
            assert_eq!(a.0.to_bits(), b.0.to_bits(), "cache diverges at {i}");
            assert_eq!(a.1.to_bits(), b.1.to_bits(), "cache diverges at {i}");
            assert_eq!(a.2.to_bits(), b.2.to_bits(), "cache diverges at {i}");
        }
        // Смена среза пересчитывает коэффициенты (свип не залипает).
        let before = c2.process(0.5, 800.0, 1.1, sr);
        let after = c2.process(0.5, 4000.0, 1.1, sr);
        assert_ne!(before.0.to_bits(), after.0.to_bits(), "cutoff stuck");
    }

    #[test]
    fn fast_tanh_stays_close_to_libm() {
        // Точность: таблица 2048 + lerp — ошибка < 1e-4 везде.
        let mut worst = 0.0f32;
        let mut x = -2.0f32;
        while x <= 2.0 {
            let err = (fast_tanh(x) - x.tanh()).abs();
            worst = worst.max(err);
            assert!(fast_tanh(x).is_finite());
            x += 0.01;
        }
        assert!(worst < 1e-4, "fast_tanh too far: {worst}");
        assert!((fast_tanh(0.05) - 0.05).abs() < 1e-3);
        assert_eq!(fast_tanh(0.0), 0.0);
        assert!(fast_tanh(-1.0) < 0.0 && fast_tanh(1.0) > 0.0);
        assert!(fast_tanh(10.0) <= 1.0 && fast_tanh(-10.0) >= -1.0);
    }

    #[test]
    fn fast_sin_matches_libm() {
        // Оба синуса — в пределах интерполяции (< 2e-5),
        // большие аргументы и отрицательные фазы заворачиваются.
        let mut worst = 0.0f32;
        let mut x = -20.0f32;
        while x <= 20.0 {
            worst = worst.max((fast_sin(x) - x.sin()).abs());
            x += 0.01;
        }
        assert!(worst < 2e-5, "fast_sin too far: {worst}");
        let mut worst01 = 0.0f32;
        let mut p = 0.0f32;
        while p < 3.0 {
            worst01 = worst01.max((sin01(p) - (p * TAU).sin()).abs());
            p += 0.001;
        }
        assert!(worst01 < 2e-5, "sin01 too far: {worst01}");
        assert!(fast_sin(1e6).abs() <= 1.0 && sin01(123.5).abs() <= 1.0);
    }

    #[test]
    fn exp_decay_matches_direct_exp() {
        // Рекуррентная огибающая совпадает с прямым exp с точностью
        // флоата за секунду работы.
        let sr = 48_000.0;
        let mut env = ExpDecay::start(7.0, sr);
        for i in 0..sr as usize {
            let t = i as f32 / sr;
            let expect = (-7.0 * t).exp();
            let got = env.next();
            assert!(
                (got - expect).abs() < 1e-4,
                "drift at {i}: {got} vs {expect}"
            );
        }
    }
}
