//! Нативный аудио-поток Android: рендер + запись в AudioTrack по JNI.
//! Dart здесь не участвует вовсе (ни таймеров, ни каналов в тракте):
//! часы — блокирующая запись из нативного потока, как cpal на десктопе.
//! Маршрут/пауза рулятся снаружи: Dart шлет только команды
//! (старт/стоп/захват), звук идет мимо него.
//!
//! Живет только на Android (`#[cfg]` ниже); на остальных платформах —
//! заглушки, чтобы собирались FRB и десктоп.

use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Mutex,
};

/// Чанк рендера/записи: 0.1с при 48 кГц (онсеты точнее шага сетки).
pub(crate) const CHUNK_FRAMES: usize = 4800;
/// Частота нативного трека (AudioTrack ресемплит сам, как раньше).
pub(crate) const TRACK_SR: f32 = 48000.0;
/// Байтов буфера трека: секунда (противодавление без спина).
const TRACK_BUF_BYTES: i32 = 48000 * 2 * 4;
/// Блоков в захвате экспертизы (30 секунд по 0.1с).
const CAPTURE_BLOCKS: usize = 300;

/// Поколение потока: stop/start впритык убивает старый цикл.
static GEN: AtomicU64 = AtomicU64::new(0);
/// Поток жив (для UI и самодиагностики).
static ALIVE: AtomicBool = AtomicBool::new(false);
/// Бортовой журнал потока (кап — память не течет).
static DIAG: Mutex<Vec<String>> = Mutex::new(Vec::new());
/// Захват потока в файл: путь + осталось блоков.
static CAPTURE: Mutex<Option<(std::fs::File, usize)>> = Mutex::new(None);

/// Счетчики тракта для сводки (каждые 100 блоков ≈ 10 секунд).
struct Counters {
    chunks: u64,
    slow: u64,
    render_ms_total: u64,
}

pub(crate) fn diag_push(line: String) {
    let mut guard = DIAG.lock().unwrap_or_else(|e| e.into_inner());
    guard.push(line);
    while guard.len() > 60 {
        guard.remove(0);
    }
}

fn diag_now() -> String {
    // HH:MM:SS без chrono: достаточно секунд суток для журнала.
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
        % 86_400;
    format!("{:02}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60)
}

/// Последние строки журнала потока (для экрана диагностики).
pub(crate) fn diag_lines() -> Vec<String> {
    DIAG.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Жив ли аудио-поток прямо сейчас.
pub(crate) fn thread_alive() -> bool {
    ALIVE.load(Ordering::Relaxed)
}

/// Захват следующих 30 секунд потока в файл. Путь — снаружи
/// (публичная папка приложения). false — уже пишем или не играет.
pub(crate) fn capture_start(path: &str) -> bool {
    if path.is_empty() || !thread_alive() {
        return false;
    }
    let mut guard = CAPTURE.lock().unwrap_or_else(|e| e.into_inner());
    if guard.is_some() {
        return false;
    }
    match std::fs::File::create(path) {
        Ok(f) => {
            *guard = Some((f, CAPTURE_BLOCKS));
            diag_push(format!("{} capture -> {path}", diag_now()));
            true
        }
        Err(e) => {
            diag_push(format!("{} capture failed: {e}", diag_now()));
            false
        }
    }
}

fn capture_write(pcm: &[f32]) {
    let mut guard = CAPTURE.lock().unwrap_or_else(|e| e.into_inner());
    let Some((file, left)) = guard.as_mut() else {
        return;
    };
    use std::io::Write;
    let mut ok = true;
    for s in pcm {
        if file.write_all(&s.to_le_bytes()).is_err() {
            ok = false;
            break;
        }
    }
    *left -= 1;
    if !ok || *left == 0 {
        *guard = None;
        diag_push(format!(
            "{} capture {}",
            diag_now(),
            if ok { "done (30s)" } else { "write failed" }
        ));
    }
}

/// Остановка потока: следующее поколение не стартует, текущий цикл
/// выходит сам (максимум за один блок) и гасит трек.
pub(crate) fn thread_stop() {
    GEN.fetch_add(1, Ordering::Relaxed);
}

#[cfg(target_os = "android")]
pub(crate) fn thread_start() {
    use std::sync::atomic::Ordering;
    if thread_alive() {
        return;
    }
    let gen = GEN.fetch_add(1, Ordering::Relaxed) + 1;
    std::thread::Builder::new()
        .name("cont-audio".to_string())
        .spawn(move || {
            audio_loop(gen);
        })
        .ok();
}

#[cfg(not(target_os = "android"))]
pub(crate) fn thread_start() {
    // Десктоп играет через cpal — нативного трека тут нет.
}

#[cfg(target_os = "android")]
fn jvm() -> jni::JavaVM {
    let ctx = ndk_context::android_context();
    // SAFETY: указатель пришел из JNI_OnLoad, живет весь процесс.
    unsafe { jni::JavaVM::from_raw(ctx.vm() as *mut _) }
}

#[cfg(target_os = "android")]
fn audio_loop(gen: u64) {
    use jni::objects::JValue;
    use std::time::Instant;

    ALIVE.store(true, Ordering::Relaxed);
    diag_push(format!("{} native audio loop running", diag_now()));
    let alive = || GEN.load(Ordering::Relaxed) == gen;

    let mut scope = jni::ScopeToken::default();
    // SAFETY: guard и scope живут на стеке этого потока до конца функции.
    let attach = unsafe {
        jvm().attach_current_thread_guard(jni::AttachConfig::new(), &mut scope)
    };
    let mut guard = match attach {
        Ok(g) => g,
        Err(e) => {
            diag_push(format!("{} attach failed: {e}", diag_now()));
            ALIVE.store(false, Ordering::Relaxed);
            return;
        }
    };
    let env = guard.borrow_env_mut();

    // --- Сборка AudioTrack через Builder-цепочку (без Activity) ---
    let track: jni::objects::JObject<'_> = match (|| -> jni::errors::Result<_> {
        let attrs_builder_cls = env.find_class("android/media/AudioAttributes$Builder")?;
        let attrs_builder = env.new_object(attrs_builder_cls, "()V", &[])?;
        let attrs_builder = env.call_method(&attrs_builder, "setUsage", "(I)Landroid/media/AudioAttributes$Builder;", &[JValue::Int(1)])?.l()?;
        let attrs_builder = env.call_method(&attrs_builder, "setContentType", "(I)Landroid/media/AudioAttributes$Builder;", &[JValue::Int(2)])?.l()?;
        let attrs = env.call_method(&attrs_builder, "build", "()Landroid/media/AudioAttributes;", &[])?.l()?;

        let fmt_builder_cls = env.find_class("android/media/AudioFormat$Builder")?;
        let fmt_builder = env.new_object(fmt_builder_cls, "()V", &[])?;
        let fmt_builder = env.call_method(&fmt_builder, "setEncoding", "(I)Landroid/media/AudioFormat$Builder;", &[JValue::Int(4)])?.l()?;
        let fmt_builder = env.call_method(&fmt_builder, "setSampleRate", "(I)Landroid/media/AudioFormat$Builder;", &[JValue::Int(48000)])?.l()?;
        let fmt_builder = env.call_method(&fmt_builder, "setChannelMask", "(I)Landroid/media/AudioFormat$Builder;", &[JValue::Int(12)])?.l()?;
        let fmt = env.call_method(&fmt_builder, "build", "()Landroid/media/AudioFormat;", &[])?.l()?;

        let track_builder_cls = env.find_class("android/media/AudioTrack$Builder")?;
        let track_builder = env.new_object(track_builder_cls, "()V", &[])?;
        let track_builder = env.call_method(&track_builder, "setAudioAttributes", "(Landroid/media/AudioAttributes;)Landroid/media/AudioTrack$Builder;", &[JValue::Object(&attrs)])?.l()?;
        let track_builder = env.call_method(&track_builder, "setAudioFormat", "(Landroid/media/AudioFormat;)Landroid/media/AudioTrack$Builder;", &[JValue::Object(&fmt)])?.l()?;
        let track_builder = env.call_method(&track_builder, "setBufferSizeInBytes", "(I)Landroid/media/AudioTrack$Builder;", &[JValue::Int(TRACK_BUF_BYTES)])?.l()?;
        let track_builder = env.call_method(&track_builder, "setTransferMode", "(I)Landroid/media/AudioTrack$Builder;", &[JValue::Int(1)])?.l()?;
        let track_builder = env.call_method(&track_builder, "setPerformanceMode", "(I)Landroid/media/AudioTrack$Builder;", &[JValue::Int(10)])?.l()?;
        let track = env.call_method(&track_builder, "build", "()Landroid/media/AudioTrack;", &[])?.l()?;
        let state: i32 = env.call_method(&track, "getState", "()I", &[])?.i()?;
        if state != 1 {
            return Err(jni::errors::Error::JavaException);
        }
        Ok(track)
    })() {
        Ok(t) => t,
        Err(e) => {
            diag_push(format!("{} track build failed: {e}", diag_now()));
            ALIVE.store(false, Ordering::Relaxed);
            return;
        }
    };
    // Глобальная ссылка: переживает локальные фреймы цикла.
    // Локальную тут же удаляем (иначе копится в таблице ссылок).
    let track_global = match env.new_global_ref(&track) {
        Ok(g) => g,
        Err(e) => {
            diag_push(format!("{} global ref failed: {e}", diag_now()));
            ALIVE.store(false, Ordering::Relaxed);
            return;
        }
    };
    let _ = env.delete_local_ref(track);
    let track = track_global;

    if env
        .call_method(track.as_obj(), "play", "()V", &[])
        .is_err()
    {
        diag_push(format!("{} track play failed", diag_now()));
        ALIVE.store(false, Ordering::Relaxed);
        return;
    }

    let mut counters = Counters {
        chunks: 0,
        slow: 0,
        render_ms_total: 0,
    };
    let controller = crate::api::playback::shared_controller();
    while alive() {
        let t0 = Instant::now();
        let pcm = controller.render_pull(CHUNK_FRAMES as u64, TRACK_SR);
        let ms = t0.elapsed().as_millis() as u64;
        counters.chunks += 1;
        counters.render_ms_total += ms;
        if ms > 50 {
            counters.slow += 1;
            diag_push(format!(
                "{} SLOW render: {ms}ms (slow={})",
                diag_now(),
                counters.slow
            ));
        } else if counters.chunks % 100 == 0 {
            let avg = counters.render_ms_total / counters.chunks.max(1);
            let queued = controller.engine_stats().queued;
            diag_push(format!(
                "{} render avg {avg}ms (slow={} queued={queued})",
                diag_now(),
                counters.slow
            ));
        }
        if pcm.is_empty() {
            std::thread::sleep(std::time::Duration::from_millis(200));
            continue;
        }
        capture_write(&pcm);
        // Запись одним куском в локальном фрейме: массив удаляется
        // сразу, таблица ссылок не растет весь трек.
        let write_res: jni::errors::Result<i32> = env.with_local_frame(8, |env| {
            let arr = env.new_float_array(pcm.len())?;
            env.set_float_array_region(&arr, 0, &pcm)?;
            let n = env.call_method(
                track.as_obj(),
                "write",
                "([FIII)I",
                &[
                    JValue::Object(&arr),
                    JValue::Int(0),
                    JValue::Int(pcm.len() as i32),
                    JValue::Int(1),
                ],
            )?.i()?;
            Ok(n)
        });
        match write_res {
            Ok(n) if n >= 0 => {}
            _ => {
                // Трек умер (маршрут без события): выходим, Dart
                // пересоздаст цикл по команде play (как раньше heal).
                diag_push(format!("{} track write failed, exiting", diag_now()));
                break;
            }
        }
    }

    // Уборка: пауза, сброс буфера, release, detach через guard.
    let env = guard.borrow_env_mut();
    let _ = env.call_method(track.as_obj(), "pause", "()V", &[]);
    let _ = env.call_method(track.as_obj(), "flush", "()V", &[]);
    let _ = env.call_method(track.as_obj(), "release", "()V", &[]);
    ALIVE.store(false, Ordering::Relaxed);
    diag_push(format!("{} native audio loop stopped", diag_now()));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diag_ring_caps_memory() {
        for i in 0..100 {
            diag_push(format!("line {i}"));
        }
        let lines = diag_lines();
        assert_eq!(lines.len(), 60);
        assert!(lines.last().unwrap().ends_with("line 99"));
    }

    #[test]
    fn capture_rejects_empty_path_or_dead_thread() {
        // Без живого потока захват не стартует (не во что писать).
        assert!(!capture_start(""));
        // Поток в тестах не запущен.
        assert!(!thread_alive());
    }
}
