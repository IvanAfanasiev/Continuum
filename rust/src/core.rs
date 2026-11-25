//! Shared musical primitives and constants.
//!
//! The single home for formulas that used to be duplicated
//! across instruments and styles (MIDI-to-frequency conversion, step
//! duration, swing). Removes "magic numbers" from DSP code.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::RwLock;

/// Reading shared state without panicking on a poisoned lock:
/// instead of panicking we return the last value. Replaces dozens of
/// `unwrap_or_else(|e| e.into_inner())` calls across the codebase.
pub(crate) fn snapshot<T: Clone>(lock: &RwLock<T>) -> T {
    lock.read()
        .map(|g| g.clone())
        .unwrap_or_else(|e| e.into_inner().clone())
}

/// Drive into the master clipper: the mix enters soft saturation instead of
/// a hard `clamp` — this glues the mix together and adds density to the lows.
/// 0.55: phones demand loudness (commercial rms ~0.3 versus our
/// 0.1). Peaks are still in the tanh region: crest is preserved, no clipping.
/// Beyond that — only a true limiter (a separate mastering task).
pub const MASTER_DRIVE: f32 = 0.8;

/// Audio engine version: increments on any change audible
/// in generation (guides, voices, scheduler). Reported in status:
/// a favorite seed sounds different after an update — the version explains why.
/// Policy: slot numbers are frozen (see Slot), the sound of old seeds
/// may change as the version grows; freezing old pipelines is
/// a separate task if a compatibility museum is ever needed.
pub const ENGINE_VERSION: u32 = 2;

/// Upper bound on the number of simultaneously active tracks.
/// With headroom above the slot count (17): an array, not a Vec — no allocations
/// in the hot path.
pub const MAX_ACTIVE_TRACKS: usize = 24;

/// Capacity of the lock-free note queue (formerly `1024` in `main.rs`).
pub const NOTE_QUEUE_CAPACITY: usize = 1024;

/// Default seed when not passed via CLI.
pub const DEFAULT_SEED: u64 = 424242;

/// Bounds of the live tempo knob, BPM. Single source of truth for UI,
/// composer, offline render and status — so clamping doesn't live only
/// in one setter and diverge from the actually sounding tempo.
pub const MIN_TEMPO_BPM: f32 = 50.0;
pub const MAX_TEMPO_BPM: f32 = 180.0;

/// BPM validation: non-numbers and non-positive values mean "no value",
/// the rest is clamped to [`MIN_TEMPO_BPM`]..=[`MAX_TEMPO_BPM`].
/// Pure function: the caller decides whether to use the style default.
pub fn normalize_tempo(bpm: f32) -> Option<f32> {
    if bpm.is_finite() && bpm > 0.0 {
        Some(bpm.clamp(MIN_TEMPO_BPM, MAX_TEMPO_BPM))
    } else {
        None
    }
}

/// Convert a MIDI note to frequency, Hz. Equal-temperament tuning formula.
/// Used to be copied into `bass.rs` and `pad.rs` (in pad — with an `i8` cast bug).
pub fn midi_to_freq(midi: u8) -> f32 {
    440.0 * 2.0_f32.powf((midi as f32 - 69.0) / 12.0)
}

/// Duration of one grid step in seconds.
pub fn base_step_secs(bpm: f32, steps_per_measure: usize) -> f32 {
    60.0 / bpm / (steps_per_measure as f32 / 4.0)
}

/// Step duration with swing: even steps lengthen,
/// odd ones shorten (the sum of a pair = 2 * base).
pub fn swing_step_secs(base: f32, swing_ratio: f32, step_in_measure: usize) -> f32 {
    base * if step_in_measure.is_multiple_of(2) {
        swing_ratio
    } else {
        2.0 - swing_ratio
    }
}

/// Filter-openness offset by the listener: the composer sets the shape per section,
/// the knob adds a constant offset on top of it (-0.5..0.5).
static TONE_OPEN_OFFSET: AtomicU32 = AtomicU32::new(0);

pub fn set_tone_open_offset(value: f32) {
    let offset = if value.is_finite() {
        value.clamp(-0.5, 0.5)
    } else {
        0.0
    };
    TONE_OPEN_OFFSET.store(offset.to_bits(), Ordering::Relaxed);
}

pub fn tone_open_offset() -> f32 {
    f32::from_bits(TONE_OPEN_OFFSET.load(Ordering::Relaxed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a4_is_440hz() {
        assert!((midi_to_freq(69) - 440.0).abs() < 0.01);
    }

    #[test]
    fn midi_extremes_are_finite() {
        assert!(midi_to_freq(0).is_finite());
        assert!(midi_to_freq(127).is_finite());
    }

    #[test]
    fn swing_pair_sums_to_double_base() {
        let base = 0.125;
        let even = swing_step_secs(base, 1.3, 0);
        let odd = swing_step_secs(base, 1.3, 1);
        assert!(((even + odd) - 2.0 * base).abs() < 1e-6);
    }
}
