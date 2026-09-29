//! Мастер-пространство: компактный стерео-реверб (Freeverb-лайт)
//! плюс темповый дилэй для вокала.
//!
//! Реверб к темпу не привязан (глубина без ритмического риска),
//! дилэй — dotted-8th от живого BPM (эхо вокала в сетке).
//! Кик в send не входит (highpass): низ остается сухим и читаемым.
//! Все структуры конкретные, без generic — безопасно для FFI-парсера.

const COMB_TUNINGS_L: [f32; 4] = [1116.0, 1188.0, 1277.0, 1356.0];
const COMB_TUNINGS_R: [f32; 4] = [1139.0, 1211.0, 1300.0, 1379.0];
const AP_TUNINGS_L: [f32; 2] = [556.0, 441.0];
const AP_TUNINGS_R: [f32; 2] = [579.0, 464.0];
const AP_FEEDBACK: f32 = 0.5;
/// Send highpass: все ниже 400 Гц в реверб не идет.
const SEND_HP_HZ: f32 = 400.0;
/// Пол анти-денормала: линии реверба живут вечно и без этого сползают
/// в субнормальный диапазон, где арифметика x86 в десятки раз медленнее
/// (редкие всплески нагрузки = хрип/прерывания). 1e-18 — обычный float,
/// на −360 дБ неслышимый, но держит всю обратную связь в норме.
const FEED_GUARD: f32 = 1e-18;

struct Comb {
    buf: Vec<f32>,
    idx: usize,
    store: f32,
}

impl Comb {
    fn with_len(len: usize) -> Self {
        Self {
            buf: vec![0.0; len.max(2)],
            idx: 0,
            store: 0.0,
        }
    }

    fn process(&mut self, x: f32, feedback: f32, damp: f32) -> f32 {
        let out = self.buf[self.idx];
        self.store = out * (1.0 - damp) + self.store * damp;
        self.buf[self.idx] = x + self.store * feedback;
        self.idx = (self.idx + 1) % self.buf.len();
        out
    }
}

struct Allpass {
    buf: Vec<f32>,
    idx: usize,
}

impl Allpass {
    fn with_len(len: usize) -> Self {
        Self {
            buf: vec![0.0; len.max(2)],
            idx: 0,
        }
    }

    fn process(&mut self, x: f32) -> f32 {
        let delayed = self.buf[self.idx];
        let out = -x + delayed;
        self.buf[self.idx] = x + delayed * AP_FEEDBACK;
        self.idx = (self.idx + 1) % self.buf.len();
        out
    }
}

pub struct SpaceBus {
    combs_l: [Comb; 4],
    combs_r: [Comb; 4],
    ap_l: [Allpass; 2],
    ap_r: [Allpass; 2],
    hp_a: f32,
    hp_y_l: f32,
    hp_y_r: f32,
    hp_x_l: f32,
    hp_x_r: f32,
    wet: f32,
    feedback: f32,
    damp: f32,
}

impl SpaceBus {
    pub fn new(sample_rate: f32) -> Self {
        let scale = sample_rate / 44_100.0;
        let comb_len = |t: f32| (t * scale).round() as usize;
        let ap_len = |t: f32| (t * scale).round() as usize;
        let rc = 1.0 / (std::f32::consts::TAU * SEND_HP_HZ);
        let dt = 1.0 / sample_rate;
        Self {
            combs_l: std::array::from_fn(|i| Comb::with_len(comb_len(COMB_TUNINGS_L[i]))),
            combs_r: std::array::from_fn(|i| Comb::with_len(comb_len(COMB_TUNINGS_R[i]))),
            ap_l: std::array::from_fn(|i| Allpass::with_len(ap_len(AP_TUNINGS_L[i]))),
            ap_r: std::array::from_fn(|i| Allpass::with_len(ap_len(AP_TUNINGS_R[i]))),
            hp_a: rc / (rc + dt),
            hp_y_l: 0.0,
            hp_y_r: 0.0,
            hp_x_l: 0.0,
            hp_x_r: 0.0,
            wet: 0.16,
            feedback: 0.82,
            damp: 0.45,
        }
    }

    /// Размер комнаты от стиля: wet 0..0.8, size 0..1 (комната → собор).
    /// Большая комната дольше и темнее.
    pub fn set_room(&mut self, wet: f32, size: f32) {
        let size = size.clamp(0.0, 1.0);
        self.wet = wet.clamp(0.0, 0.8);
        self.feedback = 0.70 + 0.22 * size;
        self.damp = 0.35 + 0.25 * size;
    }

    fn send_hp(&mut self, x: f32, left: bool) -> f32 {
        let (y_prev, x_prev) = if left {
            (self.hp_y_l, self.hp_x_l)
        } else {
            (self.hp_y_r, self.hp_x_r)
        };
        let mut y = self.hp_a * (y_prev + x - x_prev);
        if y.abs() < 1e-15 {
            y = 0.0;
        }
        if left {
            self.hp_y_l = y;
            self.hp_x_l = x;
        } else {
            self.hp_y_r = y;
            self.hp_x_r = x;
        }
        y
    }

    /// Полный сброс линий (защита от залипшего NaN — реверб с обратной
    /// связью иначе звенел бы им вечно).
    fn reset_all(&mut self) {
        for comb in self.combs_l.iter_mut().chain(self.combs_r.iter_mut()) {
            comb.buf.fill(0.0);
            comb.store = 0.0;
        }
        for ap in self.ap_l.iter_mut().chain(self.ap_r.iter_mut()) {
            ap.buf.fill(0.0);
        }
        self.hp_y_l = 0.0;
        self.hp_y_r = 0.0;
        self.hp_x_l = 0.0;
        self.hp_x_r = 0.0;
    }

    /// Стерео вход → сухой + темный мокрый хвост.
    pub fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        let (fb, damp, wet) = (self.feedback, self.damp, self.wet);
        let mut wet_l = 0.0;
        let send_l = self.send_hp(l, true) + FEED_GUARD;
        for comb in &mut self.combs_l {
            wet_l += comb.process(send_l, fb, damp);
        }
        wet_l *= 0.25;
        for ap in &mut self.ap_l {
            wet_l = ap.process(wet_l);
        }

        let mut wet_r = 0.0;
        let send_r = self.send_hp(r, false) + FEED_GUARD;
        for comb in &mut self.combs_r {
            wet_r += comb.process(send_r, fb, damp);
        }
        wet_r *= 0.25;
        for ap in &mut self.ap_r {
            wet_r = ap.process(wet_r);
        }

        let (mut out_l, mut out_r) = (l + wet_l * wet, r + wet_r * wet);
        if !(out_l.is_finite() && out_r.is_finite()) {
            self.reset_all();
            out_l = 0.0;
            out_r = 0.0;
        }
        (out_l, out_r)
    }
}

/// Темповый feedback-дилэй (dotted-8th): эхо вокала и лида в сетке.
/// Время выставляет движок из живого BPM; линия одна на канал.
pub struct Echo {
    buf_l: Vec<f32>,
    buf_r: Vec<f32>,
    write_idx: usize,
    read_offset: usize,
    feedback: f32,
    lp_a: f32,
    lp_y_l: f32,
    lp_y_r: f32,
}

impl Echo {
    /// `max_secs` — запас линии (хватит на самый медленный темп).
    pub fn new(sample_rate: f32, max_secs: f32, feedback: f32, damp_hz: f32) -> Self {
        let len = ((max_secs * sample_rate) as usize).max(64);
        let rc = 1.0 / (std::f32::consts::TAU * damp_hz);
        let dt = 1.0 / sample_rate;
        let mut echo = Self {
            buf_l: vec![0.0; len],
            buf_r: vec![0.0; len],
            write_idx: 0,
            read_offset: len / 2,
            feedback: feedback.clamp(0.0, 0.9),
            lp_a: rc / (rc + dt),
            lp_y_l: 0.0,
            lp_y_r: 0.0,
        };
        echo.set_time(0.32, sample_rate);
        echo
    }

    pub fn set_time(&mut self, secs: f32, sample_rate: f32) {
        let len = self.buf_l.len();
        self.read_offset = ((secs * sample_rate) as usize).clamp(16, len - 1);
    }

    /// Send stereo → мокрые повторы. Сухого в выходе нет.
    pub fn process(&mut self, send_l: f32, send_r: f32) -> (f32, f32) {
        let len = self.buf_l.len();
        let read = (self.write_idx + len - self.read_offset) % len;
        let mut wet_l = self.buf_l[read];
        let mut wet_r = self.buf_r[read];
        if wet_l.abs() < 1e-15 {
            wet_l = 0.0;
        }
        if wet_r.abs() < 1e-15 {
            wet_r = 0.0;
        }
        // Демпфирование в петле: повторы темнеют, как в аналоге.
        self.lp_y_l += self.lp_a * (wet_l - self.lp_y_l);
        self.lp_y_r += self.lp_a * (wet_r - self.lp_y_r);
        self.buf_l[self.write_idx] = send_l + self.lp_y_l * self.feedback;
        self.buf_r[self.write_idx] = send_r + self.lp_y_r * self.feedback;
        self.write_idx = (self.write_idx + 1) % len;
        if !(wet_l.is_finite() && wet_r.is_finite()) {
            self.buf_l.fill(0.0);
            self.buf_r.fill(0.0);
            return (0.0, 0.0);
        }
        (wet_l, wet_r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_in_silence_out() {
        let mut space = SpaceBus::new(44_100.0);
        for _ in 0..1000 {
            let (l, r) = space.process(0.0, 0.0);
            // Только неслышимый пол анти-денормала, не сигнал.
            assert!(l.abs() < 1e-9 && r.abs() < 1e-9);
        }
    }

    #[test]
    fn nan_input_recovers_to_silence() {
        let mut space = SpaceBus::new(44_100.0);
        let (l, r) = space.process(f32::NAN, 0.0);
        assert!(l.is_finite() && r.is_finite());
        // После сброса шина снова работает.
        let (l, r) = space.process(0.5, 0.5);
        assert!(l.is_finite() && r.is_finite());
    }

    #[test]
    fn impulse_rings_and_decays() {
        let sr = 44_100.0;
        let mut space = SpaceBus::new(sr);
        let (l0, r0) = space.process(1.0, 1.0);
        assert!(l0.is_finite() && r0.is_finite());
        // Энергия первой половины хвоста больше второй — затухание.
        let mut first = 0.0;
        let mut second = 0.0;
        for i in 0..(sr as usize) {
            let (l, r) = space.process(0.0, 0.0);
            assert!(l.is_finite() && r.is_finite());
            // Мокрый хвост не взрывается.
            assert!(l.abs() < 2.0 && r.abs() < 2.0);
            let e = l * l + r * r;
            if i < (0.5 * sr) as usize {
                first += e;
            } else {
                second += e;
            }
        }
        assert!(
            first > second * 4.0,
            "reverb does not decay: {first} vs {second}"
        );
    }

    #[test]
    fn room_size_changes_tail() {
        // Собор звенит дольше комнаты: сравниваем позднюю энергию хвоста
        // (ранние отражения у всех комнат похожи).
        let sr = 44_100.0;
        let late_energy = |size: f32| {
            let mut space = SpaceBus::new(sr);
            space.set_room(0.5, size);
            space.process(1.0, 1.0);
            let mut energy = 0.0;
            for i in 0..(sr as usize) {
                let (l, r) = space.process(0.0, 0.0);
                assert!(l.is_finite() && r.is_finite());
                if i >= (0.5 * sr) as usize {
                    energy += l * l + r * r;
                }
            }
            energy
        };
        let room = late_energy(0.2);
        let cathedral = late_energy(0.9);
        assert!(
            cathedral > room * 5.0,
            "room size does nothing: {cathedral} vs {room}"
        );
    }

    #[test]
    fn echo_repeats_then_dies() {
        let sr = 44_100.0;
        let mut echo = Echo::new(sr, 1.0, 0.38, 2800.0);
        echo.set_time(0.2, sr);
        // Один импульс в send.
        let (w0, _) = echo.process(1.0, 1.0);
        assert!(w0.is_finite());
        // Первый повтор приходит примерно через время задержки.
        let mut heard: f32 = 0.0;
        for i in 0..((0.5 * sr) as usize) {
            let (l, r) = echo.process(0.0, 0.0);
            assert!(l.is_finite() && r.is_finite());
            if i > ((0.19 * sr) as usize) && i < ((0.25 * sr) as usize) {
                heard = heard.max(l.abs().max(r.abs()));
            }
        }
        assert!(heard > 0.05, "no echo repeat: {heard}");
        // Хвост затухает: через 3 секунды почти тишина.
        for _ in 0..((3.0 * sr) as usize) {
            echo.process(0.0, 0.0);
        }
        let (l, r) = echo.process(0.0, 0.0);
        assert!(l.abs() < 1e-3 && r.abs() < 1e-3);
    }
}
