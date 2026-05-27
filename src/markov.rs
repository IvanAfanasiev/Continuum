// Melodic Markov generator.
//
// Note selection weights (computed per candidate, not stored as tables):
//   chord_bonus        - scale tone belonging to the current chord: weight x3.5
//   inertia            - continuing in the prevailing direction: weight x(1+inertia)
//   recency_penalty    - recently played notes are down-weighted
//   edge_penalty       - notes near the range extremes are less likely
//
// Timing:
//   The composer sleeps grid_step_ms between notes, NOT the note duration.
//   Note duration > grid_step → legato overlap.
//   Note duration < grid_step → natural gap between notes.

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

pub static PENTATONIC_MINOR: Scale = Scale { root: 57, intervals: &[0,3,5,7,10] };
pub static PENTATONIC_MAJOR: Scale = Scale { root: 60, intervals: &[0,2,4,7,9] };
pub static NATURAL_MINOR:    Scale = Scale { root: 57, intervals: &[0,2,3,5,7,8,10] };
pub static MAJOR:            Scale = Scale { root: 60, intervals: &[0,2,4,5,7,9,11] };
pub static DORIAN:           Scale = Scale { root: 62, intervals: &[0,2,3,5,7,9,10] };
pub static CHROMATIC:        Scale = Scale { root: 60, intervals: &[0,1,2,3,4,5,6,7,8,9,10,11] };

// ─────────────────────────────────────────────────────────────
//  CHORD
// ─────────────────────────────────────────────────────────────

pub struct Chord { pub root: u8, pub intervals: &'static [u8] }

impl Chord {
    pub fn contains(&self, note: u8) -> bool {
        let iv = (note % 12) as i16 - (self.root % 12) as i16;
        self.intervals.contains(&(iv.rem_euclid(12) as u8))
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
static B_FLAT: Chord = Chord { root: 70, intervals: &[0,4,7] };
static E_FLAT: Chord = Chord { root: 63, intervals: &[0,4,7] };
static A_MAJ:  Chord = Chord { root: 69, intervals: &[0,4,7] };

// ─────────────────────────────────────────────────────────────
//  RHYTHM VALUE
// ─────────────────────────────────────────────────────────────

pub struct RhythmValue { pub duration_ms: f32, pub weight: u32 }

// ─────────────────────────────────────────────────────────────
//  PRESET
// ─────────────────────────────────────────────────────────────

pub struct MarkovPreset {
    pub name:  &'static str,
    pub scale: &'static Scale,
    pub note_min: u8,
    pub note_max: u8,

    // Harmonic progression, looped, one chord per phrase
    pub chords: &'static [&'static Chord],

    // Notes per phrase
    pub phrase_min: usize,
    pub phrase_max: usize,

    // Anti-repeat: how many recent notes to remember
    pub history_len: usize,

    // Direction inertia [0..1]: higher = smoother melodic contour
    pub inertia: f32,

    // Max scale-degree step per note
    pub max_step: usize,

    // Note durations
    pub rhythm: &'static [RhythmValue],

    // Time between note onsets (eighth note in the tempo)
    pub grid_step_ms: f32,


    // Velocity range
    pub vel_min: f32,
    pub vel_max: f32,

    // Rest between phrases: probability and length in grid steps
    pub rest_prob:  f64,
    pub rest_steps: u32,
}

// ─────────────────────────────────────────────────────────────
//  GENERATOR
// ─────────────────────────────────────────────────────────────

pub struct MarkovGenerator {
    preset:       &'static MarkovPreset,
    rng:          rand::rngs::ThreadRng,
    tones:        Vec<u8>,
    history:      VecDeque<u8>,
    direction:    i32,         // +1 up, -1 down
    phrase_pos:   usize,
    phrase_len:   usize,
    chord_idx:    usize,
    current_vel:  f32,
    // Counts how many steps since last direction change, prevents
    // flip-flopping on every note at the range edges.
    steps_in_dir: usize,
}

impl MarkovGenerator {
    pub fn new(preset: &'static MarkovPreset) -> Self {
        let mut rng  = rand::rng();
        let tones    = preset.scale.tones_in_range(preset.note_min, preset.note_max);
        let start    = tones[tones.len() / 3];
        let phrase_len = rng.random_range(preset.phrase_min..=preset.phrase_max);
        let vel = (preset.vel_min + preset.vel_max) / 2.0;
        let mut history = VecDeque::with_capacity(preset.history_len + 1);
        history.push_back(start);
        Self {
            preset, rng, tones, history,
            direction: 1,
            phrase_pos: 0, phrase_len,
            chord_idx: 0,
            current_vel: vel,
            steps_in_dir: 0,
        }
    }

    // How long to sleep before the next note onset.
    pub fn grid_step_ms(&self) -> f32 {
        self.preset.grid_step_ms
    }

    // Returns Some(ms) at phrase boundaries when a rest should occur.
    // Rests happen AFTER a phrase ends, before the next begins.
    pub fn phrase_rest_ms(&mut self) -> Option<f32> {
        // Only at phrase boundary, and only sometimes
        if self.phrase_pos == 0 && self.rng.random_bool(self.preset.rest_prob) {
            Some(self.preset.grid_step_ms * self.preset.rest_steps as f32)
        } else {
            None
        }
    }

    pub fn next(&mut self) -> NoteEvent {
        // ── phrase boundary ───────────────────────────────────
        if self.phrase_pos >= self.phrase_len {
            self.phrase_pos = 0;
            self.phrase_len = self.rng.random_range(
                self.preset.phrase_min..=self.preset.phrase_max
            );
            // Advance chord on phrase boundary
            self.chord_idx = (self.chord_idx + 1) % self.preset.chords.len();
            // Do NOT flip direction here, let the note-picking logic
            // handle direction naturally. Flipping at phrase boundary
            // caused sudden melodic jumps.
        }

        let note = self.pick_note();
        let dur  = self.pick_duration();

        let drift = self.rng.random_range(-0.03f32..0.03);
        self.current_vel = (self.current_vel + drift)
            .clamp(self.preset.vel_min, self.preset.vel_max);

        self.history.push_back(note);
        while self.history.len() > self.preset.history_len {
            self.history.pop_front();
        }
        self.phrase_pos += 1;

        NoteEvent { note, velocity: self.current_vel, duration: dur }
    }

    // ── private ────────────────────────────────────────────────

    fn pick_note(&mut self) -> u8 {
        let prev = *self.history.back().unwrap_or(&60);
        let chord = self.preset.chords[self.chord_idx];

        let cur_idx = self.tones.iter().position(|&n| n == prev)
            .unwrap_or(self.tones.len() / 2);

        let max_step = self.preset.max_step;
        let lo = cur_idx.saturating_sub(max_step);
        let hi = (cur_idx + max_step).min(self.tones.len().saturating_sub(1));

        // Auto-reverse direction when we hit the range edges,
        // but only after staying there for at least 2 steps.
        // This prevents the generator from camping at one extreme.
        let at_top = cur_idx + 1 >= self.tones.len();
        let at_bot = cur_idx == 0;
        if (at_top && self.direction > 0) || (at_bot && self.direction < 0) {
            if self.steps_in_dir >= 2 {
                self.direction = -self.direction;
                self.steps_in_dir = 0;
            }
        }

        let mut candidates: Vec<(usize, f32)> = (lo..=hi)
            .filter(|&i| i != cur_idx)
            .map(|i| {
                let note = self.tones[i];
                let mut w = 1.0f32;

                // Chord tone bonus
                if chord.contains(note) { w *= 3.5; }

                // Direction inertia with fade:
                // after 'steps_in_dir' steps the inertia decays so the melody
                // naturally turns rather than running to the range extremes.
                let decay = 1.0 / (1.0 + self.steps_in_dir as f32 * 0.35);
                let effective_inertia = self.preset.inertia * decay;
                let step_dir = i as i32 - cur_idx as i32;
                if step_dir.signum() == self.direction {
                    w *= 1.0 + effective_inertia;
                } else {
                    w *= (1.0 - effective_inertia * 0.6).max(0.15);
                }
                w = w.max(0.01);

                // Recency penalty
                for (age, &h) in self.history.iter().rev().enumerate() {
                    if h == note {
                        let p = 0.90 / (1.0 + age as f32 * 0.4);
                        w *= 1.0 - p;
                        break;
                    }
                }
                w = w.max(0.01);

                // Edge penalty, avoid camping at extremes
                let dist = i.min(self.tones.len().saturating_sub(1) - i) as f32;
                if dist < 2.0 { w *= 0.4 + dist * 0.3; }

                (i, w.max(0.01))
            })
            .collect();

        if candidates.is_empty() {
            // Fallback: step toward the middle of the range
            let mid = self.tones.len() / 2;
            let fallback = if cur_idx < mid {
                (cur_idx + 1).min(self.tones.len() - 1)
            } else {
                cur_idx.saturating_sub(1)
            };
            return self.tones[fallback];
        }

        let total: f32 = candidates.iter().map(|&(_, w)| w).sum();
        let mut r = self.rng.random_range(0.0f32..total);
        let mut chosen = candidates[0].0;
        for &(idx, w) in &candidates {
            if r < w { chosen = idx; break; }
            r -= w;
        }

        // Track direction and step count
        let moved = chosen as i32 - cur_idx as i32;
        if moved.signum() == self.direction {
            self.steps_in_dir += 1;
        } else if moved != 0 {
            self.direction    = moved.signum();
            self.steps_in_dir = 1;
        }

        self.tones[chosen]
    }

    fn pick_duration(&mut self) -> f32 {
        let total: u32 = self.preset.rhythm.iter().map(|r| r.weight).sum();
        let mut r = self.rng.random_range(0..total);
        for rv in self.preset.rhythm {
            if r < rv.weight { return rv.duration_ms; }
            r -= rv.weight;
        }
        self.preset.rhythm.last().map(|r| r.duration_ms).unwrap_or(400.0)
    }
}

// ─────────────────────────────────────────────────────────────
//  PRESETS
// ─────────────────────────────────────────────────────────────

pub static AMBIENT: MarkovPreset = MarkovPreset {
    name: "Ambient",
    scale: &PENTATONIC_MINOR,
    note_min: 48, note_max: 69,
    chords: &[&A_MIN, &C_MAJ, &G_MAJ, &F_MAJ],
    phrase_min: 4, phrase_max: 7,
    history_len: 6,
    inertia: 0.55,  // fades after 3-4 steps so melody turns naturally
    max_step: 2,    // allows steps of 1 or 2 scale degrees
    rhythm: &[
        RhythmValue { duration_ms: 1800.0, weight: 15 },
        RhythmValue { duration_ms: 1200.0, weight: 40 },
        RhythmValue { duration_ms:  900.0, weight: 30 },
        RhythmValue { duration_ms:  600.0, weight: 15 },
    ],
    grid_step_ms: 400.0, // onset every 400ms; 1200ms note = 3x overlap = legato
    vel_min: 0.28, vel_max: 0.52,
    rest_prob: 0.40, rest_steps: 2,
};

pub static JAZZ: MarkovPreset = MarkovPreset {
    name: "Jazz",
    scale: &DORIAN,
    note_min: 52, note_max: 74,
    chords: &[&D_MIN7, &G_DOM7, &C_MAJ, &A_MIN],
    phrase_min: 5, phrase_max: 8,
    history_len: 5,
    inertia: 0.38,
    max_step: 3,
    rhythm: &[
        RhythmValue { duration_ms: 450.0, weight: 15 },
        RhythmValue { duration_ms: 300.0, weight: 40 },
        RhythmValue { duration_ms: 225.0, weight: 30 },
        RhythmValue { duration_ms: 150.0, weight: 15 },
    ],
    grid_step_ms: 225.0,
    vel_min: 0.38, vel_max: 0.75,
    rest_prob: 0.28, rest_steps: 2,
};

pub static MINIMAL: MarkovPreset = MarkovPreset {
    name: "Minimal",
    scale: &MAJOR,
    note_min: 57, note_max: 74, // A3-D5
    chords: &[&C_MAJ, &A_MIN, &F_MAJ, &G_MAJ],
    phrase_min: 6, phrase_max: 8,
    history_len: 4,
    inertia: 0.38,
    max_step: 2,
    rhythm: &[
        RhythmValue { duration_ms: 500.0, weight: 20 },
        RhythmValue { duration_ms: 375.0, weight: 40 },
        RhythmValue { duration_ms: 250.0, weight: 30 },
        RhythmValue { duration_ms: 187.0, weight: 10 },
    ],
    grid_step_ms:     250.0,
    vel_min: 0.42, vel_max: 0.62,
    rest_prob: 0.18, rest_steps: 2,
};

pub static CLASSICAL: MarkovPreset = MarkovPreset {
    name: "Classical",
    scale: &MAJOR,
    note_min: 52, note_max: 74, // E3-D5
    chords: &[&C_MAJ, &F_MAJ, &G_DOM7, &C_MAJ],
    phrase_min: 6, phrase_max: 8,
    history_len: 5,
    inertia: 0.52,
    max_step: 2,
    rhythm: &[
        RhythmValue { duration_ms: 600.0, weight: 15 },
        RhythmValue { duration_ms: 450.0, weight: 30 },
        RhythmValue { duration_ms: 300.0, weight: 35 },
        RhythmValue { duration_ms: 150.0, weight: 20 },
    ],
    grid_step_ms:     300.0,
    vel_min: 0.42, vel_max: 0.68,
    rest_prob: 0.32, rest_steps: 2,
};

pub static DRONE: MarkovPreset = MarkovPreset {
    name: "Drone",
    scale: &NATURAL_MINOR,
    note_min: 45, note_max: 65, // A2-F4
    chords: &[&A_MIN, &G_MIN, &D_MIN, &E_MIN],
    phrase_min: 3, phrase_max: 5,
    history_len: 4,
    inertia: 0.62,
    max_step: 2,
    rhythm: &[
        RhythmValue { duration_ms: 3000.0, weight: 15 },
        RhythmValue { duration_ms: 2400.0, weight: 25 },
        RhythmValue { duration_ms: 1800.0, weight: 35 },
        RhythmValue { duration_ms: 1200.0, weight: 25 },
    ],
    grid_step_ms:     900.0,
    vel_min: 0.18, vel_max: 0.40,
    rest_prob: 0.50, rest_steps: 3,
};

pub static CHAOS: MarkovPreset = MarkovPreset {
    name: "Chaos",
    scale: &CHROMATIC,
    note_min: 40, note_max: 84,
    chords: &[&C_MAJ, &E_FLAT, &B_FLAT, &A_MAJ],
    phrase_min: 3, phrase_max: 10,
    history_len: 3,
    inertia: 0.10,
    max_step: 7,
    rhythm: &[
        RhythmValue { duration_ms: 800.0, weight: 10 },
        RhythmValue { duration_ms: 500.0, weight: 20 },
        RhythmValue { duration_ms: 300.0, weight: 30 },
        RhythmValue { duration_ms: 150.0, weight: 25 },
        RhythmValue { duration_ms:  80.0, weight: 15 },
    ],
    grid_step_ms:     180.0,
    vel_min: 0.20, vel_max: 0.85,
    rest_prob: 0.18, rest_steps: 1,
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
