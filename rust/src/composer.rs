//! Composition generator: a bar-by-bar loop that queries the style.
//!
//! The loop owns no musical decisions: the style provides guidelines,
//! the mixer provides active tracks, tempo provides the live BPM knob. All three change
//! on the fly without restarting the stream.
//!
//! The voice hierarchy is single and lives in [`crate::styles::render_style`]:
//! the theme's lead sounds at full level, the other melodic voices are support.
//! The composer doesn't pick a second leader: otherwise the theme and the balance of parts
//! would drift apart.

use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, RwLock,
};
use std::thread;
use std::time::{Duration, Instant};

use crossbeam::queue::ArrayQueue;
use rand::rngs::StdRng;
use rand::SeedableRng;

use crate::arranger::{Arranger, SongContext};
use crate::core::{base_step_secs, normalize_tempo, snapshot, swing_step_secs, MAX_ACTIVE_TRACKS};
use crate::instruments::{InstrumentRole, NoteEvent, Slot};
use crate::mixer::MixerState;
use crate::styles::{render_style, StepContext, Style};

/// Live tempo: `None` = style default, `Some(bpm)` = frontend knob.
pub type LiveTempo = Arc<RwLock<Option<f32>>>;

/// Live snapshot for the circular visualizer: section, energy,
/// the seed's motif and the position within it. Updated by the composer every step
/// (cheap: a small Copy bundle), polled by the UI.
#[derive(Clone, Debug, Default)]
pub struct VisualSnap {
    pub section: u8,
    pub energy: f32,
    pub motif: [u8; 16],
    /// Theme position at this step (index in `motif`).
    pub motif_pos: u8,
    /// Fraction of the bar 0..1 (wheel hand).
    pub step_phase: f32,
    /// Mask of sounding beat positions (bit = 16th note): instrument dots.
    /// Reset at the bar boundary.
    pub bar_hits: u16,
    /// Masks for the main voices (the same bit = the same beat):
    /// kick, snare+hat, bass, theme (lead/chords).
    /// The wheel places a voice's dot on its beats: a drum 6 times
    /// per bar — 6 dots of that voice around the circle.
    pub kick_hits: u16,
    pub snare_hits: u16,
    pub bass_hits: u16,
    pub theme_hits: u16,
    /// Current phrase (bit = phrase beat, phrase = 2 bars):
    /// the hand makes one revolution per phrase.
    /// 32 bits = 2×16 steps; built-in styles are 16-step.
    pub phrase_len: u8,
    pub phrase_pos: u8,
    /// Exact phrase phase 0..1 (whole steps + the current step's fraction from the clock):
    /// the wheel's anchor without a polling staircase.
    pub phrase_phase: f32,
    /// Wheel window phase 0..1 (window = 4 bars): the hand moves and
    /// a dot's "age" is computed from it. The window is wider than the phrase, so the trace doesn't
    /// break at the phrase boundary.
    pub window_phase: f32,
    /// Window notes by voice (bit = window beat): kick, snare+hat,
    /// bass, theme (lead/chords). These are real notes, not a plan:
    /// the wheel shows exactly what sounded, so the display and the sound
    /// cannot diverge.
    pub pkick: u64,
    pub psnare: u64,
    pub pbass: u64,
    pub ptheme: u64,
    /// Phrase beats where something sounded (bit = phrase beat).
    /// Reset at the start of a phrase.
    pub pplayed: u32,
}

/// Section number for the snapshot (declaration order of SongSection).
pub(crate) fn section_index(section: crate::arranger::SongSection) -> u8 {
    section as u8
}

/// Wheel window: how many steps dots are remembered for (4 bars = 64 steps).
/// Wider than a musical phrase (2 bars) — otherwise at the phrase boundary the masks
/// would reset and the radar would clear exactly when the hand arrives
/// at the top.
pub const WHEEL_WINDOW_STEPS: usize = 64;

/// Step stamp: the step number and its duration in microseconds.
///
/// The engine builds the time grid from the stamps. Notes aren't present on every
/// step (form pauses, sparse layers), but the grid is needed on every one:
/// gaps estimated linearly from the current step's length accumulate error
/// (with swing a long and a short step differ by 20-50 ms, and after
/// a couple of seconds notes are "late" by half a second — the rhythm stumbles and the grid
/// re-anchors). A stamp is the honest step length.
#[derive(Clone, Copy, Debug)]
pub struct StepMark {
    pub step: u64,
    pub us: u32,
}

/// Transition shadow: the next chunk that sings over the current groove
/// in the seam. The voice enters softly (volume grows toward the end of the seam), the groove
/// stays single — otherwise the join turns into two voices.
struct Shadow {
    arranger: Arranger,
    song: SongContext,
}

pub struct Composer {
    queue: Arc<ArrayQueue<NoteEvent>>,
    /// Step stamps (length of each step): the engine builds the time
    /// grid from them. Without them the grid is built from notes and gaps are estimated
    /// linearly — timing drifts (the rhythm stumbles).
    marks: Arc<ArrayQueue<StepMark>>,
    style: Arc<RwLock<Box<dyn Style>>>,
    mixer: Arc<RwLock<MixerState>>,
    stop: Arc<AtomicBool>,
    is_paused: Arc<AtomicBool>,
    /// Seed in shared ownership: Flutter changes it live,
    /// the loop picks up the change without restarting the stream.
    seed: Arc<RwLock<u64>>,
    tempo: LiveTempo,
    /// Frontend intensity knob (default 1.0): read every step.
    intensity: Arc<RwLock<f32>>,
    /// Each step the effective BPM is written here — the engine builds
    /// the tempo echo's time from it.
    bpm_out: Arc<RwLock<f32>>,
    /// Each step the form's filter openness (0..1) is written here —
    /// the engine sweeps the high-cut toward the drop.
    filter_open: Arc<RwLock<f32>>,
    /// Counter of notes pushed to the queue (diagnostics for UI).
    pushed: Arc<AtomicU64>,
    /// Notes that didn't fit in the queue (the generator outruns the engine).
    /// Nonzero counter = the rhythm has holes: beats go missing.
    dropped: Arc<AtomicU64>,
    /// Live snapshot for the visualizer (shared with the session).
    visual: Arc<RwLock<VisualSnap>>,
    /// "Cut old voices" flag: set on a seed or style change.
    voice_reset: Arc<AtomicBool>,
    // Generation state (former local variables of `run`):
    rng: StdRng,
    arranger: Arranger,
    song: SongContext,
    current_seed: u64,
    step_count: usize,
    last_style_name: String,
    /// The phrase for which note masks are collected (monotonic index) and
    /// the global step of its start. Masks are from the phrase's real notes:
    /// the wheel shows exactly what sounds.
    last_planned: usize,
    phrase_start: usize,
    /// Phrase note masks by voice: kick, percussion, bass, melody.
    plan: [u64; 4],
    /// Style score (needed by the transition shadow).
    score: crate::arranger::Score,
    /// Style arrangement: who plays in each section.
    layers: crate::piece::Layers,
    /// How the style glues chunks together.
    transition: crate::piece::Transition,
    /// Shadow: the next chunk's voice, which sings over the current groove
    /// in the seam. `None` — outside a seam.
    shadow: Option<Shadow>,
    /// Separate RNG for the shadow: a shared stream would shift the main voice's notes.
    shadow_rng: StdRng,
    /// Start moment of the current step: exact phase within the step
    /// for the wheel's anchor (no polling staircase anymore).
    step_t0: Instant,
    /// Last note of each slot: (pitch, the step on which it
    /// stops sounding). Needed for ties: repeating the same pitch while the
    /// note still sounds extends it rather than re-attacking.
    last_note: [Option<(u8, u64)>; Slot::COUNT],
}

/// A tie is possible only if the same pitch arrives, the previous note
/// still sounds, and a deterministic roll falls below the style's probability.
/// Different pitches aren't a tie: the mono voice will legato them itself.
fn should_tie(
    tie_repeats: f32,
    last: Option<(u8, u64)>,
    pitch: u8,
    due_now: u64,
    seed_hash: u64,
    slot_idx: usize,
) -> bool {
    let Some((prev_pitch, prev_end)) = last else {
        return false;
    };
    tie_repeats > 0.0
        && prev_pitch == pitch
        && prev_end > due_now
        && crate::styles::tie_chance(seed_hash, slot_idx, due_now as usize) < tie_repeats
}

impl Composer {
    pub fn new(
        queue: Arc<ArrayQueue<NoteEvent>>,
        style: Arc<RwLock<Box<dyn Style>>>,
        mixer: Arc<RwLock<MixerState>>,
        stop: Arc<AtomicBool>,
        is_paused: Arc<AtomicBool>,
        seed: u64,
    ) -> Self {
        Self::new_shared(
            queue,
            Arc::new(ArrayQueue::new(1024)),
            style,
            mixer,
            stop,
            is_paused,
            Arc::new(RwLock::new(seed)),
            Arc::new(RwLock::new(None)),
            Arc::new(RwLock::new(1.0)),
            Arc::new(RwLock::new(120.0)),
            Arc::new(RwLock::new(0.4)),
            Arc::new(AtomicU64::new(0)),
            Arc::new(AtomicU64::new(0)),
            Arc::new(RwLock::new(VisualSnap::default())),
            Arc::new(AtomicBool::new(false)),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_shared(
        queue: Arc<ArrayQueue<NoteEvent>>,
        marks: Arc<ArrayQueue<StepMark>>,
        style: Arc<RwLock<Box<dyn Style>>>,
        mixer: Arc<RwLock<MixerState>>,
        stop: Arc<AtomicBool>,
        is_paused: Arc<AtomicBool>,
        seed: Arc<RwLock<u64>>,
        tempo: LiveTempo,
        intensity: Arc<RwLock<f32>>,
        bpm_out: Arc<RwLock<f32>>,
        filter_open: Arc<RwLock<f32>>,
        pushed: Arc<AtomicU64>,
        dropped: Arc<AtomicU64>,
        visual: Arc<RwLock<VisualSnap>>,
        voice_reset: Arc<AtomicBool>,
    ) -> Self {
        let current_seed = snapshot(&seed);
        let (score, style_name, bpm, transition, layers) = match style.read() {
            Ok(g) => (
                g.arrangement_for_seed(current_seed),
                g.name().to_string(),
                g.bpm(),
                g.transition(),
                g.arrangement_layers(),
            ),
            Err(poisoned) => {
                let g = poisoned.into_inner();
                (
                    g.arrangement_for_seed(current_seed),
                    g.name().to_string(),
                    g.bpm(),
                    g.transition(),
                    g.arrangement_layers(),
                )
            }
        };
        // First chunk: the form is finite (30-60 s), further chunks go
        // back-to-back, and each next one is written in advance (the join is written).
        let plan =
            crate::piece::plan_piece(current_seed, bpm, score.cycle, 0, 0, transition, layers);
        let mut arranger = Arranger::with_plan(current_seed, bpm, score, plan, transition, layers);
        let song = arranger.advance_measure();
        Self {
            queue,
            marks,
            style,
            mixer,
            stop,
            is_paused,
            seed,
            tempo,
            intensity,
            bpm_out,
            filter_open,
            pushed,
            dropped,
            visual,
            voice_reset,
            rng: StdRng::seed_from_u64(current_seed),
            arranger,
            song,
            current_seed,
            step_count: 0,
            // The name is already the current style's: the first step must not be treated
            // as a "style change" and reset the freshly created form.
            last_style_name: style_name,
            last_planned: usize::MAX,
            phrase_start: 0,
            plan: [0u64; 4],
            score,
            layers,
            transition,
            shadow: None,
            shadow_rng: StdRng::seed_from_u64(current_seed ^ 0x5A4D),
            step_t0: Instant::now(),
            last_note: [None; Slot::COUNT],
        }
    }

    fn reset_form(&mut self, seed: u64) {
        // Old generation is cut off: voices and the room tail fade out,
        // rather than playing over the new one (the listener changed seed/style themselves —
        // an abrupt cut is appropriate).
        self.voice_reset.store(true, Ordering::Relaxed);
        // The score comes from the current style and seed: each seed picks
        // its own form variant, not just a theme within one grid.
        let (score, bpm, transition, layers) = match self.style.read() {
            Ok(g) => (
                g.arrangement_for_seed(seed),
                g.bpm(),
                g.transition(),
                g.arrangement_layers(),
            ),
            Err(poisoned) => {
                let g = poisoned.into_inner();
                (
                    g.arrangement_for_seed(seed),
                    g.bpm(),
                    g.transition(),
                    g.arrangement_layers(),
                )
            }
        };
        // A live change takes effect from the next bar: the remainder of the current chunk
        // is re-planned for the new seed/style, what has played is left alone.
        let bar = self.arranger.piece_bar();
        let piece = self.arranger.piece();
        self.current_seed = seed;
        self.rng = StdRng::seed_from_u64(seed);
        self.score = score;
        self.transition = transition;
        self.layers = layers;
        // The previous seam's shadow isn't carried into the new form.
        self.shadow = None;
        let plan = crate::piece::plan_piece(seed, bpm, score.cycle, piece, bar, transition, layers);
        self.arranger = Arranger::with_plan(seed, bpm, score, plan, transition, layers);
        self.song = self.arranger.advance_measure();
        self.step_count = 0;
        self.last_planned = usize::MAX;
        // Voices have been silenced by the engine: remembered notes no longer sound
        // and must not produce a tie across the style/seed boundary.
        self.last_note = [None; Slot::COUNT];
    }

    /// One generation step: notes + duration until the next step.
    /// Pure mechanics without sleep — used by both the loop and the offline renderer.
    pub fn next_step(&mut self) -> (Vec<NoteEvent>, f32) {
        // Step clock: the snapshot is written at its beginning — the exact phase
        // (fraction within the step) goes to the wheel along with the position.
        self.step_t0 = Instant::now();

        // Live seed change: recreate the RNG and the form,
        // without touching the stream or the queue.
        let live_seed = snapshot(&self.seed);
        if live_seed != self.current_seed {
            self.reset_form(live_seed);
        }

        // We read the style name with a short guard so the form reset
        // (&mut self) doesn't conflict with reading the style.
        let style_name = match self.style.read() {
            Ok(g) => g.name().to_string(),
            Err(poisoned) => poisoned.into_inner().name().to_string(),
        };
        if self.last_style_name != style_name {
            // A style change starts a new form from scratch.
            self.reset_form(self.current_seed);
            self.last_style_name = style_name;
        }

        // We read the mix after the resets: a live seed/style may have changed
        // the cast, and a stale slice of active tracks must not get into
        // the first step of the new form.
        let mut active_tracks = [0usize; MAX_ACTIVE_TRACKS];
        let active_count = match self.mixer.read() {
            Ok(state) => state.active_ids(&mut active_tracks),
            Err(poisoned) => poisoned.into_inner().active_ids(&mut active_tracks),
        };
        // active_ids already sorts — order is deterministic.
        // Tracks with volume 0 don't get here: muted slots
        // load neither generation nor DSP.
        let active = &active_tracks[..active_count];

        let guard = match self.style.read() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        let style = &**guard;

        let steps_per_measure = style.steps_per_measure();
        // Effective tempo: the frontend knob overrides the style default.
        // Validation and clamping follow the same path as UI and status
        // (core::normalize_tempo): a non-sounding BPM never goes out.
        let bpm = snapshot(&self.tempo)
            .and_then(normalize_tempo)
            .or_else(|| normalize_tempo(style.bpm()))
            .unwrap_or_else(|| style.bpm());
        if let Ok(mut share) = self.bpm_out.write() {
            *share = bpm;
        }
        let base_step_secs = base_step_secs(bpm, steps_per_measure);
        let step_in_measure = self.step_count % steps_per_measure;
        // One snapshot per step: the plan and the live render see the same intensity.
        let intensity = snapshot(&self.intensity).clamp(0.0, 2.0);

        if step_in_measure == 0 && self.step_count > 0 {
            self.song = self.arranger.advance_measure();
            // The shadow (next chunk) keeps in step with the main one: the same bars.
            if let Some(shadow) = self.shadow.as_mut() {
                shadow.song = shadow.arranger.advance_measure();
            }
        }
        // Filter openness — from the fresh section: the engine sweeps
        // the high-cut toward the drop every step.
        if let Ok(mut share) = self.filter_open.write() {
            // The form sets the base, the listener adds a constant offset.
            *share = (crate::arranger::section_openness(self.song.section, self.song.progress)
                + crate::core::tone_open_offset())
            .clamp(0.0, 1.0);
        }

        // Transition shadow: in the seam (chunk finale) the next chunk's voice
        // enters over the current groove — the join doesn't cut the phrase. The shadow
        // is built when the seam starts (the next chunk's plan is already
        // written by then), and sings only melodic groups:
        // the groove stays single.
        if self.shadow.is_none() && self.arranger.in_joint() {
            if let (Some(plan), crate::piece::Transition::Rolling { .. }) =
                (self.arranger.next_plan().cloned(), self.transition)
            {
                let mut shadow = self.arranger.shadow_of(plan);
                let song = shadow.build_context();
                self.shadow = Some(Shadow {
                    arranger: shadow,
                    song,
                });
                self.shadow_rng = StdRng::seed_from_u64(self.current_seed ^ 0x5A4D);
            }
        } else if !self.arranger.in_joint() {
            // The seam ended: the next chunk's voice now sings on its own
            // as the arranger — the shadow goes away so there aren't two voices.
            self.shadow = None;
        }

        // Wheel window: the last 64 steps (4 bars) of real notes.
        // Dots on the wheel are exactly what sounds: there is no separate pre-render
        // anymore, so display and sound cannot diverge.
        // The window is wider than the phrase (32 steps) on purpose: otherwise at the phrase boundary
        // the masks would reset — and the radar would clear exactly when the
        // hand arrives at the top.
        let window_pos = self.step_count % WHEEL_WINDOW_STEPS;
        if window_pos == 0 {
            self.plan = [0u64; 4];
        }

        let mut sctx = StepContext {
            step: self.step_count,
            step_in_measure,
            base_step_secs,
            active_tracks: active,
            song: &self.song,
            rng: &mut self.rng,
            intensity,
        };
        let mut notes = render_style(&**guard, &mut sctx);

        // Ties on mono layers: a repeat of the same pitch arriving while the note
        // still sounds doesn't re-attack but extends it. This makes the note's
        // length audible: otherwise the next onset cuts the bass short of a step.
        // Polyphony stacks voices by itself — ties are only needed for the bass.
        let due_now = self.step_count as u64;
        let guides = style.slot_guides();
        for note in notes.iter_mut() {
            let Some(slot) = Slot::from_id(note.instrument_id) else {
                continue;
            };
            let Some(guide) = guides.get(slot.index()) else {
                continue;
            };
            let gate = (note.duration_secs / base_step_secs).round().max(1.0) as u64;
            let mut end = due_now + gate;
            let prev = self.last_note[slot.index()];
            if slot == Slot::Bass
                && should_tie(
                    guide.tie_repeats,
                    prev,
                    note.midi_pitch,
                    due_now,
                    self.song.seed_hash,
                    slot.index(),
                )
            {
                note.tie = true;
                if let Some((_, prev_end)) = prev {
                    end = end.max(prev_end);
                }
            }
            self.last_note[slot.index()] = Some((note.midi_pitch, end));
        }

        // Transition shadow: the next chunk's voice enters over the
        // current groove. Only melodic groups (lead): the groove and
        // the pump stay single, otherwise the seam turns into two voices.
        // The shadow's volume grows toward the end of the seam — the entrance is heard as an event,
        // not as background from nowhere. It grows per step, not per bar:
        // the voice rises smoothly, without steps.
        let shadow_gain = self
            .arranger
            .joint_step_progress(steps_per_measure, step_in_measure)
            .powf(1.2)
            .clamp(0.0, 1.0);
        if shadow_gain > 0.0 {
            if let Some(shadow) = self.shadow.as_mut() {
                let mut shctx = StepContext {
                    step: self.step_count,
                    step_in_measure,
                    base_step_secs,
                    active_tracks: active,
                    song: &shadow.song,
                    rng: &mut self.shadow_rng,
                    intensity,
                };
                for mut n in render_style(&**guard, &mut shctx) {
                    let melodic = matches!(
                        Slot::from_id(n.instrument_id).map(Slot::role),
                        Some(InstrumentRole::Lead)
                    );
                    if !melodic {
                        continue;
                    }
                    n.velocity *= shadow_gain;
                    notes.push(n);
                }
            }
        }

        // Seed swing on top of the style: every seed has its own bounce.
        // We compute it BEFORE the snapshot: the duration is needed both for the stamps and for the
        // wheel's exact phrase phase.
        let swing = style.swing_ratio() * crate::styles::swing_var(self.song.seed_hash);
        let duration = swing_step_secs(base_step_secs, swing, step_in_measure);
        // Exact phrase phase: whole steps + the current step's fraction from the clock.
        let phrase_len = (2 * steps_per_measure).min(32);
        let phrase_pos = self.step_count.saturating_sub(self.phrase_start).min(31);
        let step_frac = (Instant::now()
            .saturating_duration_since(self.step_t0)
            .as_secs_f32()
            / duration.max(0.001))
        .clamp(0.0, 1.0);
        let phrase_phase = (phrase_pos as f32 + step_frac) / phrase_len.max(1) as f32;
        // Wheel window phase: the hand and a dot's "age" are computed from it.
        let window_phase = (window_pos as f32 + step_frac) / WHEEL_WINDOW_STEPS as f32;

        // Window masks — from real notes (including the shadow voice): the wheel
        // and the sound take the same source, so they cannot diverge.
        if !notes.is_empty() {
            let bit = 1u64 << window_pos.min(WHEEL_WINDOW_STEPS - 1);
            for n in notes.iter() {
                match Slot::from_id(n.instrument_id).map(Slot::role) {
                    Some(InstrumentRole::Kick) => self.plan[0] |= bit,
                    Some(InstrumentRole::Snare | InstrumentRole::Hat) => self.plan[1] |= bit,
                    Some(InstrumentRole::Bass) => self.plan[2] |= bit,
                    Some(InstrumentRole::Lead | InstrumentRole::Chords) => {
                        self.plan[3] |= bit
                    }
                    _ => {}
                }
            }
        }
        // Live snapshot for the visualizer: bar fraction + masks of sounding
        // beats for the main voices (bit = 16th note, reset at the bar
        // boundary) + the phrase plan (one hand revolution = phrase, dots placed immediately).
        // Theme position — the same formula as in `play_guide`: the first
        // half of the motif in the first bar of the phrase, the second — in the second.
        if let Ok(mut snap) = self.visual.write() {
            let motif_len = self.song.motif.len().max(1);
            let bar_in_phrase = self.song.measure_index.saturating_sub(1) % 2;
            let bar_span = (steps_per_measure / 2).max(1);
            let rot = self.song.stage % motif_len;
            let pos = (bar_in_phrase * bar_span + step_in_measure / 2 + rot) % motif_len;
            let active_motif = if self.song.use_alt_motif {
                self.song.motif_b
            } else {
                self.song.motif
            };
            let mut hits = snap.bar_hits;
            let mut kick = snap.kick_hits;
            let mut snare = snap.snare_hits;
            let mut bass = snap.bass_hits;
            let mut theme = snap.theme_hits;
            if step_in_measure == 0 {
                hits = 0;
                kick = 0;
                snare = 0;
                bass = 0;
                theme = 0;
            }
            if !notes.is_empty() {
                let bit = 1u16 << step_in_measure.min(15);
                hits |= bit;
                for n in notes.iter() {
                    match Slot::from_id(n.instrument_id).map(Slot::role) {
                        Some(InstrumentRole::Kick) => kick |= bit,
                        Some(InstrumentRole::Snare | InstrumentRole::Hat) => snare |= bit,
                        Some(InstrumentRole::Bass) => bass |= bit,
                        Some(InstrumentRole::Lead | InstrumentRole::Chords) => theme |= bit,
                        _ => {}
                    }
                }
            }
            // Phrase played: the bit is set when a step has finished sounding.
            // Reset at the start of the phrase.
            let mut played = snap.pplayed;
            if phrase_pos == 0 {
                played = 0;
            }
            if !notes.is_empty() {
                played |= 1u32 << phrase_pos.min(31);
            }
            *snap = VisualSnap {
                section: section_index(self.song.section),
                energy: self.song.energy,
                motif: active_motif,
                motif_pos: pos as u8,
                step_phase: step_in_measure as f32 / steps_per_measure.max(1) as f32,
                bar_hits: hits,
                kick_hits: kick,
                snare_hits: snare,
                bass_hits: bass,
                theme_hits: theme,
                phrase_len: phrase_len as u8,
                phrase_pos: phrase_pos as u8,
                phrase_phase,
                window_phase,
                pkick: self.plan[0],
                psnare: self.plan[1],
                pbass: self.plan[2],
                ptheme: self.plan[3],
                pplayed: played,
            };
        }

        self.step_count += 1;
        // Due stamp: a note knows its step and its duration —
        // the pump will fire it exactly at its media frame, no matter when it
        // drains the queue (the frontend no longer owns timing).
        let step_us = (duration.max(0.001) * 1_000_000.0) as u32;
        let due_step = self.step_count as u64 - 1;
        for note in notes.iter_mut() {
            note.due_step = due_step;
            note.step_us = step_us;
        }
        (notes, duration)
    }

    pub fn run(mut self) {
        let mut next_deadline = Instant::now();

        while !self.stop.load(Ordering::Relaxed) {
            if self.is_paused.load(Ordering::Relaxed) {
                thread::sleep(Duration::from_millis(100));
                // We shift the deadline so there's no "catch-up" queue of steps after a pause.
                next_deadline = Instant::now();
                continue;
            }

            let (notes, step_duration) = self.next_step();
            // A step stamp — always, even if the step has no notes: from it the engine
            // knows the honest step length and builds an exact time grid.
            // Without stamps the grid is built from notes, gaps are estimated
            // linearly, and timing drifts (the rhythm stumbles).
            let step_us = (step_duration.max(0.001) * 1_000_000.0) as u32;
            let _ = self.marks.push(StepMark {
                step: self.step_count.saturating_sub(1) as u64,
                us: step_us,
            });
            // Full queue: we drop the newest note, leaving older ones alone.
            // Counted separately: a silent drop means "the rhythm stumbled",
            // while the counter shows that the generator specifically is falling behind.
            let mut ok = 0u64;
            let mut lost = 0u64;
            for note in notes {
                if self.queue.push(note).is_ok() {
                    ok += 1;
                } else {
                    lost += 1;
                }
            }
            if ok > 0 {
                self.pushed.fetch_add(ok, Ordering::Relaxed);
            }
            if lost > 0 {
                self.dropped.fetch_add(lost, Ordering::Relaxed);
            }

            // Drift-free stepper: sleep until the deadline, don't accumulate lag.
            // The clamp protects against a Duration::from_secs_f32 panic with a broken
            // custom style (NaN/Inf/negative duration).
            let safe_step = step_duration.clamp(0.001, 4.0);
            next_deadline += Duration::from_secs_f32(safe_step);
            let now = Instant::now();
            if next_deadline > now {
                thread::sleep(next_deadline - now);
            } else {
                next_deadline = now;
            }
        }
    }
}

/// Compatibility wrapper over [`Composer::run`].
pub fn start_composing(
    queue: Arc<ArrayQueue<NoteEvent>>,
    style_lock: Arc<RwLock<Box<dyn Style>>>,
    mixer: Arc<RwLock<MixerState>>,
    stop: Arc<AtomicBool>,
    is_paused: Arc<AtomicBool>,
    seed: u64,
) {
    Composer::new(queue, style_lock, mixer, stop, is_paused, seed).run();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instruments::Slot;
    use crate::mixer::MixerState;

    #[test]
    fn bass_lengths_vary() {
        // Integration of lengths: the composer really gives the bass different
        // durations, not a single timbral constant.
        let style = Arc::new(RwLock::new(
            crate::styles::make_style("club").expect("club style"),
        ));
        let mixer = Arc::new(RwLock::new(MixerState::new()));
        {
            let guard = style.read().expect("style readable");
            let mut m = mixer.write().expect("mixer writable");
            crate::playback::apply_style_mix(&**guard, 424242, &mut m);
        }
        let queue = Arc::new(ArrayQueue::new(16_384));
        let mut composer = Composer::new(
            queue,
            style,
            mixer,
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
            424242,
        );
        let mut bass = 0usize;
        let mut lengths = std::collections::HashSet::new();
        for _ in 0..1600 {
            let (notes, _) = composer.next_step();
            for n in notes {
                if n.instrument_id == Slot::Bass.id() {
                    bass += 1;
                    lengths.insert((n.duration_secs * 1000.0).round() as i32);
                }
            }
        }
        assert!(bass > 100, "too few bass notes: {bass}");
        assert!(lengths.len() >= 3, "bass lengths did not vary: {lengths:?}");
    }

    #[test]
    fn tie_decision_needs_same_pitch_and_overlap() {
        let due = 100u64;
        assert!(should_tie(1.0, Some((40, 104)), 40, due, 7, 0));
        assert!(
            !should_tie(1.0, Some((41, 104)), 40, due, 7, 0),
            "other pitch is a legato, not a tie"
        );
        assert!(
            !should_tie(1.0, Some((40, 100)), 40, due, 7, 0),
            "previous note already ended"
        );
        assert!(
            !should_tie(0.0, Some((40, 104)), 40, due, 7, 0),
            "style forbids ties"
        );
        assert!(!should_tie(1.0, None, 40, due, 7, 0), "no previous note");
    }
}
