//! Слушательный набор: соло-рендер каждого слота стиля в WAV плюс
//! объективные метрики (пик, RMS, атака, декей, тональный баланс).
//! Временный инструмент (удаляется после проверки).

use rust_lib_cont::instruments::Slot;
use rust_lib_cont::playback::render_wav;

fn read_wav_pcm16(path: &str) -> (u32, Vec<i16>) {
    let bytes = std::fs::read(path).expect("wav readable");
    // RIFF-заголовок: ищем "data" и читаем PCM16.
    let mut pos = 12usize;
    let mut sample_rate = 44100u32;
    let mut samples = Vec::new();
    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let size = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().unwrap()) as usize;
        if id == b"fmt " {
            sample_rate = u32::from_le_bytes(bytes[pos + 12..pos + 16].try_into().unwrap());
        }
        if id == b"data" {
            let end = (pos + 8 + size).min(bytes.len());
            let mut i = pos + 8;
            while i + 1 < end {
                samples.push(i16::from_le_bytes(bytes[i..i + 2].try_into().unwrap()));
                i += 2;
            }
            break;
        }
        pos += 8 + size + (size & 1);
    }
    (sample_rate, samples)
}

/// Метрики одного файла: громкость, динамика, атака, декей, спектр.
fn analyze(path: &str) {
    let (sr, s16) = read_wav_pcm16(path);
    let n = s16.len();
    if n < 1024 {
        println!("{path}: too short");
        return;
    }
    let peak = s16.iter().map(|v| v.abs() as f32).fold(0.0f32, f32::max);
    let sum_sq: f64 = s16.iter().map(|v| (*v as f64) * (*v as f64)).sum();
    let rms = (sum_sq / n as f64).sqrt() as f32;
    // Спектр: энергия по полосам по всему файлу (окна 8192 с перекрытием).
    let size = 8192usize;
    let fft_n = size.next_power_of_two();
    let bands = [
        (0.0, 120.0),
        (120.0, 500.0),
        (500.0, 2000.0),
        (2000.0, 8000.0),
        (8000.0, 22050.0),
    ];
    let mut energies = [0.0f64; 5];
    let hop = size / 2;
    let mut start = 0usize;
    while start + size <= n {
        let mut re = vec![0.0f64; fft_n];
        let mut im = vec![0.0f64; fft_n];
        for i in 0..size {
            let w = 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / size as f64).cos();
            re[i] = s16[start + i] as f64 * w;
        }
        fft(&mut re, &mut im);
        let bin_hz = sr as f64 / fft_n as f64;
        for k in 1..fft_n / 2 {
            let hz = k as f64 * bin_hz;
            let e = re[k] * re[k] + im[k] * im[k];
            for (bi, (lo, hi)) in bands.iter().enumerate() {
                if hz >= *lo && hz < *hi {
                    energies[bi] += e;
                }
            }
        }
        start += hop;
    }
    // ВЧ-пик: однополюсный фильтр 2 кГц — транзиент виден отдельно от
    // энергии (клик длится миллисекунды, в спектре энергии его нет).
    let mut hp_state = 0.0f32;
    let mut hp_prev = 0.0f32;
    let a = (-2.0 * std::f32::consts::PI * 2000.0 / sr as f32).exp();
    let mut hp_peak = 0.0f32;
    for v in &s16 {
        let x = *v as f32 / 32768.0;
        let y = a * (hp_state + x - hp_prev);
        hp_state = y;
        hp_prev = x;
        hp_peak = hp_peak.max(y.abs());
    }
    // Ширина микса: разница каналов против суммы. 0 = моно (плоско),
    // ближе к 1 = каналы почти разделены.
    let mut side = 0.0f64;
    let mut mid = 0.0f64;
    for c in s16.chunks(2) {
        let l = c[0] as f32 / 32768.0;
        let r = c[1] as f32 / 32768.0;
        side += ((l - r) * 0.5).powi(2) as f64;
        mid += ((l + r) * 0.5).powi(2) as f64;
    }
    let width = (side / mid.max(1e-12)).sqrt();
    // RMS по активной части (где есть сигнал): тишина не должна съедать
    // средний уровень инструмента.
    let total: f64 = energies.iter().sum::<f64>().max(1e-9);
    let pct: Vec<String> = bands
        .iter()
        .zip(energies.iter())
        .map(|((lo, _), e)| format!("{lo:.0}Hz:{:.0}%", 100.0 * e / total))
        .collect();
    println!(
        "{:<28} peak={:5.1}dB rms={:5.1}dB crest={:4.1}dB hp2k={:5.1}dB width={:4.2} | {}",
        std::path::Path::new(path)
            .file_stem()
            .unwrap()
            .to_string_lossy(),
        20.0 * (peak / 32768.0).max(1e-6).log10(),
        20.0 * (rms / 32768.0).max(1e-6).log10(),
        20.0 * (peak / rms.max(1e-6)).log10(),
        20.0 * hp_peak.max(1e-6).log10(),
        width,
        pct.join(" ")
    );
}

/// FFT на месте (итеративный radix-2, без зависимостей).
/// Разрывы сигнала: отклонение от линейного предсказания. Большой
/// разрыв = щелчок (граница ноты или обрыв хвоста).
fn jumps(path: &str) {
    let (sr, s16) = read_wav_pcm16(path);
    // WAV стерео с чередованием L/R: для разрывов берём среднее.
    let mono: Vec<f32> = s16
        .chunks(2)
        .map(|c| (c[0] as f32 + c[1] as f32) * 0.5 / 32768.0)
        .collect();
    // Ширина микса: разница каналов против суммы. 0 = моно (плоско),
    // ближе к 1 = каналы почти разделены.
    let mut side = 0.0f64;
    let mut mid = 0.0f64;
    for c in s16.chunks(2) {
        let l = c[0] as f32 / 32768.0;
        let r = c[1] as f32 / 32768.0;
        side += ((l - r) * 0.5).powi(2) as f64;
        mid += ((l + r) * 0.5).powi(2) as f64;
    }
    let width = (side / mid.max(1e-12)).sqrt();
    let n = mono.len();
    if n < 16 {
        return;
    }
    let mut worst = (0.0f32, 0usize);
    let mut over = 0usize;
    for i in 2..n {
        let pred = 2.0 * mono[i - 1] - mono[i - 2];
        let d = (mono[i] - pred).abs();
        if d > worst.0 {
            worst = (d, i);
        }
        if d > 0.01 {
            over += 1;
        }
    }
    println!(
        "    ширина={:.3} (0 = моно) | разрыв: макс={:.4} @ {:.3}с, с разрывом>0.01: {} ({} ppm)",
        width,
        worst.0,
        worst.1 as f32 / sr as f32,
        over,
        over * 1_000_000 / n
    );
}

/// Спектральный поток по 16-м: насколько спектр меняется от доли к
/// доле. Стык фразы (каждые 32 доли) сравниваем с обычными долями.
fn flux(path: &str, bpm: f32) {
    let (sr, s16) = read_wav_pcm16(path);
    let mono: Vec<f32> = s16
        .chunks(2)
        .map(|c| (c[0] as f32 + c[1] as f32) * 0.5 / 32768.0)
        .collect();
    let step = (sr as f32 * 60.0 / bpm / 4.0) as usize; // 16-я
    if step < 64 || mono.len() < step * 4 {
        println!("мало данных");
        return;
    }
    // Магнитуда спектра по полосам (8 полос) для каждой 16-й.
    let bands = 8usize;
    let mut prev = vec![0f32; bands];
    let mut fluxes: Vec<f32> = Vec::new();
    let mut idx = 0usize;
    while idx + step <= mono.len() {
        let frame = &mono[idx..idx + step];
        let mut cur = vec![0f32; bands];
        for (i, v) in frame.iter().enumerate() {
            let hz = i as f32 * sr as f32 / step as f32;
            let b = ((hz / (sr as f32 / 2.0)) * bands as f32) as usize;
            if b < bands {
                cur[b] += v.abs();
            }
        }
        let mut fl = 0f32;
        for b in 0..bands {
            let d = cur[b] - prev[b];
            if d > 0.0 {
                fl += d;
            }
        }
        fluxes.push(fl);
        prev = cur;
        idx += step;
    }
    // Стыки фраз: каждые 32 доли (2 такта) — доли 0 внутри фразы.
    // Сравниваем со ВТОРИМИ даунбитами (каждые 16 долей): иначе стык
    // всегда сравнивается с пустыми долями и метрика врёт.
    let phrase = 32usize;
    let mut at_bound = Vec::new();
    let mut inside = Vec::new();
    for (i, f) in fluxes.iter().enumerate() {
        if i < 2 {
            continue;
        }
        if i % phrase == 0 {
            at_bound.push(*f);
        } else if i % 16 == 0 {
            inside.push(*f);
        }
    }
    let mean = |v: &Vec<f32>| -> f32 {
        if v.is_empty() {
            return 0.0;
        }
        v.iter().sum::<f32>() / v.len() as f32
    };
    let sorted = |v: &mut Vec<f32>| -> Vec<f32> {
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        v.clone()
    };
    let b = sorted(&mut at_bound.clone());
    let ins = sorted(&mut inside.clone());
    let p95 = |v: &[f32], p: f32| -> f32 {
        if v.is_empty() {
            return 0.0;
        }
        let i = ((v.len() - 1) as f32 * p) as usize;
        v[i]
    };
    println!(
        "поток по 16-м: средний={:.4} | на стыке фраз: средний={:.4} p95={:.4} | внутри фразы: средний={:.4} p95={:.4}",
        mean(&fluxes),
        mean(&b),
        p95(&b, 0.95),
        mean(&ins),
        p95(&ins, 0.95)
    );
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

fn main() {
    let style = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "club".to_string());
    let dir = std::env::args()
        .nth(2)
        .unwrap_or_else(|| "build/sounds".to_string());
    let _ = std::fs::create_dir_all(&dir);
    // Режим разрывов: разбор уже отрендеренных соло-файлов.
    if std::env::args().nth(3).as_deref() == Some("jumps") {
        let entries: Vec<_> = match std::fs::read_dir(&dir) {
            Ok(rd) => rd.flatten().collect(),
            Err(_) => Vec::new(),
        };
        for entry in entries {
            let path = entry.path();
            let name = path.to_string_lossy().to_string();
            if !name.ends_with(".wav") || name.contains("_mix") {
                continue;
            }
            print!("{:<24}", path.file_stem().unwrap().to_string_lossy());
            jumps(&path.to_string_lossy());
        }
        return;
    }
    // Режим потока по соло: кто именно скачет на стыке фразы.
    if std::env::args().nth(3).as_deref() == Some("fluxall") {
        let bpm = rust_lib_cont::styles::make_style(&style)
            .map(|s| s.bpm())
            .unwrap_or(126.0);
        let slots = [
            ("bass", Slot::Bass),
            ("kick", Slot::Kick),
            ("snare", Slot::Snare),
            ("hat", Slot::Hat),
            ("shaker", Slot::Shaker),
            ("pluck", Slot::Pluck),
            ("keys", Slot::Keys),
            ("pad", Slot::Pad),
            ("kick808", Slot::Kick808),
            ("vocal", Slot::Vocal),
            ("guitar", Slot::Guitar),
        ];
        for (name, slot) in slots {
            let path = format!("{dir}/{style}_flux_{name}.wav");
            match render_wav(&style, 424242, 40.0, 44_100.0, &path, &[], &[slot.id()]) {
                Ok(_) => {
                    print!("{:<10}", name);
                    flux(&path, bpm);
                }
                Err(e) => println!("{name}: {e}"),
            }
        }
        return;
    }
    // Режим потока: спектральный поток по 16-м. Показывает, где музыка
    // «прыгает»: на стыке фраз поток должен быть в рамках обычного.
    if std::env::args().nth(3).as_deref() == Some("flux") {
        let path = format!("{dir}/{style}_mix.wav");
        let bpm = rust_lib_cont::styles::make_style(&style)
            .map(|s| s.bpm())
            .unwrap_or(126.0);
        match render_wav(&style, 424242, 40.0, 44_100.0, &path, &[], &[]) {
            Ok(_) => flux(&path, bpm),
            Err(e) => println!("render failed: {e}"),
        }
        return;
    }
    // Режим микса: полная смесь стиля — спектр и разрывы всего трека.
    if std::env::args().nth(3).as_deref() == Some("mix") {
        let path = format!("{dir}/{style}_mix.wav");
        match render_wav(&style, 424242, 20.0, 44_100.0, &path, &[], &[]) {
            Ok(_) => {
                analyze(&path);
                jumps(&path);
            }
            Err(e) => println!("render failed: {e}"),
        }
        return;
    }
    let bpm = rust_lib_cont::styles::make_style(&style)
        .map(|s| s.bpm())
        .unwrap_or(126.0);
    let secs = 8.0f32;
    let bars = secs * bpm / 60.0 * 4.0;
    let slots = [
        ("bass", Slot::Bass),
        ("kick", Slot::Kick),
        ("snare", Slot::Snare),
        ("hat", Slot::Hat),
        ("shaker", Slot::Shaker),
        ("pluck", Slot::Pluck),
        ("keys", Slot::Keys),
        ("pad", Slot::Pad),
        ("kick808", Slot::Kick808),
        ("vocal", Slot::Vocal),
        ("guitar", Slot::Guitar),
        ("vinyl", Slot::Vinyl),
    ];
    println!("--- соло-рендер {style} ---");
    for (name, slot) in slots {
        let path = format!("{dir}/{style}_{name}.wav");
        match render_wav(&style, 424242, secs, 44_100.0, &path, &[], &[slot.id()]) {
            Ok(report) => {
                analyze(&path);
                println!(
                    "    нот: {} (≈ {:.2} на такт)",
                    report.notes,
                    report.notes as f32 / bars
                );
            }
            Err(e) => println!("{name}: render failed: {e}"),
        }
    }
}
