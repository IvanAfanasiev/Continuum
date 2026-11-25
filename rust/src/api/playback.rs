//! Playback control for Flutter.
//!
//! Every function returns [`PlaybackStatus`] — an up-to-date state snapshot,
//! so the Dart side always sees fact, not assumption.
//! No function (except an explicit `playback_stop`) terminates the main
//! loop: pause, tempo, style, seed and volumes change live.
//!
//! Slot semantics: volume > 0 = the slot is used in generation,
//! 0 = excluded (not generated, no DSP load).

use std::sync::OnceLock;

use crate::playback::{
    EngineStats, GroupInfo, PlaybackController, PlaybackStatus, StyleMeta, TrackInfo,
};

fn controller() -> &'static PlaybackController {
    static CONTROLLER: OnceLock<PlaybackController> = OnceLock::new();
    CONTROLLER.get_or_init(PlaybackController::new)
}

/// Controller handle for the native audio stream (the same singleton,
/// the same locks — Dart and the stream see one state).
/// Android only: nobody else calls it.
#[cfg(target_os = "android")]
pub(crate) fn shared_controller() -> &'static PlaybackController {
    controller()
}

pub fn playback_start(style_name: String, seed: u64) -> PlaybackStatus {
    controller().start(&style_name, seed, true)
}

pub fn playback_stop() -> PlaybackStatus {
    controller().stop()
}

pub fn playback_set_paused(paused: bool) -> PlaybackStatus {
    controller().set_paused(paused)
}

pub fn playback_set_style(style_name: String) -> PlaybackStatus {
    controller().set_style(&style_name)
}

pub fn playback_set_seed(seed: u64) -> PlaybackStatus {
    controller().set_seed(seed)
}

/// Seed by code `XXX-XXX` (alphabet 0-9A-Z, case-insensitive).
/// Garbage is discarded, too-short input is rejected without changing the seed.
pub fn playback_set_seed_code(code: String) -> PlaybackStatus {
    controller().set_seed_code(&code)
}

/// Random seed ("dice" button): the code is returned in the status.
pub fn playback_random_seed() -> PlaybackStatus {
    controller().random_seed()
}

/// Live tempo in BPM (typically 75–135, clamped to 50–180).
/// Non-positive = reset to the style default.
pub fn playback_set_tempo_bpm(bpm: f32) -> PlaybackStatus {
    controller().set_tempo_bpm(bpm)
}

/// Live intensity 0..2 (the "temperature" of generation).
pub fn playback_set_intensity(intensity: f32) -> PlaybackStatus {
    controller().set_intensity(intensity)
}

/// Master effects live: reverb wet 0..0.6, size 0..1,
/// high-cut 2000..20000 Hz, sidechain depth 0..2.
/// They survive style and seed changes; reset is a separate function.
pub fn playback_set_master_fx(
    reverb_wet: f32,
    reverb_size: f32,
    treble_hz: f32,
    sidechain: f32,
) -> PlaybackStatus {
    controller().set_master_fx(reverb_wet, reverb_size, treble_hz, sidechain)
}

/// Reset master effects to the current style's authored values.
pub fn playback_reset_master_fx() -> PlaybackStatus {
    controller().reset_master_fx()
}

/// Filter openness: offset -0.5..0.5 on top of the section shape.
pub fn playback_set_tone_open(offset: f32) -> PlaybackStatus {
    controller().set_tone_open(offset)
}

/// Slot volume 0..1. Zero excludes the slot from generation.
/// A manual edit marks the list as changed: style and seed changes
/// no longer overwrite it (reset is a separate button).
pub fn playback_set_track(instrument_id: u64, volume: f32) -> PlaybackStatus {
    controller().set_track(instrument_id, volume)
}

/// Reset the instrument list to the current style's and seed's default cast.
/// Clears the "listener edited" flag: style and seed affect
/// the lineup again.
pub fn playback_reset_instruments() -> PlaybackStatus {
    controller().reset_instruments()
}

/// Group macro fader 0..1 ("kick", "lead", …): one "Kick" instead of
/// "punchy vs 808" — the specific variant under the hood is chosen by the style.
/// Zero mutes the whole group. Survives style changes.
pub fn playback_set_group_volume(group_name: String, volume: f32) -> PlaybackStatus {
    controller().set_group_volume(&group_name, volume)
}

/// Mixer slots for UI: id, names, roles, groups, audible volumes.
pub fn playback_tracks() -> Vec<TrackInfo> {
    controller().tracks()
}

/// Macro faders for UI: group name and its multiplier.
pub fn playback_groups() -> Vec<GroupInfo> {
    controller().groups()
}

pub fn playback_status() -> PlaybackStatus {
    controller().status()
}

/// Live pipeline diagnostics (frames/notes/queue) for the UI banner.
pub fn playback_engine_stats() -> EngineStats {
    controller().engine_stats()
}

/// Start a pull session for the frontend (Android plays on its own):
/// the composer runs, there is no audio stream, samples are pulled on demand.
pub fn playback_pull_start(style_name: String, seed: u64) -> PlaybackStatus {
    controller().start(&style_name, seed, false)
}

/// Pull block for the frontend: stereo interleaved f32.
/// Call repeatedly in small blocks (~10 times a second — onsets
/// are more accurate than the grid step), tempo is set by the consumer.
pub fn playback_pull_render(frames: u64, sample_rate: f32) -> Vec<f32> {
    controller().render_pull(frames, sample_rate)
}

/// Native audio stream (Android): session + render + track write
/// entirely in Rust, Dart only sends commands. Status is for UI.
pub fn playback_pull_thread_start(style_name: String, seed: u64) -> PlaybackStatus {
    let status = controller().start(&style_name, seed, false);
    crate::pull_thread::thread_start();
    status
}

/// Stop the native stream. The session stays alive (pause is held by the backend).
pub fn playback_pull_thread_stop() -> PlaybackStatus {
    crate::pull_thread::thread_stop();
    controller().status()
}

/// Is the native stream alive (for UI and self-diagnostics).
pub fn playback_pull_alive() -> bool {
    crate::pull_thread::thread_alive()
}

/// Capture the stream to a file (30 seconds, for examining transients).
/// The path is outside the app's public folder.
pub fn playback_pull_capture(path: String) -> bool {
    crate::pull_thread::capture_start(&path)
}

/// Native stream log for the diagnostics screen.
pub fn playback_pull_diag() -> Vec<String> {
    crate::pull_thread::diag_lines()
}

/// Visualizer frame from the live session. Without a session — zeros.
pub fn playback_live_visual() -> crate::playback::LiveVisual {
    controller().live_visual()
}

pub fn playback_style_names() -> Vec<String> {
    crate::styles::style_names()
        .iter()
        .map(|s| s.to_string())
        .collect()
}

/// Styles with catalog shelves for frontend sorting.
pub fn playback_styles() -> Vec<StyleMeta> {
    crate::styles::style_names()
        .iter()
        .filter_map(|name| {
            crate::styles::make_style(name).map(|s| StyleMeta {
                name: s.name().to_string(),
                group: s.group().name().to_string(),
            })
        })
        .collect()
}
