//! Одна нота на инструмент: пик, крест-фактор, искажения (THD).
//! Отвечает на вопрос «хрипит ли инструмент» числами, а не слухом.
//! Временный инструмент.

use rust_lib_cont::instruments::{NoteEvent, Slot};
use rust_lib_cont::playback::build_instruments;

fn main() {
    let sr = 44_100.0f32;
    let notes: Vec<(usize, u8, f32)> = Slot::ALL.iter().map(|s| (s.id(), 55u8, 0.9f32)).collect();
    let mut instruments = build_instruments();
    for (id, pitch, vel) in notes {
        let Some(pos) = instruments.iter().position(|i| i.id() == id) else {
            continue;
        };
        let name = Slot::ALL
            .iter()
            .find(|s| s.id() == id)
            .map(|s| s.name())
            .unwrap_or("?");
        let mut inst = instruments.remove(pos);
        inst.set_sample_rate(sr);
        inst.trigger(&NoteEvent::new(id, pitch, vel, 0.4));
        let n = (sr * 0.4) as usize;
        let mut buf = Vec::with_capacity(n);
        for i in 0..n {
            // Перекрытие: вторая нота через 100 мс (хвост первой + новая).
            if i == (sr * 0.1) as usize {
                inst.trigger(&NoteEvent::new(id, pitch, vel, 0.4));
            }
            buf.push(inst.next_sample());
        }
        let peak = buf.iter().fold(0.0f32, |a, v| a.max(v.abs()));
        // RMS по активной части (выше -40 дБ от пика): тишина в хвосте
        // не должна съедать средний уровень.
        let thr = peak * 0.01;
        let active: Vec<f32> = buf.iter().copied().filter(|v| v.abs() > thr).collect();
        let rms = if active.is_empty() {
            0.0
        } else {
            (active.iter().map(|v| v * v).sum::<f32>() / active.len() as f32).sqrt()
        };
        let clipped = buf.iter().filter(|v| v.abs() > 0.999).count();
        let (thd_db, f0_hz) = thd(&buf, sr);
        println!(
            "{:<10} пик={:6.3} rms={:6.3} крест={:5.1}дБ клип={:4} f0≈{:6.0}Гц THD={:5.1}дБ",
            name,
            peak,
            rms,
            20.0 * (peak / rms.max(1e-6)).log10(),
            clipped,
            f0_hz,
            thd_db
        );
    }
}

/// Энергия гармоник против основной: спектр одной ноты.
fn thd(buf: &[f32], sr: f32) -> (f32, f32) {
    let size = 16384usize;
    let n = buf.len().min(size);
    let fft_n = n.next_power_of_two();
    let mut re = vec![0.0f64; fft_n];
    let mut im = vec![0.0f64; fft_n];
    for i in 0..n {
        let w = 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / n as f64).cos();
        re[i] = buf[i] as f64 * w;
    }
    fft(&mut re, &mut im);
    let bin_hz = sr as f64 / fft_n as f64;
    let mut best = 0usize;
    let mut best_e = 0.0f64;
    let lo = (30.0 / bin_hz) as usize;
    let hi = ((sr as f64 * 0.25) / bin_hz) as usize;
    for k in lo..hi.min(fft_n / 2) {
        let e = re[k] * re[k] + im[k] * im[k];
        if e > best_e {
            best_e = e;
            best = k;
        }
    }
    let f0 = best as f64 * bin_hz;
    let mut fund = 0.0f64;
    let mut harm = 0.0f64;
    for h in 2..=12 {
        let k = (f0 * h as f64 / bin_hz) as usize;
        if k == 0 || k >= fft_n / 2 {
            continue;
        }
        let e = re[k] * re[k] + im[k] * im[k];
        if h == 1 {
            fund += e;
        } else {
            harm += e;
        }
    }
    let ratio = (harm / fund.max(1e-12)).sqrt();
    (20.0 * ratio.max(1e-9).log10() as f32, f0 as f32)
}

fn fft(re: &mut [f64], im: &mut [f64]) {
    let n = re.len();
    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2usize;
    while len <= n {
        let ang = -2.0 * std::f64::consts::PI / len as f64;
        let (w_re, w_im) = (ang.cos(), ang.sin());
        let mut i = 0usize;
        while i < n {
            let (mut cur_re, mut cur_im) = (1.0f64, 0.0f64);
            for k in 0..len / 2 {
                let (u_re, u_im) = (re[i + k], im[i + k]);
                let (v_re, v_im) = (
                    cur_re * re[i + k + len / 2] - cur_im * im[i + k + len / 2],
                    cur_re * im[i + k + len / 2] + cur_im * re[i + k + len / 2],
                );
                re[i + k] = u_re + v_re;
                im[i + k] = u_im + v_im;
                re[i + k + len / 2] = u_re - v_re;
                im[i + k + len / 2] = u_im - v_im;
                let next_re = cur_re * w_re - cur_im * w_im;
                cur_im = cur_re * w_im + cur_im * w_re;
                cur_re = next_re;
            }
            i += len;
        }
        len <<= 1;
    }
}
