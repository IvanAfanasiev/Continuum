//! Audio engine: drain the note queue -> trigger voices -> sidechain ->
//! stereo mix -> space -> master.
//!
//! A modern club pipeline in miniature:
//! - sidechain pump: the kick envelope ducks bass/pads (depth and bounce);
//! - stereo: lows centered, polyphony spread across the sides by panners;
//! - master space (dark reverb) + a soft clipper instead of clamp.

use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, Mutex, RwLock,
};
use std::time::{Duration, Instant};

use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    SampleFormat,
};
use crossbeam::queue::ArrayQueue;

use crate::core::{snapshot, MASTER_DRIVE, MAX_ACTIVE_TRACKS};
use crate::instruments::dsp::{fast_tanh, MasterLimiter};
use crate::instruments::{dsp::Svf, Instrument, InstrumentRole, NoteEvent};
use crate::mixer::MixerState;
use crate::playback::EngineCounters;
use crate::space::SpaceBus;

/// Kick envelope decay: ~180 ms recovery (house-tempo pump).
const DUCK_RELEASE_SECS: f32 = 0.18;
/// Envelope scaling to the 0..1 range (kick peak after volume ~0.8).
const DUCK_ENV_SCALE: f32 = 1.25;
/// Sidechain detector's low cut: only the kick's LOW end should pump the bass.
/// A click-beater at 3.5 kHz used to put wideband hash into the envelope —
/// the pump twitched on every click instead of on the body of the hit.
const DUCK_DETECT_HZ: f32 = 180.0;
/// Maximum instrument id for the volume table (O(1) access).
/// Slot range 1..20, headroom up to 64.
const VOL_ID_CAP: usize = 64;

/// Master effects from the style: reverb room and high-cut.
/// The controller rewrites them on a style change, the engine reads every block.
#[derive(Debug, Clone, Copy)]
pub struct MasterFx {
    pub reverb_wet: f32,
    pub reverb_size: f32,
    pub treble_hz: f32,
    /// Sidechain depth multiplier (1.0 — the engine's character).
    pub sidechain: f32,
}

impl Default for MasterFx {
    fn default() -> Self {
        Self {
            reverb_wet: 0.16,
            reverb_size: 0.5,
            treble_hz: 19000.0,
            sidechain: 1.0,
        }
    }
}

pub type SharedFx = Arc<RwLock<MasterFx>>;

/// Render state without cpal: voices, sidechain, room, highs.
/// There is no tempo delay in the pipeline (disabled: situational, smears
/// transients; `Echo` itself lives in space.rs and will return as a separate
/// feature with sends from the mixer rather than hardcoded by role).
/// The live engine and the offline run (soak) use the very same code —
/// no divergence between "what I hear" and "what I check" by construction.
pub(crate) struct RenderState {
    instruments: Vec<Box<dyn Instrument>>,
    index_by_id: HashMap<usize, usize>,
    is_kick: Vec<bool>,
    space: SpaceBus,
    treble_l: Svf,
    treble_r: Svf,
    kick_env: f32,
    /// LP filter of the sidechain detector (only the kick's low end).
    kick_lp: f32,
    kick_lp_a: f32,
    /// Volume table by id: replacing a linear search on every sample.
    vol_cache: [f32; VOL_ID_CAP],
    vol_key: u64,
    duck_release: f32,
    /// Fast envelope attack (2 ms): the pump keeps up with the kick,
    /// but doesn't crackle with wideband hash on the hit's front edge.
    duck_attack: f32,
    duck_mult: f32,
    /// Master HP at 28 Hz: cuts infrabass and DC (phone speaker rasp),
    /// doesn't touch musical content (minimum 32 Hz).
    hp_l: Svf,
    hp_r: Svf,
    sample_rate: f32,
    /// Smoothed high-cut: the target jumps at section boundaries
    /// (form sweep), the filter follows it over ~120 ms instead of stepping —
    /// the step in the SVF coefficient was the rare "gulp".
    treble_smooth: f32,
    /// Master peak limiter: makeup + ceiling after the whole chain.
    limiter: MasterLimiter,
    /// High-cut smoothing coefficient (~120 ms): a sample-rate constant,
    /// computed in new/retune, not per sample.
    smooth_k: f32,
}

impl RenderState {
    /// Full reset of voices and the room tail: seed or style change.
    /// Old generation must not keep playing over the new one.
    pub(crate) fn reset_voices(&mut self) {
        for inst in self.instruments.iter_mut() {
            inst.reset();
        }
        self.space.reset();
        self.kick_env = 0.0;
        self.limiter.reset();
    }

    pub(crate) fn new(mut instruments: Vec<Box<dyn Instrument>>, sample_rate: f32) -> Self {
        let sample_rate = if sample_rate.is_finite() && sample_rate > 0.0 {
            sample_rate
        } else {
            44_100.0
        };
        for inst in &mut instruments {
            inst.set_sample_rate(sample_rate);
        }
        let index_by_id = instruments
            .iter()
            .enumerate()
            .map(|(pos, inst)| (inst.id(), pos))
            .collect();
        // Mask of kicks for the sidechain: we render them first in every sample.
        let is_kick: Vec<bool> = instruments
            .iter()
            .map(|inst| inst.role() == InstrumentRole::Kick)
            .collect();
        Self {
            instruments,
            index_by_id,
            is_kick,
            space: SpaceBus::new(sample_rate),
            treble_l: Svf::default(),
            treble_r: Svf::default(),
            kick_env: 0.0,
            kick_lp: 0.0,
            kick_lp_a: 1.0 - (-2.0 * std::f32::consts::PI * DUCK_DETECT_HZ / sample_rate).exp(),
            vol_cache: [0.0; VOL_ID_CAP],
            vol_key: u64::MAX,
            duck_release: (-1.0 / (sample_rate * DUCK_RELEASE_SECS)).exp(),
            duck_attack: (-1.0 / (sample_rate * 0.002)).exp(),
            duck_mult: 1.0,
            hp_l: Svf::default(),
            hp_r: Svf::default(),
            sample_rate,
            treble_smooth: 0.0,
            limiter: MasterLimiter::default(),
            smooth_k: 1.0 - (-1.0 / (sample_rate * 0.12)).exp(),
        }
    }

    /// Retuning to a new sample rate (output route change):
    /// voices and their state are preserved, only
    /// sample-rate-dependent nodes are reset (room, highs, sidechain).
    pub(crate) fn retune(&mut self, sample_rate: f32) {
        let sr = if sample_rate.is_finite() && sample_rate > 0.0 {
            sample_rate
        } else {
            44_100.0
        };
        for inst in &mut self.instruments {
            inst.set_sample_rate(sr);
        }
        // This used to be `self.space = SpaceBus::new(sr)` — the reverb
        // tail was cut on every headphone reconnect.
        // Now the sample-rate-dependent nodes are retuned in place.
        self.space.retune(sr);
        self.kick_lp_a = 1.0 - (-2.0 * std::f32::consts::PI * DUCK_DETECT_HZ / sr).exp();
        self.treble_l = Svf::default();
        self.treble_r = Svf::default();
        self.treble_smooth = 0.0;
        self.hp_l = Svf::default();
        self.hp_r = Svf::default();
        self.duck_release = (-1.0 / (sr * DUCK_RELEASE_SECS)).exp();
        self.duck_attack = (-1.0 / (sr * 0.002)).exp();
        self.smooth_k = 1.0 - (-1.0 / (sr * 0.12)).exp();
        self.sample_rate = sr;
    }

    pub(crate) fn push_note(&mut self, note: &NoteEvent) {
        if let Some(&pos) = self.index_by_id.get(&note.instrument_id) {
            if let Some(inst) = self.instruments.get_mut(pos) {
                inst.trigger(note);
            }
        }
    }

    pub(crate) fn set_room(&mut self, wet: f32, size: f32) {
        self.space.set_room(wet, size);
    }

    pub(crate) fn set_duck_mult(&mut self, mult: f32) {
        self.duck_mult = mult.clamp(0.0, 2.0);
    }

    pub(crate) fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    /// Rebuild the volume table only when it actually changed.
    /// In the live pipeline `volumes` is the same slice every sample — instead of
    /// a linear search over 14 instruments we get one hash per pass
    /// and O(1) lookup by id inside the render loop.
    fn sync_volumes(&mut self, volumes: &[(usize, f32)]) {
        let mut key = 0xcbf2_9ce4_8422_2325u64;
        for &(id, vol) in volumes {
            key ^= (id as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ vol.to_bits() as u64;
            key = key.wrapping_mul(0x0100_0000_01b3);
        }
        if key == self.vol_key {
            return;
        }
        self.vol_cache.fill(0.0);
        for &(id, vol) in volumes {
            if id < VOL_ID_CAP {
                self.vol_cache[id] = vol;
            }
        }
        self.vol_key = key;
    }

    #[inline]
    fn vol_of(cache: &[f32; VOL_ID_CAP], id: usize) -> f32 {
        cache.get(id).copied().unwrap_or(0.0)
    }

    /// One stereo sample: sidechain → mix → echo → room → highs → master.
    /// `volumes` — effective volumes (id, vol); zero, faded-out
    /// voices are skipped without DSP.
    pub(crate) fn render_sample(&mut self, volumes: &[(usize, f32)], treble_hz: f32) -> (f32, f32) {
        self.sync_volumes(volumes);
        let cache = &self.vol_cache;
        // Pass 1: kicks. Their level charges the sidechain envelope.
        // We skip a silent, faded-out kick (as with the others in pass 2).
        let mut kick_l = 0.0;
        let mut kick_r = 0.0;
        for (pos, inst) in self.instruments.iter_mut().enumerate() {
            if !self.is_kick[pos] {
                continue;
            }
            let vol = Self::vol_of(cache, inst.id());
            if vol <= 0.0 && !inst.is_active() {
                continue;
            }
            let (l, r) = inst.next_sample_stereo();
            kick_l += l * vol;
            kick_r += r * vol;
        }
        let kick_abs = (kick_l + kick_r) * 0.5;
        // Detector LP: only the kick's LOW end (180 Hz) enters the sidechain,
        // the 3.5 kHz click-beater no longer twitches the envelope.
        self.kick_lp += self.kick_lp_a * (kick_abs - self.kick_lp);
        let kick_detect = self.kick_lp.abs();
        // Attack is smoothed (2 ms): the hit's front edge doesn't produce wideband
        // hash in the envelope, while the groove doesn't suffer — the decay is the same.
        let a = if kick_detect > self.kick_env {
            self.duck_attack
        } else {
            self.duck_release
        };
        self.kick_env = kick_detect + (self.kick_env - kick_detect) * a;
        if self.kick_env < 1e-12 {
            self.kick_env = 0.0;
        }

        // Pass 2: the remaining voices with ducking by role.
        // We skip a silent and fully faded-out voice entirely:
        // an excluded slot spends no DSP. A muted but still
        // sounding tail is advanced with no contribution, so re-enabling
        // doesn't fire a stale sound.
        let mut mix_l = kick_l;
        let mut mix_r = kick_r;
        for (pos, inst) in self.instruments.iter_mut().enumerate() {
            if self.is_kick[pos] {
                continue;
            }
            let vol = Self::vol_of(cache, inst.id());
            if vol > 0.0 {
                let duck = duck_gain(inst.role(), self.kick_env, self.duck_mult);
                let (l, r) = inst.next_sample_stereo();
                mix_l += l * vol * duck;
                mix_r += r * vol * duck;
            } else if inst.is_active() {
                let _ = inst.next_sample_stereo();
            }
        }

        // The clean mix goes straight to the room, with no delay in between.
        let (wet_l, wet_r) = self.space.process(mix_l, mix_r);
        // Style's high-cut: lofi is murky, club is surgical.
        // The target is smoothed: a jump at a section boundary (form sweep) travels
        // over ~120 ms rather than stepping into the filter coefficient.
        let target = treble_hz.clamp(2000.0, 20000.0);
        if self.treble_smooth <= 0.0 {
            self.treble_smooth = target;
        }
        self.treble_smooth += (target - self.treble_smooth) * self.smooth_k;
        let treble = self.treble_smooth;
        let sr = self.sample_rate;
        let (t_l, _, _) = self.treble_l.process(wet_l, treble, 0.7, sr);
        let (t_r, _, _) = self.treble_r.process(wet_r, treble, 0.7, sr);
        // Master HP: infrabass and DC out (speaker rasp),
        // music starts at 32 Hz (808) — untouched.
        let (_, _, h_l) = self.hp_l.process(t_l, 28.0, 0.7, sr);
        let (_, _, h_r) = self.hp_r.process(t_r, 28.0, 0.7, sr);
        // Limiter last: body makeup + peak ceiling, then tanh.
        let (l_l, l_r) = self.limiter.process(h_l, h_r, sr);
        (master_sample(l_l), master_sample(l_r))
    }
}

/// Output stream pump: block preparation (notes from the queue,
/// effective volumes, live FX) + per-sample render with a pause fade.
///
/// One code path for all sample formats: some Android devices deliver
/// only I16/U16, and a monomorphic F32 stream couldn't be built there (silence
/// with no visible error). Conversion lives in thin `run` closures.
///
/// Cap on queue draining during block preparation (see begin_block).
const MAX_DRAIN_PER_BLOCK: u64 = 32;
/// Cap on due-note fires in one frame (see next_frame): chord
/// stacks (3–4 notes with one due) pass in full, while a batch after a stall
/// is spread over frames instead of hitting in one sample.
const MAX_FIRES_PER_FRAME: usize = 8;
/// Age of the due schedule after which a note counts as stale
/// and is dropped (see next_frame). 250 ms: longer than a 16th at 60 BPM —
/// playing it then would be lying about the rhythm.
const MAX_LATE_SECS: f32 = 0.25;
pub(crate) struct Pump {
    queue: Arc<ArrayQueue<NoteEvent>>,
    /// Step stamps (honest length of each step): the time grid is built from them.
    marks: Arc<ArrayQueue<crate::composer::StepMark>>,
    mixer: Arc<RwLock<MixerState>>,
    is_paused: Arc<AtomicBool>,
    fx: SharedFx,
    bpm: Arc<RwLock<f32>>,
    state: RenderState,
    volumes: [(usize, f32); MAX_ACTIVE_TRACKS],
    volume_count: usize,
    treble: f32,
    pause_gain: f32,
    pause_step: f32,
    counters: EngineCounters,
    logged_first: bool,
    filter_open: Arc<RwLock<f32>>,
    /// "Reset voices" flag: a seed or style change cuts off the old
    /// generation (tails and reverb) — as the listener requested.
    voice_reset: Arc<AtomicBool>,
    /// Deferred notes (due frame, order, note), sorted by ascending due.
    /// The pump fires a note exactly at its media frame, not when it
    /// drained the queue: timing is owned by the backend, the frontend only by bytes.
    /// Order within one frame is generation order (kick first):
    /// the engine loads voices in order, and sorting by instrument id
    /// broke that order. Capacity is reserved up front: the audio callback
    /// doesn't allocate.
    pending: Vec<(u64, u64, NoteEvent)>,
    /// Order counter: monotonic so equal dues preserve their
    /// arrival order (sorting is unstable, order isn't guaranteed).
    pending_seq: u64,
    /// The grid is built from step stamps (exact timing). Then notes
    /// aren't written into the grid: the stamp already gave this step's length.
    marks_used: bool,
    /// Timing diagnostics: notes that fired noticeably later than their
    /// beat (the rhythm stumbles), and grid re-anchors (the generator
    /// fell behind — the schedule was rebuilt on the current frame).
    late_notes: u64,
    reanchors: u64,
    /// Notes of the current block before merging with `pending`: a reusable
    /// buffer so `begin_block` doesn't allocate in the audio path.
    fresh: Vec<(u64, u64, NoteEvent)>,
    /// Media clock: rendered frames. Monotonic, no drift.
    media_cursor: u64,
    /// Grid anchor: which step corresponds to which frame.
    /// Set from the first stamped note of the dry stream;
    /// while the stream is dry the next note starts "now".
    anchor_step: u64,
    anchor_frame: u64,
    anchored: bool,
    /// Step-length map (step → frames): swing makes steps unequal,
    /// the linear formula from the anchor lied more the farther the note was.
    /// We sum the true lengths from the anchor — the grid is always exact,
    /// re-anchoring is only needed on real breaks, not on every swing.
    /// Ring of the last 256 steps; evicted entries are folded into the anchor.
    step_grid: std::collections::VecDeque<(u64, u64)>,
}

impl Pump {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        queue: Arc<ArrayQueue<NoteEvent>>,
        marks: Arc<ArrayQueue<crate::composer::StepMark>>,
        instruments: Vec<Box<dyn Instrument>>,
        mixer: Arc<RwLock<MixerState>>,
        is_paused: Arc<AtomicBool>,
        fx: SharedFx,
        bpm: Arc<RwLock<f32>>,
        sample_rate: f32,
        counters: EngineCounters,
        filter_open: Arc<RwLock<f32>>,
        voice_reset: Arc<AtomicBool>,
    ) -> Self {
        // Start gain from the actual pause: recreating the stream while paused
        // must not fire 10 ms of loud sound.
        let paused_now = is_paused.load(Ordering::Relaxed);
        Self {
            queue,
            marks,
            mixer,
            is_paused,
            fx,
            bpm,
            state: RenderState::new(instruments, sample_rate),
            volumes: [(0usize, 0.0f32); MAX_ACTIVE_TRACKS],
            volume_count: 0,
            treble: MasterFx::default().treble_hz,
            // Pause gain: the play/pause transition goes through a 10 ms
            // fade, not a hard cut to zero (that cut was the click).
            pause_gain: if paused_now { 0.0 } else { 1.0 },
            pause_step: 1.0 / (0.01 * sample_rate),
            counters,
            logged_first: false,
            filter_open,
            voice_reset,
            pending: Vec::with_capacity(256),
            pending_seq: 0,
            marks_used: false,
            late_notes: 0,
            reanchors: 0,
            fresh: Vec::with_capacity(MAX_DRAIN_PER_BLOCK as usize),
            media_cursor: 0,
            anchor_step: 0,
            anchor_frame: 0,
            anchored: false,
            step_grid: std::collections::VecDeque::with_capacity(257),
        }
    }

    /// Retuning the pump to a new output rate (reconnect):
    /// mix state is preserved, gains don't tear.
    pub(crate) fn retune(&mut self, sample_rate: f32) {
        self.state.retune(sample_rate);
        self.pause_step = 1.0 / (0.01 * self.state.sample_rate);
    }

    pub(crate) fn sample_rate(&self) -> f32 {
        self.state.sample_rate()
    }

    /// Length of the due schedule (notes waiting for their media frame).
    pub(crate) fn pending_len(&self) -> usize {
        self.pending.len()
    }

    /// Block preparation: notes, volumes, live FX and echo time.
    fn begin_block(&mut self) {
        // Accumulated timing-diagnostic counters go into the shared ones (once
        // per block, not per note: the audio stream must not touch
        // atomics in the hot loop).
        if self.late_notes > 0 {
            self.counters
                .late
                .fetch_add(self.late_notes, Ordering::Relaxed);
            self.late_notes = 0;
        }
        if self.reanchors > 0 {
            self.counters
                .reanchors
                .fetch_add(self.reanchors, Ordering::Relaxed);
            self.reanchors = 0;
        }
        // try_read, not a blocking lock: the audio stream doesn't wait on a hard UI.
        // Fresh volumes are picked up by the next block (~10 ms) — imperceptibly
        // fast, while rare mixer writes don't tear the DSP.
        if let Ok(state) = self.mixer.try_read() {
            // Effective volumes via volume_for, so the mute logic
            // lives in one place (MixerState).
            self.volume_count = 0;
            for id in state.tracks.keys() {
                if self.volume_count < MAX_ACTIVE_TRACKS {
                    self.volumes[self.volume_count] = (*id, state.volume_for(*id));
                    self.volume_count += 1;
                }
            }
        }
        let mut eaten = 0u64;
        // Step stamps go first: from them the time grid is exact (every
        // step, not just steps with notes), and the anchor is set from them too:
        // "the step started now" is the current media frame, not the frame of the
        // first note (which may arrive a step later).
        let mut marks_read = 0u64;
        while let Some(mark) = self.marks.pop() {
            marks_read += 1;
            self.marks_used = true;
            let frames = (mark.us as u64 * self.state.sample_rate() as u64 / 1_000_000).max(1);
            if self.step_grid.back().map(|(s, _)| *s) == Some(mark.step) {
                continue;
            }
            // Grid from the new scale: we clear the old one entirely (a live seed/style
            // restarts numbering — otherwise the lengths run ahead).
            if mark.step < self.anchor_step
                || self
                    .step_grid
                    .back()
                    .map(|(s, _)| mark.step < *s)
                    .unwrap_or(false)
            {
                self.step_grid.clear();
                self.anchored = false;
            }
            if !self.anchored {
                // Anchor by stamp: the step started on this frame.
                self.anchor_step = mark.step;
                self.anchor_frame = self.media_cursor;
                self.anchored = true;
            }
            self.step_grid.push_back((mark.step, frames));
            // Eviction with fold: we fold the old part into the anchor,
            // the grid from it stays exact.
            while self.step_grid.len() > 256 {
                if let Some((s, f)) = self.step_grid.pop_front() {
                    if s >= self.anchor_step {
                        self.anchor_frame = self.anchor_frame.saturating_add(f);
                        self.anchor_step = s + 1;
                    }
                } else {
                    break;
                }
            }
        }
        let _ = marks_read;
        // Per-call drain cap: a backlog after a stall is spread
        // over subblocks (~12 ms apart — almost on time), rather than
        // firing all at once. A live stream (<32 notes in 12 ms is musically
        // impossible) doesn't notice the cap.
        // Stamped notes go into the due schedule (exact time),
        // legacy notes without a stamp (tests, direct calls) — straight into voices.
        // The `fresh` buffer is reused: no allocations in a block.
        self.fresh.clear();
        while eaten < MAX_DRAIN_PER_BLOCK {
            let Some(note) = self.queue.pop() else {
                break;
            };
            if note.step_us == 0 {
                self.state.push_note(&note);
            } else {
                let step_frames = note.step_us as u64 * self.state.sample_rate() as u64 / 1_000_000;
                // Step numbering reset: a live seed/style restarts the
                // form at due_step 0. A normal stream is monotonic (chords
                // of one step are duplicates), so a step behind the last one
                // in the grid means a new scale, not a late note.
                // The old scale's grid can't be folded into the anchor: its lengths
                // would push the schedule tens of seconds ahead (silence
                // after a seed change), so we clear it entirely.
                // When the grid runs on stamps, we catch the reset there too: stamps
                // arrive in batches ahead of notes, and "a step behind the tail" is
                // normal, not a scale reset.
                let numbering_reset = !self.marks_used
                    && (note.due_step < self.anchor_step
                        || self
                            .step_grid
                            .back()
                            .map(|(s, _)| note.due_step < *s)
                            .unwrap_or(false));
                if numbering_reset {
                    self.step_grid.clear();
                }
                // Length map: if we've seen the step, the length is the same, we skip
                // the duplicate; if it's new, we append it to the tail (steps go in order).
                // If the grid runs on stamps, we don't write notes into it: the stamp
                // already gave the step length (otherwise the step was doubled and the timing
                // drifted — one and a half times slower than the grid).
                if !self.marks_used && self.step_grid.back().map(|(s, _)| *s) != Some(note.due_step)
                {
                    self.step_grid
                        .push_back((note.due_step, step_frames.max(1)));
                    // Eviction with fold: we fold the old part into the anchor,
                    // the grid from it stays exact.
                    while self.step_grid.len() > 256 {
                        if let Some((s, f)) = self.step_grid.pop_front() {
                            if s >= self.anchor_step {
                                self.anchor_frame = self.anchor_frame.saturating_add(f);
                                self.anchor_step = s + 1;
                            }
                        } else {
                            break;
                        }
                    }
                }
                // Exact grid: sum the true step lengths from the anchor
                // to the note's step. An unknown tail (a gap) — linearly with the
                // current length; a numbering reset — straight into the anchor.
                let mut due = self.anchor_frame;
                if !numbering_reset && note.due_step >= self.anchor_step {
                    let mut cursor = self.anchor_step;
                    for &(gs, gf) in self.step_grid.iter() {
                        if gs < cursor || gs >= note.due_step {
                            continue;
                        }
                        due = due.saturating_add(gf);
                        cursor = gs + 1;
                    }
                    due = due
                        .saturating_add(note.due_step.saturating_sub(cursor) * step_frames.max(1));
                }
                // The anchor is set from the first stamped note and
                // holds while the grid is current: a delay longer than a step
                // (tempo change, catch-up after a stall) means "now", not
                // accumulated error. An empty pending isn't a reason
                // to re-anchor: sparse notes (pads, form pauses)
                // arrive at their beat of the shared grid, not "now".
                if numbering_reset
                    || !self.anchored
                    || due.saturating_add(step_frames.max(1)) < self.media_cursor
                {
                    if self.anchored && !numbering_reset {
                        // Re-anchoring on a live stream: the generator fell behind
                        // by more than a step (stall, tempo change, catch-up).
                        // That's exactly "the rhythm stumbled": the schedule moved
                        // to the current frame. We count it — visible in the banner.
                        self.reanchors += 1;
                    }
                    self.anchor_step = note.due_step;
                    self.anchor_frame = self.media_cursor;
                    self.anchored = true;
                    // A map older than the anchor is garbage: we clean it, memory doesn't grow.
                    while self
                        .step_grid
                        .front()
                        .map(|(s, _)| *s < self.anchor_step)
                        .unwrap_or(false)
                    {
                        self.step_grid.pop_front();
                    }
                    due = self.media_cursor;
                }
                self.fresh.push((due, self.pending_seq, note));
                self.pending_seq += 1;
            }
            eaten += 1;
        }
        if !self.fresh.is_empty() {
            self.pending.append(&mut self.fresh);
            // Unstable sort doesn't allocate (unlike stable):
            // in the audio callback that matters more. The order of equal dues is set by
            // the counter: notes of one frame stay in generation order
            // (kick first) — the engine loads voices in order, and sorting
            // by instrument id broke that order.
            self.pending
                .sort_unstable_by_key(|(due, seq, _)| (*due, *seq));
        }
        if eaten > 0 {
            self.counters.consumed.fetch_add(eaten, Ordering::Relaxed);
        }
        let fx_now = snapshot(&self.fx);
        self.state.set_room(fx_now.reverb_wet, fx_now.reverb_size);
        self.state.set_duck_mult(fx_now.sidechain);
        // Form sweep: openness 0..1 mapped onto a fraction of the base high-cut.
        // A closed filter (~1/8 of base) is a dull intro, an open one is the drop.
        let open = snapshot(&self.filter_open).clamp(0.0, 1.0);
        self.treble = fx_now.treble_hz * openness_gain(open);
        // The live tempo is published by the composer (bpm_out) and read here
        // for future tempo features (the echo will return with a grid from it too).
        let _live_bpm =
            snapshot(&self.bpm).clamp(crate::core::MIN_TEMPO_BPM, crate::core::MAX_TEMPO_BPM);
    }

    /// One stereo f32 frame with the pause fade advanced.
    /// Before rendering, due notes whose time has come are fired.
    ///
    /// Guard against a volley after a stall: at most
    /// [`MAX_FIRES_PER_FRAME`] notes fire in one frame, the rest catch up in later frames
    /// (a stretch of a few ms instead of the whole batch in one sample —
    /// the batch broke polyphony/theft and sounded raspy). Notes late by
    /// more than [`MAX_LATE_SECS`] are dropped silently: playing a bar
    /// three seconds old after a freeze is worse than skipping it.
    fn next_frame(&mut self) -> (f32, f32) {
        let late_limit = (self.state.sample_rate() * MAX_LATE_SECS) as u64;
        // Clearly late (more than an eighth of a step): audible as a rhythm glitch.
        // The audio-block quantum (10-46 ms) doesn't count — that's normal
        // delivery latency, not a broken rhythm.
        let step_late_limit = (self.state.sample_rate() * 0.125) as u64;
        // How many head notes we take in this frame: stale ones
        // are dropped, firing ones are limited by [`MAX_FIRES_PER_FRAME`].
        // `drain` instead of `remove(0)` — no shifting of the tail on every note.
        let mut take = 0usize;
        let mut fired = 0usize;
        while let Some((due, _, _)) = self.pending.get(take) {
            if *due > self.media_cursor {
                break;
            }
            let late = self.media_cursor.saturating_sub(*due) > late_limit;
            if !late && fired >= MAX_FIRES_PER_FRAME {
                break;
            }
            if !late {
                fired += 1;
            }
            take += 1;
        }
        if take > 0 {
            for (due, _, note) in self.pending.drain(..take) {
                // Stale ones we drop: playing long-past events is lying about the rhythm.
                let lateness = self.media_cursor.saturating_sub(due);
                if lateness <= late_limit {
                    self.state.push_note(&note);
                    // A delay longer than a quarter of a step is audible: the note
                    // landed off its beat. We count it separately from
                    // "stale" ones (which we drop) — this is rhythm diagnostics.
                    if lateness > step_late_limit {
                        self.late_notes += 1;
                    }
                } else {
                    self.late_notes += 1;
                }
            }
        }
        self.media_cursor += 1;
        let target = if self.is_paused.load(Ordering::Relaxed) {
            0.0
        } else {
            1.0
        };
        if self.pause_gain < target {
            self.pause_gain = (self.pause_gain + self.pause_step).min(target);
        } else if self.pause_gain > target {
            self.pause_gain = (self.pause_gain - self.pause_step).max(target);
        }
        let (l, r) = self
            .state
            .render_sample(&self.volumes[..self.volume_count], self.treble);
        (l * self.pause_gain, r * self.pause_gain)
    }

    /// Filling a callback block. Full pause: silence, DSP frozen.
    pub(crate) fn fill<T>(&mut self, data: &mut [T], channels: usize, conv: fn(f32) -> T) {
        let target = if self.is_paused.load(Ordering::Relaxed) {
            0.0
        } else {
            1.0
        };
        // We count frames in both branches: for diagnostics it matters to know
        // the callback is called at all, even in the silence of a pause.
        let frames_in_block = data.len() / channels.max(1);
        if !self.logged_first {
            self.logged_first = true;
            eprintln!("[engine] first callback: {frames_in_block} frames/block");
        }
        if target == 0.0 && self.pause_gain <= 0.0 {
            for sample in data.iter_mut() {
                *sample = conv(0.0);
            }
            // In a dead pause the schedule is reset: leaving it means
            // new notes "now", not a catch-up volley from the old plan.
            self.pending.clear();
            self.anchored = false;
            self.counters
                .frames
                .fetch_add(frames_in_block as u64, Ordering::Relaxed);
            return;
        }
        self.begin_block();
        // Seed or style change: old generation is cut off — voices and
        // the room tail fade out rather than playing over the new one.
        if self.voice_reset.swap(false, Ordering::Relaxed) {
            self.state.reset_voices();
        }
        for frame in data.chunks_mut(channels) {
            let (l, r) = self.next_frame();
            if channels == 1 {
                frame[0] = conv((l + r) * 0.5);
            } else {
                for (i, sample) in frame.iter_mut().enumerate() {
                    *sample = conv(if i.is_multiple_of(2) { l } else { r });
                }
            }
        }
        self.counters
            .frames
            .fetch_add(frames_in_block as u64, Ordering::Relaxed);
    }
}

pub(crate) fn f32_identity(x: f32) -> f32 {
    x
}

/// Mapping of filter openness onto a fraction of the high-cut: 0 → 1/8 of base
/// (dull intro), 1 → the style's full cut (opened-up drop).
/// Pure function — testable without audio.
fn openness_gain(open: f32) -> f32 {
    0.125 + 0.875 * open.clamp(0.0, 1.0)
}

#[cfg(test)]
fn f32_to_i16(x: f32) -> i16 {
    (x.clamp(-1.0, 1.0) * 32767.0) as i16
}

/// TPDF dither: two independent uniform noises in [-0.5, 0.5] LSB.
/// Without dither a quiet tail/reverb quantizes into steps and audibly
/// becomes "8-bit" (especially noticeable on decay).
/// The generator is splitmix64 from a fixed seed: the audio path
/// is deterministic, no rand/SystemTime.
static DITHER_STATE: AtomicU64 = AtomicU64::new(0x9e37_79b9_7f4a_7c15);

#[inline]
fn dither_lsb() -> f32 {
    let mut old = DITHER_STATE.load(Ordering::Relaxed);
    let next = old
        .wrapping_add(0x9e37_79b9_7f4a_7c15)
        .wrapping_mul(0xbf58_476d_1ce4_e5b9);
    loop {
        match DITHER_STATE.compare_exchange_weak(old, next, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => break,
            Err(v) => {
                old = v;
            }
        }
    }
    let mut z = next;
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^= z >> 31;
    ((z >> 11) as f32 * (1.0 / ((1u64 << 53) as f32))) - 0.5
}

fn f32_to_i16_dither(x: f32) -> i16 {
    let d = dither_lsb() + dither_lsb();
    let y = x.clamp(-1.0, 1.0) * 32767.0 + d;
    y.round().clamp(-32768.0, 32767.0) as i16
}

#[cfg(test)]
fn f32_to_u16(x: f32) -> u16 {
    // −1 → 0, 0 → 32768, +1 → 65535: unsigned PCM without bias
    // (the old formula `* 32767 + 32768` gave −1 → 1).
    ((x.clamp(-1.0, 1.0) + 1.0) * 32767.5).round() as u16
}

fn f32_to_u16_dither(x: f32) -> u16 {
    let d = dither_lsb() + dither_lsb();
    let y = (x.clamp(-1.0, 1.0) + 1.0) * 32767.5 + d;
    y.round().clamp(0.0, 65535.0) as u16
}

/// Config priority: F32 stereo closer to 48 kHz — first.
/// Pure function on ordinary values (testable without audio).
fn config_score(sample_format: SampleFormat, channels: u16, max_rate: u32) -> (u8, u16, u32) {
    let fmt = match sample_format {
        SampleFormat::F32 => 0,
        SampleFormat::I16 => 1,
        SampleFormat::U16 => 2,
        _ => 3,
    };
    let ch = if channels == 2 { 0 } else { 1 };
    let rate_dist = (max_rate as i32 - 48_000).unsigned_abs();
    (fmt, ch, rate_dist)
}

/// Output config choice: F32 stereo preferred, otherwise the first
/// supported format (conversion is in `Pump::fill`).
fn pick_output_config(device: &cpal::Device) -> Result<cpal::SupportedStreamConfig, String> {
    // Candidates: first the device default (as the working v1 does),
    // then everything enumerated by priority. getMinBufferSize lies on some
    // phones (F32 is listed but Oboe won't open it), so the winner is the first
    // config that ACTUALLY opens, rather than the first
    // in the list.
    let mut candidates: Vec<cpal::SupportedStreamConfig> = Vec::new();
    let mut seen: Vec<(u8, u16, u32)> = Vec::new();
    let mut push_unique = |c: cpal::SupportedStreamConfig| {
        let key = (
            match c.sample_format() {
                SampleFormat::F32 => 0,
                SampleFormat::I16 => 1,
                SampleFormat::U16 => 2,
                _ => 3,
            },
            c.channels(),
            c.sample_rate().0,
        );
        if !seen.contains(&key) {
            seen.push(key);
            candidates.push(c);
        }
    };
    if let Ok(def) = device.default_output_config() {
        push_unique(def);
    }
    if let Ok(ranges) = device.supported_output_configs() {
        let mut rest: Vec<_> = ranges.collect();
        rest.sort_by_key(|r| config_score(r.sample_format(), r.channels(), r.max_sample_rate().0));
        for r in rest {
            let min = r.min_sample_rate().0;
            let max = r.max_sample_rate().0;
            push_unique(r.with_sample_rate(cpal::SampleRate(max.min(48_000).max(min))));
        }
    }
    candidates
        .into_iter()
        .find(|c| probe_output(device, c))
        .ok_or_else(|| "no supported output config (all rejected by device)".to_string())
}

/// Dry run: checks that the config is actually accepted by Oboe.
/// A silent callback, the stream is dropped immediately.
fn probe_output(device: &cpal::Device, config: &cpal::SupportedStreamConfig) -> bool {
    let cfg: cpal::StreamConfig = config.config();
    let ok = match config.sample_format() {
        SampleFormat::F32 => device
            .build_output_stream(&cfg, |_: &mut [f32], _| {}, |_| {}, None)
            .is_ok(),
        SampleFormat::I16 => device
            .build_output_stream(&cfg, |_: &mut [i16], _| {}, |_| {}, None)
            .is_ok(),
        SampleFormat::U16 => device
            .build_output_stream(&cfg, |_: &mut [u16], _| {}, |_| {}, None)
            .is_ok(),
        _ => false,
    };
    if !ok {
        eprintln!(
            "[engine] config rejected: {} Hz, {:?}, {}ch",
            cfg.sample_rate.0,
            config.sample_format(),
            cfg.channels
        );
    }
    ok
}

pub struct AudioEngine {
    queue: Arc<ArrayQueue<NoteEvent>>,
    /// Step stamps: the pump builds the exact time grid from them.
    marks: Arc<ArrayQueue<crate::composer::StepMark>>,
    instruments: Vec<Box<dyn Instrument>>,
    mixer: Arc<RwLock<MixerState>>,
    stop: Arc<AtomicBool>,
    is_paused: Arc<AtomicBool>,
    fx: SharedFx,
    bpm: Arc<RwLock<f32>>,
    counters: EngineCounters,
    audio_config: Arc<RwLock<String>>,
    filter_open: Arc<RwLock<f32>>,
    /// "Reset voices" flag: a seed or style change cuts off the old
    /// generation (tails and reverb) — as the listener requested.
    voice_reset: Arc<AtomicBool>,
}

impl AudioEngine {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        queue: Arc<ArrayQueue<NoteEvent>>,
        marks: Arc<ArrayQueue<crate::composer::StepMark>>,
        instruments: Vec<Box<dyn Instrument>>,
        mixer: Arc<RwLock<MixerState>>,
        stop: Arc<AtomicBool>,
        is_paused: Arc<AtomicBool>,
        fx: SharedFx,
        bpm: Arc<RwLock<f32>>,
        counters: EngineCounters,
        audio_config: Arc<RwLock<String>>,
        filter_open: Arc<RwLock<f32>>,
        voice_reset: Arc<AtomicBool>,
    ) -> Self {
        Self {
            queue,
            marks,
            instruments,
            mixer,
            stop,
            is_paused,
            fx,
            bpm,
            counters,
            audio_config,
            filter_open,
            voice_reset,
        }
    }

    pub fn run(self) -> Result<(), Box<dyn std::error::Error>> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or("Audio device not found")?;
        let counters = self.counters.clone();

        // The pump lives separately from the stream (shared): when the stream is
        // recreated (headphones plugged/unplugged, a call, a driver hiccup) the voices,
        // sidechain and fades don't tear — only the output changes.
        // The mutex has no contention here: the engine touches the pump only
        // between streams, the callback — only inside a stream.
        let pump = Arc::new(Mutex::new(Pump::new(
            self.queue,
            self.marks,
            self.instruments,
            self.mixer,
            self.is_paused,
            self.fx,
            self.bpm,
            44_100.0,
            counters.clone(),
            self.filter_open,
            self.voice_reset,
        )));
        // Break flag: set by cpal's error callback (headphones unplugged —
        // Oboe tears down the stream). The watchdog below also catches silent stalls.
        let broken = Arc::new(AtomicBool::new(false));

        let mut first = true;
        loop {
            if self.stop.load(Ordering::Relaxed) {
                return Ok(());
            }
            let config = match pick_output_config(&device) {
                Ok(c) => c,
                Err(e) => {
                    // Startup failure — an honest error out (a red
                    // banner), not endless retries on a dead device.
                    if first {
                        return Err(format!("Output format unsuitable: {e}").into());
                    }
                    eprintln!("[engine] no output config, retrying: {e}");
                    std::thread::sleep(Duration::from_secs(2));
                    continue;
                }
            };
            let sample_rate = config.sample_rate().0 as f32;
            let channels = config.channels() as usize;
            pump.lock()
                .unwrap_or_else(|e| e.into_inner())
                .retune(sample_rate);

            // The choice's description — into stats (the banner shows which config
            // the engine took) and into logcat for deeper debugging.
            let desc = format!(
                "{} Hz, {:?}, {}ch",
                sample_rate as u32,
                config.sample_format(),
                channels
            );
            if let Ok(mut g) = self.audio_config.write() {
                *g = desc.clone();
            }
            eprintln!("[engine] output: {desc}");

            let broken_flag = Arc::clone(&broken);
            broken.store(false, Ordering::Relaxed);
            // In the callback — a blocking lock, not try_lock with a silence
            // fill: the engine only takes the pump during retune, before creating
            // the new stream (the old one is already stopped by then),
            // so there's no real contention. try_lock on a rare overlap blanked
            // a whole block into silence — worse than a short wait; the invariant
            // keeps the stream of sound continuous.
            let stream = match config.sample_format() {
                SampleFormat::F32 => {
                    let pump = Arc::clone(&pump);
                    device.build_output_stream(
                        &config.into(),
                        move |data: &mut [f32], _| {
                            pump.lock().unwrap_or_else(|e| e.into_inner()).fill(
                                data,
                                channels,
                                f32_identity,
                            );
                        },
                        move |err| {
                            eprintln!("[Engine Error] {err}");
                            broken_flag.store(true, Ordering::Relaxed);
                        },
                        None,
                    )
                }
                SampleFormat::I16 => {
                    let pump = Arc::clone(&pump);
                    device.build_output_stream(
                        &config.into(),
                        move |data: &mut [i16], _| {
                            pump.lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .fill(data, channels, f32_to_i16_dither);
                        },
                        move |err| {
                            eprintln!("[Engine Error] {err}");
                            broken_flag.store(true, Ordering::Relaxed);
                        },
                        None,
                    )
                }
                SampleFormat::U16 => {
                    let pump = Arc::clone(&pump);
                    device.build_output_stream(
                        &config.into(),
                        move |data: &mut [u16], _| {
                            pump.lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .fill(data, channels, f32_to_u16_dither);
                        },
                        move |err| {
                            eprintln!("[Engine Error] {err}");
                            broken_flag.store(true, Ordering::Relaxed);
                        },
                        None,
                    )
                }
                fmt => {
                    if first {
                        return Err(format!("unsupported output sample format: {fmt:?}").into());
                    }
                    eprintln!("[engine] unsupported format {fmt:?}, retrying");
                    std::thread::sleep(Duration::from_secs(2));
                    continue;
                }
            };
            let stream = match stream {
                Ok(s) => s,
                Err(e) => {
                    if first {
                        return Err(e.into());
                    }
                    eprintln!("[engine] build failed, retrying: {e}");
                    std::thread::sleep(Duration::from_secs(2));
                    continue;
                }
            };
            if let Err(e) = stream.play() {
                drop(stream);
                if first {
                    return Err(e.into());
                }
                eprintln!("[engine] play failed, retrying: {e}");
                std::thread::sleep(Duration::from_secs(2));
                continue;
            }
            first = false;

            // Watchdog: a break without an error (callbacks silently stopped) is caught
            // by the stalled frame counter. In a pause callbacks still come
            // (silence is counted), so there are no false positives.
            let mut last_frames = counters.frames.load(Ordering::Relaxed);
            let mut last_tick = Instant::now();
            loop {
                std::thread::sleep(Duration::from_millis(200));
                if self.stop.load(Ordering::Relaxed) {
                    return Ok(());
                }
                let frames = counters.frames.load(Ordering::Relaxed);
                if frames != last_frames {
                    last_frames = frames;
                    last_tick = Instant::now();
                }
                if broken.load(Ordering::Relaxed)
                    || last_tick.elapsed() > Duration::from_millis(1500)
                {
                    eprintln!("[engine] stream lost, rebuilding");
                    counters.restarts.fetch_add(1, Ordering::Relaxed);
                    break;
                }
            }
            // A pause before reopening: let the route switch over.
            std::thread::sleep(Duration::from_millis(500));
        }
    }
}

/// Sidechain depth by role: the bass pumps hardest,
/// the kick is never ducked. `mult` — the style's depth (rock pushes,
/// lofi breathes). Pure function — testable without audio.
pub(crate) fn duck_gain(role: InstrumentRole, kick_env: f32, mult: f32) -> f32 {
    let depth = match role {
        InstrumentRole::Bass => 0.6,
        InstrumentRole::Chords => 0.5,
        InstrumentRole::Lead => 0.4,
        InstrumentRole::Hat => 0.25,
        InstrumentRole::Snare => 0.12,
        InstrumentRole::Texture => 0.0,
        InstrumentRole::Kick => 0.0,
    };
    1.0 - mult.clamp(0.0, 2.0) * depth * (kick_env * DUCK_ENV_SCALE).min(1.0)
}

/// Master processing: soft saturation instead of hard clipping.
/// Never exceeds ±1, barely touches quiet signals.
pub(crate) fn master_sample(mixed: f32) -> f32 {
    // A 2048-point table instead of libm tanh per sample: interpolation
    // error <1e-5 (inaudible), cost — two reads.
    fast_tanh(mixed * MASTER_DRIVE)
}

/// Compatibility wrapper over [`AudioEngine::run`].
pub fn run_audio_engine(
    queue: Arc<ArrayQueue<NoteEvent>>,
    marks: Arc<ArrayQueue<crate::composer::StepMark>>,
    instruments: Vec<Box<dyn Instrument>>,
    mixer: Arc<RwLock<MixerState>>,
    stop: Arc<AtomicBool>,
    is_paused: Arc<AtomicBool>,
) -> Result<(), Box<dyn std::error::Error>> {
    AudioEngine::new(
        queue,
        marks,
        instruments,
        mixer,
        stop,
        is_paused,
        Arc::new(RwLock::new(MasterFx::default())),
        Arc::new(RwLock::new(120.0)),
        EngineCounters::default(),
        Arc::new(RwLock::new(String::new())),
        Arc::new(RwLock::new(0.4)),
        Arc::new(AtomicBool::new(false)),
    )
    .run()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duck_curves_pump_bass_not_kick() {
        use crate::instruments::InstrumentRole;
        // Silence — no ducking at all.
        for role in [
            InstrumentRole::Bass,
            InstrumentRole::Kick,
            InstrumentRole::Snare,
            InstrumentRole::Hat,
            InstrumentRole::Chords,
            InstrumentRole::Lead,
        ] {
            assert_eq!(duck_gain(role, 0.0, 1.0), 1.0);
        }
        // A kick hit: the bass ducks hardest, the kick is untouched.
        let bass = duck_gain(InstrumentRole::Bass, 1.0, 1.0);
        let chords = duck_gain(InstrumentRole::Chords, 1.0, 1.0);
        let hat = duck_gain(InstrumentRole::Hat, 1.0, 1.0);
        assert!(bass < 0.5, "bass must pump: {bass}");
        assert!(bass < chords && chords < hat, "depth order broken");
        assert_eq!(duck_gain(InstrumentRole::Kick, 1.0, 1.0), 1.0);
        // Monotonicity: a stronger kick — a deeper dip.
        assert!(
            duck_gain(InstrumentRole::Bass, 1.0, 1.0) < duck_gain(InstrumentRole::Bass, 0.3, 1.0)
        );
    }

    #[test]
    fn duck_mult_scales_style_pump() {
        use crate::instruments::InstrumentRole;
        // Zero — no sidechain at all (lofi breathes evenly).
        assert_eq!(duck_gain(InstrumentRole::Bass, 1.0, 0.0), 1.0);
        // More — deeper dip (rock pushes).
        let soft = duck_gain(InstrumentRole::Bass, 1.0, 0.5);
        let hard = duck_gain(InstrumentRole::Bass, 1.0, 1.5);
        assert!(hard < soft, "mult must deepen: {hard} vs {soft}");
        // The kick is never ducked at any multiplier.
        assert_eq!(duck_gain(InstrumentRole::Kick, 1.0, 2.0), 1.0);
    }

    #[test]
    fn master_never_clips_and_preserves_quiet() {
        assert!(master_sample(10.0) <= 1.0);
        assert!(master_sample(-10.0) >= -1.0);
        // A quiet signal is barely touched (tanh's linear region).
        let quiet = master_sample(0.1);
        assert!((quiet - 0.1 * MASTER_DRIVE).abs() < 1e-3);
        // A loud mix is compressed, not cut: monotonicity is preserved.
        assert!(master_sample(3.0) > master_sample(1.5));
    }

    #[test]
    fn config_score_prefers_f32_stereo_48k() {
        let best = config_score(SampleFormat::F32, 2, 48_000);
        assert!(config_score(SampleFormat::I16, 2, 48_000) > best);
        assert!(config_score(SampleFormat::F32, 1, 48_000) > best);
        assert!(config_score(SampleFormat::F32, 2, 44_100) > best);
        assert!(
            config_score(SampleFormat::U16, 2, 48_000) > config_score(SampleFormat::I16, 2, 48_000)
        );
    }

    #[test]
    fn openness_spans_eighth_to_full() {
        assert!((openness_gain(0.0) - 0.125).abs() < 1e-6);
        assert!((openness_gain(1.0) - 1.0).abs() < 1e-6);
        assert!((openness_gain(0.4) - 0.475).abs() < 1e-6);
        assert_eq!(openness_gain(-2.0), openness_gain(0.0));
        assert_eq!(openness_gain(99.0), openness_gain(1.0));
    }

    #[test]
    fn tpdf_dither_breaks_quantization_staircase() {
        // A very quiet signal without dither quantizes to one
        // repeated value; with TPDF dither the low bit "breathes" while staying
        // within ±1 LSB of the ideal.
        let vals: Vec<i16> = (0..4000).map(|_| f32_to_i16_dither(0.00002)).collect();
        let min = *vals.iter().min().unwrap();
        let max = *vals.iter().max().unwrap();
        assert!(max - min >= 1, "dither did nothing: {min}..{max}");
        assert!(min >= -2 && max <= 2, "dither too loud: {min}..{max}");
        // Full scale isn't broken: +1 is still 32767.
        assert!((f32_to_i16_dither(1.0) as i32) <= 32767);
        assert!((f32_to_i16_dither(-1.0) as i32) >= -32767);
    }

    #[test]
    fn volume_lookup_is_indexed_by_id() {
        let mut state = RenderState::new(Vec::new(), 44_100.0);
        state.sync_volumes(&[(2, 0.3), (14, 0.7)]);
        assert_eq!(RenderState::vol_of(&state.vol_cache, 2), 0.3);
        assert_eq!(RenderState::vol_of(&state.vol_cache, 14), 0.7);
        assert_eq!(RenderState::vol_of(&state.vol_cache, 7), 0.0);
        // Repeating the same values doesn't rebuild the table (same key).
        let key = state.vol_key;
        state.sync_volumes(&[(2, 0.3), (14, 0.7)]);
        assert_eq!(state.vol_key, key);
        state.sync_volumes(&[(2, 0.5)]);
        assert_eq!(RenderState::vol_of(&state.vol_cache, 2), 0.5);
        assert_eq!(RenderState::vol_of(&state.vol_cache, 14), 0.0);
    }

    #[test]
    fn sample_conversions_span_full_scale() {
        assert_eq!(f32_to_i16(0.0), 0);
        assert_eq!(f32_to_i16(1.0), 32767);
        assert_eq!(f32_to_i16(-1.0), -32767);
        assert_eq!(f32_to_i16(10.0), 32767);
        assert_eq!(f32_to_u16(0.0), 32768);
        assert_eq!(f32_to_u16(1.0), 65535);
        assert_eq!(f32_to_u16(-1.0), 0);
        assert_eq!(f32_to_u16(-10.0), 0);
    }

    #[test]
    fn swing_grid_lands_on_accumulated_steps() {
        // The grid is more accurate than a ruler: alternating long/short steps
        // (swing) are summed, not multiplied by the current length.
        // Steps 0..4 with lengths 100/50/100/50 ms: notes must land
        // at 0/4410/6615/11025 frames (@44100), not along a ruler.
        use crossbeam::queue::ArrayQueue;
        let queue = Arc::new(ArrayQueue::new(128));
        let marks = Arc::new(ArrayQueue::new(64));
        let lens = [100_000u32, 50_000, 100_000, 50_000];
        for (i, len) in lens.iter().enumerate() {
            let mut n = NoteEvent::new(6, 60, 0.8, 0.2);
            n.due_step = i as u64;
            n.step_us = *len;
            queue.push(n).unwrap();
        }
        let mut pump = Pump::new(
            queue,
            marks,
            Vec::new(),
            Arc::new(RwLock::new(crate::mixer::MixerState::new())),
            Arc::new(AtomicBool::new(false)),
            Arc::new(RwLock::new(MasterFx::default())),
            Arc::new(RwLock::new(126.0)),
            44_100.0,
            EngineCounters::default(),
            Arc::new(RwLock::new(0.4)),
            Arc::new(AtomicBool::new(false)),
        );
        let mut block = vec![0.0f32; 600 * 2];
        pump.fill(&mut block, 2, f32_identity);
        let mut dues: Vec<u64> = pump.pending.iter().map(|(d, _, _)| *d).collect();
        dues.sort();
        // Step 0 fired in the block, the rest hang on exact frames.
        assert_eq!(dues, vec![4410, 6615, 11025], "swing grid off: {dues:?}");
    }

    #[test]
    fn stamped_notes_wait_for_due_frames() {
        // Due scheduling: stamped notes wait for their media frame,
        // rather than firing when the queue is drained. 10 notes of steps 100..109
        // at 0.1 s each (@44100: 4410 frames/step): the first subblock (600 frames)
        // must fire only step 100, the rest stay
        // in pending. Legacy without a stamp fires immediately (test below).
        use crossbeam::queue::ArrayQueue;
        let queue = Arc::new(ArrayQueue::new(128));
        let marks = Arc::new(ArrayQueue::new(64));
        for step in 100..110u64 {
            let mut n = NoteEvent::new(6, 60, 0.8, 0.2);
            n.due_step = step;
            n.step_us = 100_000;
            queue.push(n).unwrap();
        }
        let mut pump = Pump::new(
            queue,
            marks,
            Vec::new(),
            Arc::new(RwLock::new(crate::mixer::MixerState::new())),
            Arc::new(AtomicBool::new(false)),
            Arc::new(RwLock::new(MasterFx::default())),
            Arc::new(RwLock::new(126.0)),
            44_100.0,
            EngineCounters::default(),
            Arc::new(RwLock::new(0.4)),
            Arc::new(AtomicBool::new(false)),
        );
        let mut block = vec![0.0f32; 600 * 2];
        pump.fill(&mut block, 2, f32_identity);
        assert!(
            block.iter().all(|s| s.is_finite()),
            "non-finite scheduled render"
        );
        assert_eq!(
            pump.pending.len(),
            9,
            "due scheduling broken: pending={}",
            pump.pending.len()
        );
        // We run about 1 second: all 10 fire on their frames.
        for _ in 0..10 {
            let mut more = vec![0.0f32; 4410 * 2];
            pump.fill(&mut more, 2, f32_identity);
            assert!(more.iter().all(|s| s.is_finite()));
        }
        assert!(pump.pending.is_empty(), "stamped notes lost in pending");
    }

    #[test]
    fn step_marks_give_an_exact_time_grid() {
        // The time grid is built from step stamps: each step's length
        // is known exactly, even if the step has no notes. Without stamps a gap
        // was estimated linearly (from the current step's length) — with swing the
        // error accumulated 20-50 ms per step, and after a couple of seconds notes
        // were "late" by half a second: the rhythm stumbled, the grid re-anchored.
        use crossbeam::queue::ArrayQueue;
        let queue = Arc::new(ArrayQueue::new(64));
        let marks = Arc::new(ArrayQueue::new(64));
        // Swing: steps alternate 143 ms / 95 ms.
        let lens_us = [143_000u32, 95_000u32];
        let sr = 44_100.0f32;
        let frame_of = |us: u32| -> u64 { us as u64 * sr as u64 / 1_000_000 };
        for step in 0..8u64 {
            let us = lens_us[(step % 2) as usize];
            marks.push(crate::composer::StepMark { step, us }).unwrap();
            if step % 2 == 0 {
                // Notes only on even steps: the steps between them are empty,
                // and only the stamps know their length.
                let mut n = NoteEvent::new(2, 36, 0.9, 0.2);
                n.due_step = step;
                n.step_us = us;
                queue.push(n).unwrap();
            }
        }
        let mut pump = Pump::new(
            queue,
            marks,
            Vec::new(),
            Arc::new(RwLock::new(crate::mixer::MixerState::new())),
            Arc::new(AtomicBool::new(false)),
            Arc::new(RwLock::new(MasterFx::default())),
            Arc::new(RwLock::new(126.0)),
            44_100.0,
            EngineCounters::default(),
            Arc::new(RwLock::new(0.4)),
            Arc::new(AtomicBool::new(false)),
        );
        let mut block = vec![0.0f32; 600 * 2];
        pump.fill(&mut block, 2, f32_identity);
        let mut dues: Vec<u64> = pump.pending.iter().map(|(d, _, _)| *d).collect();
        dues.sort_unstable();
        // Steps 2, 4, 6 — exact frames from the stamps (step 0 sounded immediately).
        let want = vec![
            frame_of(lens_us[0]) + frame_of(lens_us[1]),
            2 * (frame_of(lens_us[0]) + frame_of(lens_us[1])),
            3 * (frame_of(lens_us[0]) + frame_of(lens_us[1])),
        ];
        assert_eq!(dues, want, "grid from stamps is not accurate");
        assert!(pump.reanchors == 0, "re-anchor on an empty grid");
    }

    #[test]
    fn live_reset_with_marks_reanchors_once() {
        // A live seed/style change: step numbering restarts at zero.
        // Stamps and notes arrive from the new scale — the grid is rebuilt, but
        // the schedule doesn't run tens of seconds ahead (otherwise silence),
        // and the new scale is counted accurately again.
        use crossbeam::queue::ArrayQueue;
        let queue = Arc::new(ArrayQueue::new(256));
        let marks = Arc::new(ArrayQueue::new(1024));
        let mut pump = Pump::new(
            queue.clone(),
            Arc::clone(&marks),
            Vec::new(),
            Arc::new(RwLock::new(crate::mixer::MixerState::new())),
            Arc::new(AtomicBool::new(false)),
            Arc::new(RwLock::new(MasterFx::default())),
            Arc::new(RwLock::new(126.0)),
            44_100.0,
            EngineCounters::default(),
            Arc::new(RwLock::new(0.4)),
            Arc::new(AtomicBool::new(false)),
        );
        let mut block = vec![0.0f32; 4410 * 2];
        // Old scale: 300 steps, notes on some of them.
        for step in 0..300u64 {
            marks
                .push(crate::composer::StepMark { step, us: 100_000 })
                .unwrap();
            if step % 4 == 0 {
                let mut n = NoteEvent::new(2, 36, 0.9, 0.2);
                n.due_step = step;
                n.step_us = 100_000;
                queue.push(n).unwrap();
            }
        }
        // We drain the queue in blocks, as the live pipeline does.
        for _ in 0..12 {
            pump.fill(&mut block, 2, f32_identity);
        }
        assert!(pump.anchor_step > 0, "grid didn't start");
        // New scale: steps start from zero again.
        for step in 0..8u64 {
            marks
                .push(crate::composer::StepMark { step, us: 100_000 })
                .unwrap();
        }
        let mut n = NoteEvent::new(2, 36, 0.9, 0.2);
        n.due_step = 4;
        n.step_us = 100_000;
        queue.push(n).unwrap();
        let before = pump.media_cursor;
        pump.fill(&mut block, 2, f32_identity);
        assert!(pump.reanchors <= 1, "re-anchors: {}", pump.reanchors);
        assert_eq!(pump.anchor_step, 0, "anchor wasn't rebuilt");
        // The new scale is counted accurately: step 4 is four steps from the anchor.
        let due = pump
            .pending
            .iter()
            .filter(|(_, _, n)| n.due_step == 4)
            .map(|(d, _, _)| *d)
            .min()
            .expect("note of step 4 in the schedule");
        let want = pump.anchor_frame + 4 * 4410;
        assert!(
            (due as i64 - want as i64).abs() <= 2,
            "new scale counted inaccurately: {due} vs {want}"
        );
        assert!(
            due < before + 4410 * 8,
            "schedule ran ahead: {due} vs {before}"
        );
    }

    #[test]
    fn notes_of_one_frame_keep_generation_order() {
        // Kick and bass in the same frame: the engine loads voices in
        // generation order (kick first — the foundation, so the kick's duck is ready).
        // Sorting by instrument id broke that order.
        use crossbeam::queue::ArrayQueue;
        let queue = Arc::new(ArrayQueue::new(8));
        let marks = Arc::new(ArrayQueue::new(64));
        // The first note sets the grid anchor.
        let mut first = NoteEvent::new(6, 60, 0.9, 0.2);
        first.due_step = 0;
        first.step_us = 100_000;
        queue.push(first).unwrap();
        // Kick and bass in the same frame: the engine loads voices in
        // generation order (kick first — the foundation, so the kick's duck is ready).
        // Sorting by instrument id broke that order.
        for (id, pitch) in [(2usize, 36u8), (1, 40)] {
            let mut n = NoteEvent::new(id, pitch, 0.9, 0.2);
            n.due_step = 1;
            n.step_us = 100_000;
            queue.push(n).unwrap();
        }
        let mut pump = Pump::new(
            Arc::clone(&queue),
            marks,
            Vec::new(),
            Arc::new(RwLock::new(crate::mixer::MixerState::new())),
            Arc::new(AtomicBool::new(false)),
            Arc::new(RwLock::new(MasterFx::default())),
            Arc::new(RwLock::new(126.0)),
            44_100.0,
            EngineCounters::default(),
            Arc::new(RwLock::new(0.4)),
            Arc::new(AtomicBool::new(false)),
        );
        let mut block = vec![0.0f32; 600 * 2];
        pump.fill(&mut block, 2, f32_identity);
        let ids: Vec<usize> = pump
            .pending
            .iter()
            .map(|(_, _, n)| n.instrument_id)
            .collect();
        assert_eq!(ids, vec![2, 1], "frame note order broken");
    }

    #[test]
    fn live_numbering_reset_clears_old_step_grid() {
        // A live seed/style restarts the form at due_step 0.
        // The old step grid must not be folded into the new anchor:
        // otherwise after a loop (256 steps) the schedule runs tens of
        // seconds ahead and silence sets in.
        use crossbeam::queue::ArrayQueue;
        let queue = Arc::new(ArrayQueue::new(8));
        let marks = Arc::new(ArrayQueue::new(64));
        let mut pump = Pump::new(
            Arc::clone(&queue),
            marks,
            Vec::new(),
            Arc::new(RwLock::new(crate::mixer::MixerState::new())),
            Arc::new(AtomicBool::new(false)),
            Arc::new(RwLock::new(MasterFx::default())),
            Arc::new(RwLock::new(126.0)),
            44_100.0,
            EngineCounters::default(),
            Arc::new(RwLock::new(0.4)),
            Arc::new(AtomicBool::new(false)),
        );
        let mut block = vec![0.0f32; 4410 * 2]; // one step is 100 ms
        let push = |step: u64| {
            let mut n = NoteEvent::new(6, 60, 0.8, 0.2);
            n.due_step = step;
            n.step_us = 100_000;
            queue.push(n).unwrap();
        };
        // A long session: the ring overflows, the anchor moves ahead.
        for step in 0..300u64 {
            push(step);
            pump.fill(&mut block, 2, f32_identity);
            assert!(
                pump.pending.is_empty(),
                "old timeline stalled at step {step}: pending={}",
                pump.pending.len()
            );
        }
        assert!(pump.anchor_step > 0, "grid never folded");
        // Numbering reset. Every step of the new form must sound
        // in its own block rather than piling up in pending.
        for step in 0..300u64 {
            push(step);
            pump.fill(&mut block, 2, f32_identity);
            assert!(
                pump.pending.is_empty(),
                "schedule drifted after live reset at new step {step}: pending={}",
                pump.pending.len()
            );
        }
    }

    #[test]
    fn sparse_notes_keep_the_shared_grid() {
        // Sparse notes (pads, form pauses) aren't re-anchored to "now"
        // just because the queue emptied: step 10 must sound
        // ten steps from the anchor, not at drain time.
        use crossbeam::queue::ArrayQueue;
        let queue = Arc::new(ArrayQueue::new(8));
        let marks = Arc::new(ArrayQueue::new(64));
        let mut pump = Pump::new(
            Arc::clone(&queue),
            marks,
            Vec::new(),
            Arc::new(RwLock::new(crate::mixer::MixerState::new())),
            Arc::new(AtomicBool::new(false)),
            Arc::new(RwLock::new(MasterFx::default())),
            Arc::new(RwLock::new(126.0)),
            44_100.0,
            EngineCounters::default(),
            Arc::new(RwLock::new(0.4)),
            Arc::new(AtomicBool::new(false)),
        );
        let mut block = vec![0.0f32; 4410 * 2];
        let mut n = NoteEvent::new(6, 60, 0.8, 0.2);
        n.due_step = 0;
        n.step_us = 100_000;
        queue.push(n).unwrap();
        pump.fill(&mut block, 2, f32_identity); // step 0 has finished sounding
        for _ in 0..4 {
            pump.fill(&mut block, 2, f32_identity); // four steps of silence
        }
        assert_eq!(pump.media_cursor, 5 * 4410);
        let mut n = NoteEvent::new(6, 64, 0.8, 0.2);
        n.due_step = 10;
        n.step_us = 100_000;
        queue.push(n).unwrap();
        pump.fill(&mut block, 2, f32_identity);
        assert_eq!(pump.pending.len(), 1, "sparse note lost");
        assert_eq!(
            pump.pending[0].0, 44_100,
            "sparse note re-anchored to now instead of its grid step"
        );
    }

    #[test]
    fn unstamped_notes_fire_immediately() {
        // Legacy without a stamp (tests, direct calls): draining = firing,
        // no schedule. 100 notes in one subblock hit
        // only the drain cap of 32.
        use crossbeam::queue::ArrayQueue;
        let queue = Arc::new(ArrayQueue::new(128));
        let marks = Arc::new(ArrayQueue::new(64));
        for i in 0..100 {
            queue
                .push(NoteEvent::new(6, 60 + (i % 12) as u8, 0.8, 0.2))
                .unwrap();
        }
        let mut pump = Pump::new(
            queue,
            marks,
            Vec::new(),
            Arc::new(RwLock::new(crate::mixer::MixerState::new())),
            Arc::new(AtomicBool::new(false)),
            Arc::new(RwLock::new(MasterFx::default())),
            Arc::new(RwLock::new(126.0)),
            44_100.0,
            EngineCounters::default(),
            Arc::new(RwLock::new(0.4)),
            Arc::new(AtomicBool::new(false)),
        );
        let mut block = vec![0.0f32; 600 * 2];
        pump.fill(&mut block, 2, f32_identity);
        assert!(pump.pending.is_empty(), "legacy notes scheduled?!");
    }

    #[test]
    fn full_ensemble_mix_stays_finite() {
        // A mini-mix of the whole ensemble through the same RenderState as live:
        // finiteness + a sane level.
        use crate::instruments::{
            BassGuitar, HiHat, Kick, Kick808, NoteEvent, SnareDrum, Vinyl, VoicePool,
        };
        use crate::playback::{
            BASS_ID, GUITAR_ID, HAT_ID, KEYS_ID, KICK808_ID, KICK_ID, PAD_ID, PLUCK_ID, SHAKER_ID,
            SNARE_ID, VINYL_ID,
        };
        let sr = 44_100.0;
        let insts: Vec<Box<dyn Instrument>> = vec![
            Box::new(BassGuitar::new(BASS_ID)),
            Box::new(Kick::new(KICK_ID)),
            Box::new(VoicePool::pads(PAD_ID, 6)),
            Box::new(SnareDrum::new(SNARE_ID)),
            Box::new(HiHat::new(HAT_ID)),
            Box::new(VoicePool::plucks(PLUCK_ID, 6)),
            Box::new(VoicePool::keys(KEYS_ID, 8)),
            Box::new(Kick808::new(KICK808_ID)),
            Box::new(Vinyl::new(VINYL_ID)),
            Box::new(VoicePool::guitars(GUITAR_ID, 6)),
            Box::new(crate::instruments::Shaker::new(SHAKER_ID)),
        ];
        let mut state = RenderState::new(insts, sr);
        let volumes: Vec<(usize, f32)> = crate::playback::ENSEMBLE_IDS
            .iter()
            .map(|&id| (id, 0.9))
            .collect();
        let notes = [
            (BASS_ID, 40, 0.9, 0.3),
            (KICK_ID, 36, 1.0, 0.1),
            (PAD_ID, 48, 0.6, 1.0),
            (SNARE_ID, 38, 0.8, 0.1),
            (HAT_ID, 42, 0.6, 0.05),
            (PLUCK_ID, 72, 0.7, 0.3),
            (KEYS_ID, 60, 0.6, 1.0),
            (KICK808_ID, 36, 1.0, 0.1),
            (VINYL_ID, 60, 0.5, 2.0),
            (GUITAR_ID, 64, 0.7, 1.0),
            (SHAKER_ID, 60, 0.5, 0.05),
        ];
        for (id, pitch, vel, dur) in notes {
            state.push_note(&NoteEvent::new(id, pitch, vel, dur));
        }
        let mut peak: f32 = 0.0;
        let mut sum_sq = 0.0;
        let n = (0.5 * sr) as usize;
        for _ in 0..n {
            let (l, r) = state.render_sample(&volumes, 19000.0);
            assert!(l.is_finite() && r.is_finite());
            assert!(l.abs() <= 1.0 && r.abs() <= 1.0);
            peak = peak.max(l.abs().max(r.abs()));
            sum_sq += l * l + r * r;
        }
        let rms = (sum_sq / (2 * n) as f32).sqrt();
        assert!(peak > 0.15, "mix too quiet: peak={peak}");
        assert!(rms > 0.02, "mix too quiet: rms={rms}");
    }

    #[test]
    fn treble_slew_converges_without_jump() {
        // The high-cut travels to its target over ~120 ms, not by a step: a section
        // boundary (form sweep) produces no click in the filter coefficient.
        use crate::instruments::{BassGuitar, Instrument};
        let sr = 44_100.0;
        let insts: Vec<Box<dyn Instrument>> = vec![Box::new(BassGuitar::new(1))];
        let mut state = RenderState::new(insts, sr);
        let vols = [(1usize, 0.9f32)];
        for _ in 0..100 {
            state.render_sample(&vols, 2000.0);
        }
        let before = state.treble_smooth;
        assert!((before - 2000.0).abs() < 1.0, "snap failed: {before}");
        state.render_sample(&vols, 19000.0);
        let jump = (state.treble_smooth - before).abs();
        assert!(
            jump < 100.0,
            "treble jumps: {before} -> {}",
            state.treble_smooth
        );
        for _ in 0..sr as usize {
            state.render_sample(&vols, 19000.0);
        }
        assert!(
            (state.treble_smooth - 19000.0).abs() < 50.0,
            "never converges: {}",
            state.treble_smooth
        );
    }
}
