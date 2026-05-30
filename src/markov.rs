// Melodic Markov generator.
//
// Note selection weights (computed dynamically each step):
//   chord_bonus      - chord tone: weight x3.5
//   inertia (fading) - continue prevailing direction, decays after 3-4 steps
//   recency_penalty  - recently played notes are down-weighted
//   edge_penalty     - avoid camping at range extremes
//   tonic_pull       - last note of phrase steered toward tonic
//
// Rhythm:
//   A RhythmFigure is a fixed sequence of durations applied to consecutive notes.
//   A new figure is picked at each phrase boundary.
//   The composer sleeps grid_step_ms between onsets (not note duration),
//   so notes overlap when duration > grid_step - this creates legato.
//
// Motif memory:
//   Each completed phrase is saved. With motif_recall_prob the generator
//   replays it verbatim (same notes, current figure durations).
//   This creates the recognisable-theme / call-and-response effect.

use crate::instruments::{EnvelopeConfig, Instrument};
use crate::NoteEvent;
use rand::prelude::*;
use std::collections::VecDeque;

// ─────────────────────────────────────────────────────────────
//  SCALE
// ─────────────────────────────────────────────────────────────

pub struct Scale {
    pub root:      u8,
    pub intervals: &'static [u8],
}

impl Scale {
    pub fn tones_in_range(&self, min: u8, max: u8) -> Vec<u8> {
        let root_pc = self.root % 12;
        let mut out = Vec::new();
        let start = (min / 12).saturating_sub(1) * 12;
        let end   = (max / 12 + 2) * 12;
        for base in (start..end).step_by(12) {
            for &iv in self.intervals {
                let n = base + root_pc + iv;
                if n >= min && n <= max { out.push(n); }
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }
}

pub static PENTATONIC_MINOR:    Scale = Scale { root: 57, intervals: &[0,3,5,7,10] };
pub static NATURAL_MINOR:       Scale = Scale { root: 57, intervals: &[0,2,3,5,7,8,10] };
pub static MAJOR:               Scale = Scale { root: 60, intervals: &[0,2,4,5,7,9,11] };
pub static DORIAN:              Scale = Scale { root: 62, intervals: &[0,2,3,5,7,9,10] };
pub static CHROMATIC:           Scale = Scale { root: 60, intervals: &[0,1,2,3,4,5,6,7,8,9,10,11] };
pub static LYDIAN_FLOATING:     Scale = Scale { root: 60, intervals: &[0, 2, 4, 6, 7, 9, 11] };

// ─────────────────────────────────────────────────────────────
//  LAYER CONFIG
//
//  A preset has one or more layers. Each layer runs its own
//  MarkovGenerator sharing the preset's scale and chords, but
//  with its own note range, instrument, and rhythm role.
// ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RhythmRole {
    // Full Markov melody with motif memory
    Melody,
    // Only plays root tones of the current chord, slowly
    Bass,
    // Holds one chord tone for a long duration, changes rarely
    Pad,
    // Ignores pitch - plays a fixed note on a rhythmic pattern
    Percussion,
}

pub struct LayerConfig {
    pub instrument:  Instrument,
    pub role:        RhythmRole,
    pub note_min:    u8,
    pub note_max:    u8,
    // Velocity multiplier relative to the preset base (1.0 = normal)
    pub vel_scale:   f32,
    // Grid step multiplier (1.0 = same as preset, 2.0 = twice as slow)
    pub grid_mult:   f32,
    // For Percussion: the fixed MIDI note to play
    pub fixed_note:  u8,
    // For Percussion: pattern of grid steps between hits (true = hit, false = rest)
    pub beat_pattern: &'static [bool],
    // Optional ADSR override. None = use instrument default_envelope().
    pub envelope: Option<EnvelopeConfig>,
}

// ─────────────────────────────────────────────────────────────
//  CHORD
// ─────────────────────────────────────────────────────────────

pub struct Chord { pub root: u8, pub intervals: &'static [u8] }

impl Chord {
    pub fn contains(&self, note: u8) -> bool {
        let iv = (note % 12) as i16 - (self.root % 12) as i16;
        self.intervals.contains(&(iv.rem_euclid(12) as u8))
    }

    pub fn nearest_tone(&self, note: u8, min: u8, max: u8) -> u8 {
        let root_pc = self.root % 12;
        let mut best = note;
        let mut best_dist = u8::MAX;
        for oct in 2u8..8 {
            for &iv in self.intervals {
                let n = (oct * 12).saturating_add(root_pc).saturating_add(iv);
                if n < min || n > max { continue; }
                let dist = (n as i16 - note as i16).unsigned_abs() as u8;
                if dist < best_dist { best_dist = dist; best = n; }
            }
        }
        best
    }
}

static C_MAJ:  Chord = Chord { root: 60, intervals: &[0,4,7] };
static A_MIN:  Chord = Chord { root: 57, intervals: &[0,3,7] };
static F_MAJ:  Chord = Chord { root: 65, intervals: &[0,4,7] };
static G_MAJ:  Chord = Chord { root: 67, intervals: &[0,4,7] };
static G_DOM7: Chord = Chord { root: 67, intervals: &[0,4,7,10] };
static D_MIN7: Chord = Chord { root: 62, intervals: &[0,3,7,10] };
static G_MIN:  Chord = Chord { root: 67, intervals: &[0,3,7] };
static D_MIN:  Chord = Chord { root: 62, intervals: &[0,3,7] };
static E_MIN:  Chord = Chord { root: 64, intervals: &[0,3,7] };
static E_FLAT: Chord = Chord { root: 63, intervals: &[0,4,7] };
static B_FLAT: Chord = Chord { root: 70, intervals: &[0,4,7] };
static A_MAJ:  Chord = Chord { root: 69, intervals: &[0,4,7] };

// ─────────────────────────────────────────────────────────────
//  RHYTHM FIGURE
// ─────────────────────────────────────────────────────────────

pub struct RhythmFigure {
    pub durations: &'static [f32], // note durations in ms
    pub weight:    u32,
}

// ─────────────────────────────────────────────────────────────
//  PRESET
// ─────────────────────────────────────────────────────────────

pub struct MarkovPreset {
    pub name:  &'static str,
    pub scale: &'static Scale,
    pub note_min: u8,
    pub note_max: u8,

    // Harmonic progression - one chord per phrase, looped
    pub chords: &'static [&'static Chord],

    // Notes per phrase
    pub phrase_min: usize,
    pub phrase_max: usize,

    // Anti-repeat history depth
    pub history_len: usize,

    // Direction inertia [0..1] - fades automatically after 3-4 steps
    pub inertia: f32,

    // Max scale-degree step per note
    pub max_step: usize,

    // Rhythmic figures - one picked per phrase, cycled within it
    pub figures: &'static [RhythmFigure],

    // Time between note onsets (tempo grid)
    pub grid_step_ms: f32,

    // Velocity range
    pub vel_min: f32,
    pub vel_max: f32,

    // Velocity multiplier on the first note of each phrase
    pub accent: f32,

    // Rest between phrases
    pub rest_prob:  f64,
    pub rest_steps: u32,

    // Probability the last note of a phrase resolves toward tonic
    pub tonic_pull: f64,

    // Motif recall settings
    pub motif_recall_prob:  f64,
    pub motif_recall_after: usize,

    // Layers (instruments) for this preset
    pub layers: &'static [LayerConfig],
}

// ─────────────────────────────────────────────────────────────
//  GENERATOR
// ─────────────────────────────────────────────────────────────

pub struct MarkovGenerator {
    preset:              &'static MarkovPreset,
    rng:                 rand::rngs::ThreadRng,
    tones:               Vec<u8>,
    history:             VecDeque<u8>,
    direction:           i32,
    steps_in_dir:        usize,
    phrase_pos:          usize,
    phrase_len:          usize,
    chord_idx:           usize,
    current_vel:         f32,
    figure:              &'static [f32],
    figure_pos:          usize,
    motif:               Vec<u8>,
    motif_pos:           Option<usize>,
    phrases_since_motif: usize,
}

impl MarkovGenerator {
    pub fn new(preset: &'static MarkovPreset) -> Self {
        Self::new_with_range(preset, preset.note_min, preset.note_max)
    }

    // Create a generator with a custom note range (used by layer system).
    // All other parameters come from the preset.
    pub fn new_with_range(preset: &'static MarkovPreset, note_min: u8, note_max: u8) -> Self {
        let mut rng    = rand::rng();
        let tones      = preset.scale.tones_in_range(note_min, note_max);
        let start      = tones[tones.len() / 3];
        let phrase_len = rng.random_range(preset.phrase_min..=preset.phrase_max);
        let vel        = (preset.vel_min + preset.vel_max) / 2.0;
        let figure     = Self::pick_figure(preset, &mut rng);
        let mut hist   = VecDeque::with_capacity(preset.history_len + 1);
        hist.push_back(start);
        Self {
            preset, rng, tones,
            history: hist,
            direction: 1, steps_in_dir: 0,
            phrase_pos: 0, phrase_len,
            chord_idx: 0,
            current_vel: vel,
            figure, figure_pos: 0,
            motif: Vec::new(),
            motif_pos: None,
            phrases_since_motif: 0,
        }
    }

    pub fn grid_step_ms(&self) -> f32 { self.preset.grid_step_ms }

    // Returns Some(rest_ms) at phrase boundaries when a rest should occur.
    pub fn phrase_rest_ms(&mut self) -> Option<f32> {
        if self.phrase_pos == 0 && self.rng.random_bool(self.preset.rest_prob) {
            Some(self.preset.grid_step_ms * self.preset.rest_steps as f32)
        } else {
            None
        }
    }

    pub fn next(&mut self) -> NoteEvent {
        // ── phrase boundary ───────────────────────────────────
        if self.phrase_pos >= self.phrase_len {
            // Save phrase as motif candidate (only when generating freely)
            if self.motif_pos.is_none() && self.phrase_len >= 3 {
                let start = self.history.len().saturating_sub(self.phrase_len);
                self.motif = self.history.iter().skip(start).copied().collect();
            }

            self.phrase_pos = 0;
            self.chord_idx  = (self.chord_idx + 1) % self.preset.chords.len();
            self.figure     = Self::pick_figure(self.preset, &mut self.rng);
            self.figure_pos = 0;

            let can_recall = !self.motif.is_empty()
                && self.motif_pos.is_none()
                && self.phrases_since_motif >= self.preset.motif_recall_after;

            if can_recall && self.rng.random_bool(self.preset.motif_recall_prob) {
                self.motif_pos           = Some(0);
                self.phrase_len          = self.motif.len();
                self.phrases_since_motif = 0;
            } else {
                self.motif_pos  = None;
                self.phrase_len = self.rng.random_range(
                    self.preset.phrase_min..=self.preset.phrase_max
                );
                self.phrases_since_motif += 1;
            }
        }

        let is_last        = self.phrase_pos + 1 >= self.phrase_len;
        let is_penultimate = self.phrase_pos + 2 >= self.phrase_len && !is_last;
        let is_first       = self.phrase_pos == 0;

        // Replay motif or generate freely
        let note = if let Some(pos) = self.motif_pos {
            let n = self.motif.get(pos).copied().unwrap_or(60);
            self.motif_pos = if pos + 1 < self.motif.len() {
                Some(pos + 1)
            } else {
                None
            };
            n
        } else {
            self.pick_note(is_last, is_penultimate)
        };

        // Duration from current figure (cycles within phrase)
        let dur = self.figure[self.figure_pos % self.figure.len()];
        self.figure_pos += 1;

        // Velocity: gentle drift + accent on phrase downbeat
        let drift = self.rng.random_range(-0.018f32..0.018);
        self.current_vel = (self.current_vel + drift)
            .clamp(self.preset.vel_min, self.preset.vel_max);
        let vel = if is_first {
            (self.current_vel * self.preset.accent).clamp(0.0, 1.0)
        } else {
            self.current_vel
        };

        self.history.push_back(note);
        while self.history.len() > self.preset.history_len {
            self.history.pop_front();
        }
        self.phrase_pos += 1;

        NoteEvent {
            note,
            velocity: vel,
            duration: dur,
            instrument: crate::instruments::Instrument::Sine,
            envelope: crate::instruments::Instrument::Sine.default_envelope(),
        }
    }

    // ── private ────────────────────────────────────────────────

    fn pick_figure(preset: &'static MarkovPreset, rng: &mut impl Rng) -> &'static [f32] {
        let total: u32 = preset.figures.iter().map(|f| f.weight).sum();
        let mut r = rng.random_range(0..total);
        for fig in preset.figures {
            if r < fig.weight { return fig.durations; }
            r -= fig.weight;
        }
        preset.figures.last().map(|f| f.durations).unwrap_or(&[400.0])
    }

    fn pick_note(&mut self, is_last: bool, is_penultimate: bool) -> u8 {
        let prev       = *self.history.back().unwrap_or(&60);
        let chord      = self.preset.chords[self.chord_idx];
        let tonic      = self.preset.chords[0];
        // The chord that will sound on the NEXT phrase
        let next_chord = self.preset.chords[(self.chord_idx + 1) % self.preset.chords.len()];

        let cur_idx = self.tones.iter().position(|&n| n == prev)
            .unwrap_or(self.tones.len() / 2);

        // Auto-reverse at range edges after 2+ steps there
        let at_top = cur_idx + 1 >= self.tones.len();
        let at_bot = cur_idx == 0;
        if (at_top && self.direction > 0) || (at_bot && self.direction < 0) {
            if self.steps_in_dir >= 2 {
                self.direction    = -self.direction;
                self.steps_in_dir = 0;
            }
        }

        // Tonic resolution on last note
        if is_last && self.rng.random_bool(self.preset.tonic_pull) {
            let target = tonic.nearest_tone(prev, self.preset.note_min, self.preset.note_max);
            if target != prev { return target; }
        }

        // Tension note on penultimate position:
        // prefer tones that are IN the next chord but NOT in the current chord.
        // This creates a "leading" dissonance that resolves on the next phrase.
        // The effect: the last note of a phrase sounds like a question,
        // the first note of the next phrase sounds like the answer.
        let tension_mode = is_penultimate && self.rng.random_bool(self.preset.tonic_pull * 0.8);

        let max_step = self.preset.max_step;
        let lo = cur_idx.saturating_sub(max_step);
        let hi = (cur_idx + max_step).min(self.tones.len().saturating_sub(1));

        let mut candidates: Vec<(usize, f32)> = (lo..=hi)
            .filter(|&i| i != cur_idx)
            .map(|i| {
                let note = self.tones[i];
                let mut w = 1.0f32;

                // Chord tone bonus
                if chord.contains(note) { w *= 3.5; }

                // Tension bonus (penultimate note only):
                // boost tones that belong to the NEXT chord but not the current.
                // This pulls the melody toward the coming harmony before it arrives.
                if tension_mode {
                    let in_next    = next_chord.contains(note);
                    let in_current = chord.contains(note);
                    if in_next && !in_current {
                        w *= 2.8; // strong pull toward next chord
                    } else if in_current && !in_next {
                        w *= 0.4; // avoid settling into current chord
                    }
                }

                // Fading direction inertia
                let decay = 1.0 / (1.0 + self.steps_in_dir as f32 * 0.35);
                let eff   = self.preset.inertia * decay;
                let dir   = (i as i32 - cur_idx as i32).signum();
                if dir == self.direction {
                    w *= 1.0 + eff;
                } else {
                    w *= (1.0 - eff * 0.6).max(0.15);
                }

                // Recency penalty
                for (age, &h) in self.history.iter().rev().enumerate() {
                    if h == note {
                        let p = 0.90 / (1.0 + age as f32 * 0.4);
                        w *= 1.0 - p;
                        break;
                    }
                }

                // Edge penalty
                let dist = i.min(self.tones.len().saturating_sub(1) - i) as f32;
                if dist < 2.0 { w *= 0.4 + dist * 0.3; }

                (i, w.max(0.01))
            })
            .collect();

        if candidates.is_empty() {
            let mid = self.tones.len() / 2;
            return self.tones[if cur_idx < mid {
                (cur_idx + 1).min(self.tones.len() - 1)
            } else {
                cur_idx.saturating_sub(1)
            }];
        }

        let total: f32 = candidates.iter().map(|&(_, w)| w).sum();
        let mut r = self.rng.random_range(0.0f32..total);
        let mut chosen = candidates[0].0;
        for &(idx, w) in &candidates {
            if r < w { chosen = idx; break; }
            r -= w;
        }

        let moved = chosen as i32 - cur_idx as i32;
        if moved.signum() == self.direction {
            self.steps_in_dir += 1;
        } else if moved != 0 {
            self.direction    = moved.signum();
            self.steps_in_dir = 1;
        }

        self.tones[chosen]
    }
}

// ─────────────────────────────────────────────────────────────
//  PRESETS
//
//  grid_step_ms = time between note onsets (the tempo).
//  note duration > grid_step → overlap → legato.
//  note duration < grid_step → gap → staccato.
//
//  Chord bonus x3.5 means chord tones dominate but scale tones
//  still appear as passing notes - keeps melody tonal but not rigid.
// ─────────────────────────────────────────────────────────────

// AMBIENT - light, unobtrusive. Rare, long, soft notes.
// Pentatonic minor: no semitones, always consonant.
// Very slow onsets (500ms), long durations (1200-2400ms) → deep overlap.
// Low velocity, high rest probability → space between ideas.
pub static AMBIENT: MarkovPreset = MarkovPreset {
    name:     "Ambient",
    scale:    &LYDIAN_FLOATING,
    note_min: 36, 
    note_max: 84,
    chords:   &[&C_MAJ, &F_MAJ, &A_MIN, &G_MAJ],
    phrase_min: 4, 
    phrase_max: 8,
    history_len: 3,
    inertia:  0.4,
    max_step: 3,
    
    grid_step_ms: 2200.0, 
    figures: &[
        RhythmFigure { durations: &[5000.0, 7000.0, 6000.0], weight: 40 },
        RhythmFigure { durations: &[9000.0, 3500.0, 4500.0], weight: 35 },
        RhythmFigure { durations: &[4000.0, 8000.0], weight: 25 },
    ],

    vel_min: 0.15, 
    vel_max: 0.35,
    accent:  1.01,

    rest_prob: 0.3, 
    rest_steps: 2,
    
    tonic_pull: 0.15,
    motif_recall_prob:  0.35,
    motif_recall_after: 3,
    
    layers: &[
        LayerConfig {
            instrument:   Instrument::Sine,
            role:         RhythmRole::Bass,
            note_min:     52, 
            note_max:     64,
            vel_scale:    0.3,
            grid_mult:    2.0,
            fixed_note:   0, 
            beat_pattern: &[true],
            envelope:     Some(EnvelopeConfig::new(3000.0, 1000.0, 1.0, 5000.0)),
        },

        LayerConfig {
            instrument:   Instrument::Pad,
            role:         RhythmRole::Pad,
            note_min:     48, 
            note_max:     67,
            vel_scale:    0.3, 
            grid_mult:    1.0, 
            fixed_note:   0, 
            beat_pattern: &[true],
            envelope:     Some(EnvelopeConfig::new(4000.0, 2000.0, 0.8, 6000.0)),
        },
    ],
};

// JAZZ - steady pulse, constant tempo, rich harmonic range.
// Dorian with ii-V-I. Swing figures (long-short pairs).
// Moderate inertia → phrases move without running away.
// Wide note range (D3-E5) → room for different instruments later.
pub static JAZZ: MarkovPreset = MarkovPreset {
    name:     "Jazz",
    scale:    &DORIAN,
    note_min: 50, note_max: 76,
    chords:   &[&D_MIN7, &G_DOM7, &C_MAJ, &A_MIN],
    phrase_min: 5, phrase_max: 9,
    history_len: 5,
    inertia:  0.38,
    max_step: 3,
    figures: &[
        RhythmFigure { durations: &[450.0, 225.0],                   weight: 35 },
        RhythmFigure { durations: &[450.0, 225.0, 450.0, 225.0],     weight: 25 },
        RhythmFigure { durations: &[675.0, 225.0],                   weight: 20 },
        RhythmFigure { durations: &[225.0, 225.0, 225.0, 675.0],     weight: 20 },
    ],
    grid_step_ms: 225.0,
    vel_min: 0.38, vel_max: 0.72,
    accent:  1.22,
    rest_prob: 0.22, rest_steps: 2,
    tonic_pull: 0.50,
    motif_recall_prob:  0.22,
    motif_recall_after: 4,
    layers: &[
        LayerConfig {
            instrument:   Instrument::Piano,
            role:         RhythmRole::Melody,
            note_min:     60, note_max: 76,
            vel_scale:    1.0, grid_mult: 1.0,
            fixed_note:   0, beat_pattern: &[true],
            envelope: None,
        },
        LayerConfig {
            instrument:   Instrument::Bass,
            role:         RhythmRole::Bass,
            note_min:     36, note_max: 52,
            vel_scale:    0.65, grid_mult: 2.0,
            fixed_note:   0, beat_pattern: &[true],
            envelope: None,
        },
        LayerConfig {
            instrument:   Instrument::Hihat,
            role:         RhythmRole::Percussion,
            note_min:     42, note_max: 42,
            vel_scale:    0.38, grid_mult: 1.0,
            fixed_note:   42,
            // swing hihat: hit on 1 and 3, lighter on 2 and 4
            beat_pattern: &[true, true, true, true],
            envelope: None,
        },
    ],
};

// MINIMAL - irregular rhythm, short notes, frequent repetition.
// Stepwise only (max_step:1). Fast grid (200ms) with varying durations
// creates the Philip Glass "irregular pulse" feel.
// High motif recall → the same motif keeps cycling with variations.
pub static MINIMAL: MarkovPreset = MarkovPreset {
    name:     "Minimal",
    scale:    &MAJOR,
    note_min: 60, note_max: 76,
    chords:   &[&C_MAJ, &A_MIN, &F_MAJ, &G_MAJ],
    phrase_min: 4, phrase_max: 7,
    history_len: 4,
    inertia:  0.50,
    max_step: 1,
    figures: &[
        RhythmFigure { durations: &[300.0, 300.0, 300.0, 300.0],    weight: 30 },
        RhythmFigure { durations: &[450.0, 150.0, 300.0],           weight: 25 },
        RhythmFigure { durations: &[150.0, 150.0, 300.0, 300.0],    weight: 25 },
        RhythmFigure { durations: &[600.0, 150.0, 150.0],           weight: 20 },
    ],
    grid_step_ms: 200.0,
    vel_min: 0.40, vel_max: 0.62,
    accent:  1.18,
    rest_prob: 0.15, rest_steps: 1,
    tonic_pull: 0.45,
    motif_recall_prob:  0.60,
    motif_recall_after: 2,
    layers: &[
        LayerConfig {
            instrument:   Instrument::Piano,
            role:         RhythmRole::Melody,
            note_min:     60, note_max: 76,
            vel_scale:    1.0, grid_mult: 1.0,
            fixed_note:   0, beat_pattern: &[true],
            envelope: None,
        },
        LayerConfig {
            instrument:   Instrument::Organ,
            role:         RhythmRole::Pad,
            note_min:     48, note_max: 60,
            vel_scale:    0.45, grid_mult: 8.0,
            fixed_note:   0, beat_pattern: &[true],
            envelope: None,
        },
    ],
};

// CLASSICAL - expressive dynamics, phrase peaks, clear cadences.
// Strong accent (1.28) makes phrase starts pop. Wide velocity range.
// Mix of quarter and eighth figures → natural tempo variation feel.
// Strong tonic pull (0.72) → clear cadential resolutions.
pub static CLASSICAL: MarkovPreset = MarkovPreset {
    name:     "Classical",
    scale:    &MAJOR,
    note_min: 52, note_max: 76,
    chords:   &[&C_MAJ, &F_MAJ, &G_DOM7, &C_MAJ],
    phrase_min: 6, phrase_max: 9,
    history_len: 6,
    inertia:  0.55,
    max_step: 2,
    figures: &[
        RhythmFigure { durations: &[600.0, 300.0, 300.0, 600.0],         weight: 25 },
        RhythmFigure { durations: &[300.0, 300.0, 300.0, 900.0],         weight: 25 },
        RhythmFigure { durations: &[900.0, 300.0, 600.0],                weight: 20 },
        RhythmFigure { durations: &[300.0, 150.0, 150.0, 300.0, 600.0],  weight: 30 },
    ],
    grid_step_ms: 280.0,
    vel_min: 0.35, vel_max: 0.75,
    accent:  1.28,
    rest_prob: 0.38, rest_steps: 2,
    tonic_pull: 0.72,
    motif_recall_prob:  0.42,
    motif_recall_after: 3,
    layers: &[
        LayerConfig {
            instrument:   Instrument::Piano,
            role:         RhythmRole::Melody,
            note_min:     60, note_max: 76,
            vel_scale:    1.0, grid_mult: 1.0,
            fixed_note:   0, beat_pattern: &[true],
            envelope: None,
        },
        LayerConfig {
            instrument:   Instrument::Bass,
            role:         RhythmRole::Bass,
            note_min:     40, note_max: 55,
            vel_scale:    0.60, grid_mult: 2.0,
            fixed_note:   0, beat_pattern: &[true],
            envelope: None,
        },
    ],
};

// DRONE - dark, sustained. Similar to ambient but lower and minor.
// Natural minor (darker colour than pentatonic minor).
// Very slow grid (1000ms), very long notes → maximum overlap, blurring.
// Almost no accent → undifferentiated, hypnotic mass of sound.
pub static DRONE: MarkovPreset = MarkovPreset {
    name:     "Drone",
    scale:    &NATURAL_MINOR,
    note_min: 45, note_max: 64,   // A2-E4: low and dark
    chords:   &[&A_MIN, &G_MIN, &D_MIN, &E_MIN],
    phrase_min: 3, phrase_max: 5,
    history_len: 5,
    inertia:  0.65,
    max_step: 2,
    figures: &[
        RhythmFigure { durations: &[2400.0, 2400.0],                weight: 30 },
        RhythmFigure { durations: &[3200.0, 1600.0],                weight: 30 },
        RhythmFigure { durations: &[1600.0, 1600.0, 2400.0],        weight: 25 },
        RhythmFigure { durations: &[4000.0],                        weight: 15 },
    ],
    grid_step_ms: 1000.0,
    vel_min: 0.15, vel_max: 0.38,
    accent:  1.08,
    rest_prob: 0.55, rest_steps: 3,
    tonic_pull: 0.75,
    motif_recall_prob:  0.50,
    motif_recall_after: 2,
    layers: &[
        LayerConfig {
            instrument:   Instrument::Pad,
            role:         RhythmRole::Melody,
            note_min:     48, note_max: 64,
            vel_scale:    1.0, grid_mult: 1.0,
            fixed_note:   0, beat_pattern: &[true],
            envelope: None,
        },
        LayerConfig {
            instrument:   Instrument::Bass,
            role:         RhythmRole::Pad,
            note_min:     33, note_max: 48,
            vel_scale:    0.55, grid_mult: 3.0,
            fixed_note:   0, beat_pattern: &[true],
            envelope: None,
        },
    ],
};

// CHAOS - all values at extremes, maximum contrast.
// Chromatic scale, huge leaps, extreme velocity swings.
// Short machine-gun bursts next to sudden long held notes.
// Almost no inertia, no tonic pull, almost no motif recall.
pub static CHAOS: MarkovPreset = MarkovPreset {
    name:     "Chaos",
    scale:    &CHROMATIC,
    note_min: 36, note_max: 96,
    chords:   &[&C_MAJ, &E_FLAT, &B_FLAT, &A_MAJ],
    phrase_min: 2, phrase_max: 12,
    history_len: 3,
    inertia:  0.05,
    max_step: 9,
    figures: &[
        RhythmFigure { durations: &[80.0, 80.0, 80.0, 2000.0],      weight: 25 },
        RhythmFigure { durations: &[3000.0, 100.0, 100.0],          weight: 20 },
        RhythmFigure { durations: &[150.0, 300.0, 600.0, 1200.0],   weight: 20 },
        RhythmFigure { durations: &[100.0, 100.0, 100.0, 100.0],    weight: 20 },
        RhythmFigure { durations: &[2500.0, 2500.0],                weight: 15 },
    ],
    grid_step_ms: 150.0,
    vel_min: 0.10, vel_max: 0.95,
    accent:  1.35,
    rest_prob: 0.35, rest_steps: 1,
    tonic_pull: 0.08,
    motif_recall_prob:  0.08,
    motif_recall_after: 6,
    layers: &[
        LayerConfig {
            instrument:   Instrument::Pluck,
            role:         RhythmRole::Melody,
            note_min:     40, note_max: 88,
            vel_scale:    1.0, grid_mult: 1.0,
            fixed_note:   0, beat_pattern: &[true],
            envelope: None,
        },
        LayerConfig {
            instrument:   Instrument::Kick,
            role:         RhythmRole::Percussion,
            note_min:     36, note_max: 36,
            vel_scale:    0.80, grid_mult: 3.0,
            fixed_note:   36,
            beat_pattern: &[true, false, false],
            envelope: None,
        },
    ],
};

pub fn get_preset(name: &str) -> &'static MarkovPreset {
    match name {
        "Jazz"      => &JAZZ,
        "Minimal"   => &MINIMAL,
        "Classical" => &CLASSICAL,
        "Drone"     => &DRONE,
        "Chaos"     => &CHAOS,
        _           => &AMBIENT,
    }
}
