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

/// Резонансный фильтр переменной структуры (Zavalishin SVF, 12 дБ/окт).
/// Безусловно стабилен на любых cutoff/Q — в отличие от лестничных.
#[derive(Clone, Default)]
pub struct Svf {
    ic1eq: f32,
    ic2eq: f32,
}

impl Svf {
    /// Один проход — сразу (low, band, high).
    pub fn process(&mut self, x: f32, cutoff_hz: f32, q: f32, sample_rate: f32) -> (f32, f32, f32) {
        let g = (PI * cutoff_hz.clamp(10.0, sample_rate * 0.45) / sample_rate).tan();
        let k = 1.0 / q.clamp(0.3, 12.0);
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        let a3 = g * a2;
        let v3 = x - self.ic2eq;
        let v1 = a1 * self.ic1eq + a2 * v3;
        let v2 = self.ic2eq + a2 * self.ic1eq + a3 * v3;
        self.ic1eq = 2.0 * v1 - self.ic1eq;
        self.ic2eq = 2.0 * v2 - self.ic2eq;
        (v2, v1, x - k * v1 - v2)
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

/// Пиковый лимитер мастера со стерео-связкой: мейкап тянет тело
/// вверх, огибающая пика (атака мгновенная, спад 50 мс) давит гейн,
/// не превышая потолка. Стерео-пара делится одним гейном —
/// образ не перекашивается. Тихие сигналы идут прозрачно.
/// Держит rms коммерческих референсов без сквоша транзиентов.
/// True-peak с предпросмотром здесь нет — только мгновенный пиковый.
#[derive(Clone, Debug)]
pub struct MasterLimiter {
    env: f32,
    /// Подъем тела, разы. Умеренный: лимитер ловит только freak-стеки,
    /// характер микса (крест-фактор) цел.
    pub makeup: f32,
    /// Потолок пика до tanh-драйва мастера (редко достигается).
    pub ceiling: f32,
    /// Спад огибающей, секунды.
    pub release_secs: f32,
}

impl Default for MasterLimiter {
    fn default() -> Self {
        Self {
            env: 0.0,
            makeup: 1.5,
            ceiling: 2.5,
            release_secs: 0.05,
        }
    }
}

impl MasterLimiter {
    /// Стерео-фрейм после мейкапа и лимита (tanh-драйв — снаружи).
    pub fn process(&mut self, l: f32, r: f32, sample_rate: f32) -> (f32, f32) {
        let peak = l.abs().max(r.abs()) * self.makeup;
        let rel = (-1.0 / (sample_rate.max(1.0) * self.release_secs)).exp();
        self.env = peak.max(self.env * rel);
        if self.env < 1e-12 {
            self.env = 0.0;
        }
        let g = (self.ceiling / self.env.max(1e-6)).min(1.0);
        (l * self.makeup * g, r * self.makeup * g)
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
        let sr = 44_100.0;
        let mut lim = MasterLimiter::default();
        let (mut peak_in, mut peak_out) = (0.0f32, 0.0f32);
        for i in 0..(sr as usize) {
            let t = i as f32 / sr;
            let loud = (t * 440.0 * TAU).sin() * 3.0;
            let (l, r) = lim.process(loud, loud * 0.5, sr);
            peak_in = peak_in.max(loud.abs());
            peak_out = peak_out.max(l.abs().max(r.abs()));
        }
        assert!(peak_in > 2.9);
        assert!(
            peak_out <= 2.6,
            "limiter leaks peaks: {peak_out}"
        );
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
        assert!(
            (l - 0.05 * lim.makeup).abs() < 0.01,
            "gain stuck down: {l}"
        );
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
}
