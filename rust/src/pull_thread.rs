//! Native Android audio stream: render + write to AudioTrack via JNI.
//! Dart takes no part here at all (no timers or channels in the pipeline):
//! the clock is the blocking write from the native stream, like cpal on desktop.
//! Route/pause are controlled externally: Dart only sends commands
//! (start/stop/capture), the sound goes around it.
//!
//! Lives only on Android (`#[cfg]` below); on other platforms there are
//! stubs so FRB and the desktop target still build.

use std::sync::{
    atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering},
    Mutex,
};

/// Render/write chunk: 0.05 s at 48 kHz. A shorter quantum means less
/// audible gap from one slow chunk (50 ms vs 100 ms)
/// and more accurate onsets. Any shorter (25 ms) — twice as many JNI calls
/// with no audible benefit.
pub(crate) const CHUNK_FRAMES: usize = 2400;
/// Native track sample rate (AudioTrack resamples itself, as before).
pub(crate) const TRACK_SR: f32 = 48000.0;
/// Track buffer bytes: one second (backpressure without spinning).
const TRACK_BUF_BYTES: i32 = 48000 * 2 * 4;
/// Blocks in a diagnostic capture (30 seconds at 0.05 s each).
const CAPTURE_BLOCKS: usize = 600;

/// Stream generation: back-to-back stop/start kills the old loop.
static GEN: AtomicU64 = AtomicU64::new(0);
/// Stream is alive (for UI and self-diagnostics).
static ALIVE: AtomicBool = AtomicBool::new(false);
/// Onboard stream log (capped — memory doesn't leak).
static DIAG: Mutex<Vec<String>> = Mutex::new(Vec::new());
/// Stream capture to a file: path + blocks remaining.
static CAPTURE: Mutex<Option<(std::fs::File, usize)>> = Mutex::new(None);
/// AudioTrack buffer queue (given minus consumed), ms: an estimate of how
/// far the sounding audio lags behind the rendered audio. Written by the audio loop
/// every 5 seconds, read by the UI to sync the visualizer
/// (the wheel must show what sounds, not what was
/// generated). Zero — stream isn't playing / unknown.
static LATENCY_MS: AtomicI64 = AtomicI64::new(0);

/// Track buffer queue for the frontend, ms. Android only: there the head
/// position is read by the native loop. Other platforms — 0 (the cpal callback
/// fires almost at the moment of sounding, the remainder is corrected by the frontend margin).
#[cfg(target_os = "android")]
pub(crate) fn output_latency_ms() -> f32 {
    LATENCY_MS.load(Ordering::Relaxed).max(0) as f32
}

#[cfg(not(target_os = "android"))]
pub(crate) fn output_latency_ms() -> f32 {
    0.0
}

/// Pipeline counters for the summary (every 100 blocks ≈ 5 seconds).
struct Counters {
    chunks: u64,
    slow: u64,
    render_ms_total: u64,
    pending_max: usize,
    /// Total frames given to the track (for clock drift).
    written: u64,
    /// Last head position + base of the 32-bit counter wrap.
    last_head: i64,
    head_base: i64,
}

pub(crate) fn diag_push(line: String) {
    let mut guard = DIAG.lock().unwrap_or_else(|e| e.into_inner());
    guard.push(line);
    while guard.len() > 60 {
        guard.remove(0);
    }
}

fn diag_now() -> String {
    // HH:MM:SS without chrono: seconds of the day are enough for the log.
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
        % 86_400;
    format!("{:02}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60)
}

/// Last lines of the stream log (for the diagnostics screen).
pub(crate) fn diag_lines() -> Vec<String> {
    DIAG.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Is the audio stream alive right now.
pub(crate) fn thread_alive() -> bool {
    ALIVE.load(Ordering::Relaxed)
}

/// Capture the next 30 seconds of the stream to a file. The path is outside
/// (the app's public folder). false — already writing or not playing.
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

/// Stopping the stream: the next generation won't start, the current loop
/// exits on its own (within one block at most) and releases the track.
pub(crate) fn thread_stop() {
    GEN.fetch_add(1, Ordering::Relaxed);
}

#[cfg(target_os = "android")]
pub(crate) fn thread_start() {
    use std::sync::atomic::Ordering;
    if thread_alive() {
        return;
    }
    // Trace of the first step: whether startup is visible at all (spawn may have silently
    // failed — then silence without a single log line).
    diag_push(format!("{} thread_start requested", diag_now()));
    // A previous capture must not strangle a new one: a file cut short by a stop is
    // closed and forgotten (otherwise the button is dead forever).
    {
        let mut guard = CAPTURE.lock().unwrap_or_else(|e| e.into_inner());
        *guard = None;
    }
    let gen = GEN.fetch_add(1, Ordering::Relaxed) + 1;
    if std::thread::Builder::new()
        .name("cont-audio".to_string())
        .spawn(move || {
            audio_loop(gen);
        })
        .is_err()
    {
        diag_push(format!("{} thread spawn failed", diag_now()));
    }
}

#[cfg(not(target_os = "android"))]
pub(crate) fn thread_start() {
    // Desktop plays via cpal — there is no native track here.
}

#[cfg(target_os = "android")]
fn jvm() -> jni::JavaVM {
    let ctx = ndk_context::android_context();
    // SAFETY: the pointer came from JNI_OnLoad, lives for the whole process.
    unsafe { jni::JavaVM::from_raw(ctx.vm() as *mut _) }
}

#[cfg(target_os = "android")]
fn audio_loop(gen: u64) {
    use jni::objects::JValue;
    use std::time::Instant;

    ALIVE.store(true, Ordering::Relaxed);
    diag_push(format!("{} native audio loop running", diag_now()));
    let alive = || GEN.load(Ordering::Relaxed) == gen;

    // The whole loop is inside the attach callback: the thread stays attached while
    // the closure hasn't returned (automatic detach at the end).
    let res: jni::errors::Result<()> = jvm().attach_current_thread(|env| {
        // --- Building AudioTrack through a Builder chain (without Activity) ---
        let attrs_builder_cls =
            env.find_class(jni::jni_str!("android/media/AudioAttributes$Builder"))?;
        let attrs_builder = env.new_object(attrs_builder_cls, jni::jni_sig!("()V"), &[])?;
        let attrs_builder = env
            .call_method(
                &attrs_builder,
                jni::jni_str!("setUsage"),
                jni::jni_sig!("(I)Landroid/media/AudioAttributes$Builder;"),
                &[JValue::Int(1)],
            )?
            .l()?;
        let attrs_builder = env
            .call_method(
                &attrs_builder,
                jni::jni_str!("setContentType"),
                jni::jni_sig!("(I)Landroid/media/AudioAttributes$Builder;"),
                &[JValue::Int(2)],
            )?
            .l()?;
        let attrs = env
            .call_method(
                &attrs_builder,
                jni::jni_str!("build"),
                jni::jni_sig!("()Landroid/media/AudioAttributes;"),
                &[],
            )?
            .l()?;

        let fmt_builder_cls =
            env.find_class(jni::jni_str!("android/media/AudioFormat$Builder"))?;
        let fmt_builder = env.new_object(fmt_builder_cls, jni::jni_sig!("()V"), &[])?;
        let fmt_builder = env
            .call_method(
                &fmt_builder,
                jni::jni_str!("setEncoding"),
                jni::jni_sig!("(I)Landroid/media/AudioFormat$Builder;"),
                &[JValue::Int(4)],
            )?
            .l()?;
        let fmt_builder = env
            .call_method(
                &fmt_builder,
                jni::jni_str!("setSampleRate"),
                jni::jni_sig!("(I)Landroid/media/AudioFormat$Builder;"),
                &[JValue::Int(48000)],
            )?
            .l()?;
        let fmt_builder = env
            .call_method(
                &fmt_builder,
                jni::jni_str!("setChannelMask"),
                jni::jni_sig!("(I)Landroid/media/AudioFormat$Builder;"),
                &[JValue::Int(12)],
            )?
            .l()?;
        let fmt = env
            .call_method(
                &fmt_builder,
                jni::jni_str!("build"),
                jni::jni_sig!("()Landroid/media/AudioFormat;"),
                &[],
            )?
            .l()?;

        let track_builder_cls =
            env.find_class(jni::jni_str!("android/media/AudioTrack$Builder"))?;
        let track_builder = env.new_object(track_builder_cls, jni::jni_sig!("()V"), &[])?;
        let track_builder = env
            .call_method(
                &track_builder,
                jni::jni_str!("setAudioAttributes"),
                jni::jni_sig!(
                    "(Landroid/media/AudioAttributes;)Landroid/media/AudioTrack$Builder;"
                ),
                &[JValue::Object(&attrs)],
            )?
            .l()?;
        let track_builder = env
            .call_method(
                &track_builder,
                jni::jni_str!("setAudioFormat"),
                jni::jni_sig!(
                    "(Landroid/media/AudioFormat;)Landroid/media/AudioTrack$Builder;"
                ),
                &[JValue::Object(&fmt)],
            )?
            .l()?;
        let track_builder = env
            .call_method(
                &track_builder,
                jni::jni_str!("setBufferSizeInBytes"),
                jni::jni_sig!("(I)Landroid/media/AudioTrack$Builder;"),
                &[JValue::Int(TRACK_BUF_BYTES)],
            )?
            .l()?;
        let track_builder = env
            .call_method(
                &track_builder,
                jni::jni_str!("setTransferMode"),
                jni::jni_sig!("(I)Landroid/media/AudioTrack$Builder;"),
                &[JValue::Int(1)],
            )?
            .l()?;
        // We do NOT set setPerformanceMode: on some devices any value other than
        // the default throws IllegalArgument ("Invalid
        // performance mode") and kills the stream. The default works everywhere.
        let track = env
            .call_method(
                &track_builder,
                jni::jni_str!("build"),
                jni::jni_sig!("()Landroid/media/AudioTrack;"),
                &[],
            )?
            .l()?;
        let state: i32 = env
            .call_method(&track, jni::jni_str!("getState"), jni::jni_sig!("()I"), &[])?
            .i()?;
        if state != 1 {
            diag_push(format!("{} bad track state: {state}", diag_now()));
            return Err(jni::errors::Error::JavaException);
        }
        // Global reference: it survives the loop's local frames.
        // We delete the local reference immediately (otherwise it accumulates in the ref table).
        let track_global = env.new_global_ref(&track)?;
        env.delete_local_ref(track);

        let mut counters = Counters {
            chunks: 0,
            slow: 0,
            render_ms_total: 0,
            pending_max: 0,
            written: 0,
            last_head: 0,
            head_base: 0,
        };
        let mut last_summary = Instant::now();
        let mut last_pushed = 0u64;
        let controller = crate::api::playback::shared_controller();
        // Prefill: 3 chunks into the buffer BEFORE play — startup without an underrun gap
        // (otherwise the track consumes faster than we produce from the very first millisecond). An
        // error or empty buffer here isn't fatal: the main
        // loop below will exit via the existing "write failed" path.
        for _ in 0..3 {
            if !alive() {
                break;
            }
            let pcm = controller.render_pull(CHUNK_FRAMES as u64, TRACK_SR);
            if pcm.is_empty() {
                break;
            }
            capture_write(&pcm);
            let write_res: jni::errors::Result<i32> =
                env.with_local_frame(8, |env| {
                    let arr = env.new_float_array(pcm.len())?;
                    arr.set_region(env, 0, &pcm)?;
                    let n = env
                        .call_method(
                            &track_global,
                            jni::jni_str!("write"),
                            jni::jni_sig!("([FIII)I"),
                            &[
                                JValue::Object(&arr),
                                JValue::Int(0),
                                JValue::Int(pcm.len() as i32),
                                JValue::Int(1),
                            ],
                        )?
                        .i()?;
                    Ok(n)
                });
            match write_res {
                Ok(n) if n >= 0 => {
                    counters.written += pcm.len() as u64 / 2;
                }
                _ => {
                    break;
                }
            }
        }

        env.call_method(&track_global, jni::jni_str!("play"), jni::jni_sig!("()V"), &[])?;
        while alive() {
            // Realtime cadence: a chunk covers 0.05 s — if we finished early,
            // wait out the remainder. Without this, with non-blocking write the loop
            // spins hundreds of chunks per second (we saw 670/s), eats a CPU core
            // and starves the composer — hence the rare notes.
            let chunk_start = Instant::now();
            let t0 = Instant::now();
            let pcm = controller.render_pull(CHUNK_FRAMES as u64, TRACK_SR);
            let ms = t0.elapsed().as_millis() as u64;
            counters.chunks += 1;
            counters.render_ms_total += ms;
            counters.pending_max =
                counters.pending_max.max(controller.pump_pending_len());
            if ms > 25 {
                counters.slow += 1;
                diag_push(format!(
                    "{} SLOW render: {ms}ms (slow={})",
                    diag_now(),
                    counters.slow
                ));
            } else if counters.chunks % 100 == 0 {
                let avg = counters.render_ms_total / counters.chunks.max(1);
                let queued = controller.engine_stats().queued;
                // Track state and real tempo: chunks per second
                // around 20 (50 ms chunks) — writing never stalls anywhere
                // (the track doesn't consume). 3 = PLAYING.
                let elapsed = last_summary.elapsed().as_secs_f32().max(0.01);
                last_summary = Instant::now();
                let rate = 100.0 / elapsed;
                let play_state: i32 = env
                    .call_method(
                        &track_global,
                        jni::jni_str!("getPlayState"),
                        jni::jni_sig!("()I"),
                        &[],
                    )
                    .map(|v| v.i().unwrap_or(-99))
                    .unwrap_or(-99);
                // Composer tempo: how many notes were generated in a window (5 s).
                // ~150 = full mix, ~10 = composer is idle.
                let pushed_now = controller.engine_stats().pushed;
                let pushed_delta = pushed_now.saturating_sub(last_pushed);
                last_pushed = pushed_now;
                // Clock drift: given to the track minus consumed by the head.
                // Growing — we produce faster than consumption (buffer swells,
                // normal after prefill ~150-300 ms); near zero for a long time —
                // we're running on the edge (underrun risk on the first spike).
                let head: i32 = env
                    .call_method(
                        &track_global,
                        jni::jni_str!("getPlaybackHeadPosition"),
                        jni::jni_sig!("()I"),
                        &[],
                    )
                    .map(|v| v.i().unwrap_or(-1))
                    .unwrap_or(-1);
                let drift_txt = if head >= 0 {
                    let head = head as i64;
                    if head < counters.last_head - 1_000_000 {
                        // Wrap of the 32-bit counter (~12.4 h at 48 kHz).
                        counters.head_base += 0x1_0000_0000;
                    }
                    counters.last_head = head;
                    let drift_ms = (counters.written as i64
                        - (counters.head_base + head))
                        * 1000
                        / TRACK_SR as i64;
                    // Track buffer queue for the visualizer: the wheel
                    // shows what sounds, not what was rendered.
                    LATENCY_MS.store(drift_ms.max(0), Ordering::Relaxed);
                    format!("{drift_ms}ms")
                } else {
                    "n/a".to_string()
                };
                diag_push(format!(
                    "{} render avg {avg}ms (slow={} queued={queued} pending_max={} rate={rate:.0}/s playstate={play_state} pushed={pushed_delta} drift={drift_txt})",
                    diag_now(),
                    counters.slow,
                    counters.pending_max
                ));
                counters.pending_max = 0;
                if play_state != 3 {
                    // Track isn't playing: push play again.
                    diag_push(format!(
                        "{} track not playing ({play_state}), replay",
                        diag_now()
                    ));
                    let _ = env.call_method(
                        &track_global,
                        jni::jni_str!("play"),
                        jni::jni_sig!("()V"),
                        &[],
                    );
                }
            }
            if pcm.is_empty() {
                std::thread::sleep(std::time::Duration::from_millis(200));
                continue;
            }
            capture_write(&pcm);
            // Write in one piece in a local frame: the array is deleted
            // right away, the reference table doesn't grow for the whole track.
            let write_res: jni::errors::Result<i32> =
                env.with_local_frame(8, |env| {
                    let arr = env.new_float_array(pcm.len())?;
                    arr.set_region(env, 0, &pcm)?;
                    let n = env
                        .call_method(
                            &track_global,
                            jni::jni_str!("write"),
                            jni::jni_sig!("([FIII)I"),
                            &[
                                JValue::Object(&arr),
                                JValue::Int(0),
                                JValue::Int(pcm.len() as i32),
                                JValue::Int(1),
                            ],
                        )?
                        .i()?;
                    Ok(n)
                });
            match write_res {
                Ok(n) if n >= 0 => {
                    counters.written += pcm.len() as u64 / 2;
                }
                _ => {
                    // Track died (route without an event): we exit, Dart
                    // will recreate the loop on a play command (as heal did before).
                    diag_push(format!("{} track write failed, exiting", diag_now()));
                    break;
                }
            }
            // Cadence: a chunk covers 0.05 s of audio — wait out the remainder AFTER
            // the write. The track gets the chunk right after render (the buffer doesn't
            // starve because of sleep), and the loop doesn't spin idle at
            // hundreds of chunks/s (ate a CPU core and starved the composer).
            let spent = chunk_start.elapsed();
            let quantum = std::time::Duration::from_millis(50);
            if spent < quantum {
                std::thread::sleep(quantum - spent);
            }
        }

        // Cleanup: pause, reset buffer, release.
        let _ = env.call_method(&track_global, jni::jni_str!("pause"), jni::jni_sig!("()V"), &[]);
        let _ = env.call_method(&track_global, jni::jni_str!("flush"), jni::jni_sig!("()V"), &[]);
        let _ = env.call_method(&track_global, jni::jni_str!("release"), jni::jni_sig!("()V"), &[]);
        // We close an unfinished capture: the file is whole (as far as we got),
        // the slot is free for the next one.
        {
            let mut guard = CAPTURE.lock().unwrap_or_else(|e| e.into_inner());
            if guard.is_some() {
                *guard = None;
                diag_push(format!("{} capture closed on stop", diag_now()));
            }
        }
        Ok(())
    });
    if let Err(e) = res {
        diag_push(format!("{} audio loop failed: {e}", diag_now()));
    }
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
        // Without a live stream a capture doesn't start (nowhere to write).
        assert!(!capture_start(""));
        // The stream isn't running in tests.
        assert!(!thread_alive());
    }
}
