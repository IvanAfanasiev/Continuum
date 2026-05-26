// Markov chain music generator.
//
// A second-order Markov chain: the next note depends on the previous two.
// Each preset has its own transition table built from hand-crafted musical
// patterns — scales, chord tones, typical jazz/classical voice leading, etc.
//
// The chain produces a stream of (note, velocity, duration) tuples.
// Velocity and duration are also Markov-driven via separate tables so the
// rhythm and dynamics feel idiomatic for each style.

use crate::NoteEvent;
use rand::prelude::*;

// ─────────────────────────────────────────────────────────────
//  TRANSITION TABLE
//
//  Maps (prev2, prev1) -> [(next_note, weight), ...]
//  Weights are relative probabilities; they are normalised at sample time.
// ─────────────────────────────────────────────────────────────

pub struct NoteTransition {
    pub from: (u8, u8),             // (prev2, prev1)
    pub to:   &'static [(u8, u32)], // (next_note, weight)
}

// Duration pool: list of (duration_ms, weight)
pub type DurPool = &'static [(u32, u32)];

// Velocity pool: list of (velocity * 100 as u8, weight)  e.g. 60 => 0.60
pub type VelPool = &'static [(u8, u32)];

pub struct MarkovPreset {
    pub name:        &'static str,
    pub transitions: &'static [NoteTransition],
    pub durations:   DurPool,
    pub velocities:  VelPool,
    // Starting notes: randomly pick one of these to seed the chain
    pub seeds:       &'static [u8],
}

// ─────────────────────────────────────────────────────────────
//  GENERATOR
// ─────────────────────────────────────────────────────────────

pub struct MarkovGenerator {
    preset:  &'static MarkovPreset,
    prev2:   u8,
    prev1:   u8,
    rng:     rand::rngs::ThreadRng,
}

impl MarkovGenerator {
    pub fn new(preset: &'static MarkovPreset) -> Self {
        let mut rng = rand::rng();
        // Seed with two random notes from the preset's seed pool
        let a = *preset.seeds.choose(&mut rng).unwrap_or(&60);
        let b = *preset.seeds.choose(&mut rng).unwrap_or(&62);
        Self { preset, prev2: a, prev1: b, rng }
    }

    // Generate the next NoteEvent using the Markov chain.
    pub fn next(&mut self) -> NoteEvent {
        let note     = self.next_note();
        let duration = self.sample_pool_u32(self.preset.durations) as f32;
        let velocity = self.sample_pool_u8(self.preset.velocities) as f32 / 100.0;

        self.prev2 = self.prev1;
        self.prev1 = note;

        NoteEvent { note, velocity, duration }
    }

    // Generate a batch of `count` notes.
    pub fn batch(&mut self, count: usize) -> Vec<NoteEvent> {
        (0..count).map(|_| self.next()).collect()
    }

    // ── internals ──────────────────────────────────────────────

    fn next_note(&mut self) -> u8 {
        // Try exact (prev2, prev1) match first
        let candidates = self.lookup(self.prev2, self.prev1)
            // Fall back to (any, prev1) — ignore second-order context
            .or_else(|| self.lookup_first_order(self.prev1));

        if let Some(table) = candidates {
            self.weighted_pick(table)
        } else {
            // No match at all — pick a random seed note
            *self.preset.seeds.choose(&mut self.rng).unwrap_or(&60)
        }
    }

    fn lookup(&self, p2: u8, p1: u8) -> Option<&'static [(u8, u32)]> {
        self.preset.transitions
            .iter()
            .find(|t| t.from == (p2, p1))
            .map(|t| t.to)
    }

    fn lookup_first_order(&self, p1: u8) -> Option<&'static [(u8, u32)]> {
        // Find any transition whose second element matches prev1
        self.preset.transitions
            .iter()
            .find(|t| t.from.1 == p1)
            .map(|t| t.to)
    }

    fn weighted_pick(&mut self, table: &[(u8, u32)]) -> u8 {
        let total: u32 = table.iter().map(|&(_, w)| w).sum();
        let mut r = self.rng.random_range(0..total);
        for &(note, weight) in table {
            if r < weight { return note; }
            r -= weight;
        }
        table.last().map(|&(n, _)| n).unwrap_or(60)
    }

    fn sample_pool_u32(&mut self, pool: DurPool) -> u32 {
        let total: u32 = pool.iter().map(|&(_, w)| w).sum();
        let mut r = self.rng.random_range(0..total);
        for &(val, w) in pool {
            if r < w { return val; }
            r -= w;
        }
        pool.last().map(|&(v, _)| v).unwrap_or(400)
    }

    fn sample_pool_u8(&mut self, pool: VelPool) -> u8 {
        let total: u32 = pool.iter().map(|&(_, w)| w).sum();
        let mut r = self.rng.random_range(0..total);
        for &(val, w) in pool {
            if r < w { return val; }
            r -= w;
        }
        pool.last().map(|&(v, _)| v).unwrap_or(55)
    }
}

// ─────────────────────────────────────────────────────────────
//  PRESET DEFINITIONS
//
//  Note naming reference (MIDI numbers):
//  C3=48 D3=50 E3=52 F3=53 G3=55 A3=57 B3=59
//  C4=60 D4=62 E4=64 F4=65 G4=67 A4=69 B4=71
//  C5=72 D5=74 E5=76 F5=77 G5=79 A5=81 B5=83
// ─────────────────────────────────────────────────────────────

// ── AMBIENT ────────────────────────────────────────────────
// Pentatonic Am: A3 C4 D4 E4 G4 A4 C5 D5 E5 G5
// Sparse stepwise motion, lots of repeated long notes.

pub static AMBIENT: MarkovPreset = MarkovPreset {
    name: "Ambient",
    seeds: &[45, 48, 50, 52, 55], // A3 C4 D4 E4 G4
    durations: &[
        (800, 10), (1000, 25), (1200, 30), (1600, 20), (2000, 15),
    ],
    velocities: &[
        (22, 10), (30, 25), (38, 30), (45, 25), (52, 10),
    ],
    transitions: &[
        NoteTransition { from: (57, 60), to: &[(60,30),(62,40),(64,20),(67,10)] },
        NoteTransition { from: (60, 62), to: &[(62,10),(64,40),(67,30),(69,20)] },
        NoteTransition { from: (62, 64), to: &[(64,20),(67,40),(69,30),(72,10)] },
        NoteTransition { from: (64, 67), to: &[(67,20),(69,30),(72,30),(74,20)] },
        NoteTransition { from: (67, 69), to: &[(69,20),(72,40),(74,20),(67,20)] },
        NoteTransition { from: (69, 72), to: &[(72,10),(74,30),(76,30),(69,30)] },
        NoteTransition { from: (72, 74), to: &[(74,20),(72,30),(69,30),(67,20)] },
        NoteTransition { from: (74, 76), to: &[(76,10),(74,30),(72,30),(69,30)] },
        // Descending phrases
        NoteTransition { from: (76, 74), to: &[(74,20),(72,40),(69,30),(67,10)] },
        NoteTransition { from: (74, 72), to: &[(72,20),(69,40),(67,30),(64,10)] },
        NoteTransition { from: (72, 69), to: &[(69,20),(67,40),(64,30),(62,10)] },
        NoteTransition { from: (69, 67), to: &[(67,20),(64,40),(62,30),(60,10)] },
        NoteTransition { from: (67, 64), to: &[(64,20),(62,40),(60,30),(57,10)] },
        NoteTransition { from: (64, 62), to: &[(62,20),(60,40),(57,30),(64,10)] },
        NoteTransition { from: (62, 60), to: &[(60,30),(62,30),(57,30),(64,10)] },
        NoteTransition { from: (60, 57), to: &[(57,20),(60,40),(62,30),(64,10)] },
    ],
};

// ── JAZZ ───────────────────────────────────────────────────
// Dm7-G7-Cmaj7 ii-V-I in C.
// Notes: D F A C E G B + chromatic passing tones (Eb, Bb, F#)

pub static JAZZ: MarkovPreset = MarkovPreset {
    name: "Jazz",
    seeds: &[62, 65, 69, 72, 74], // D4 F4 A4 C5 D5
    durations: &[
        (200, 15), (300, 30), (400, 25), (600, 20), (800, 10),
    ],
    velocities: &[
        (35, 10), (45, 20), (55, 30), (65, 25), (75, 15),
    ],
    transitions: &[
        // ii chord tones (Dm7: D F A C)
        NoteTransition { from: (62, 65), to: &[(65,20),(67,30),(69,30),(63,20)] },  // Eb chromatic
        NoteTransition { from: (65, 69), to: &[(69,25),(71,25),(72,25),(68,25)] },
        NoteTransition { from: (69, 72), to: &[(72,30),(74,30),(71,20),(69,20)] },
        // V chord tones (G7: G B D F)
        NoteTransition { from: (67, 71), to: &[(71,30),(72,30),(74,20),(69,20)] },
        NoteTransition { from: (71, 74), to: &[(74,25),(72,25),(71,25),(69,25)] },
        NoteTransition { from: (74, 77), to: &[(77,20),(76,30),(74,30),(72,20)] },
        // I chord tones (Cmaj7: C E G B)
        NoteTransition { from: (72, 76), to: &[(76,30),(77,20),(79,30),(74,20)] },
        NoteTransition { from: (76, 79), to: &[(79,20),(81,20),(77,30),(76,30)] },
        NoteTransition { from: (79, 81), to: &[(81,20),(79,20),(76,30),(74,30)] },
        // Chromatic approach notes
        NoteTransition { from: (63, 62), to: &[(62,40),(60,30),(65,30)] },
        NoteTransition { from: (70, 69), to: &[(69,40),(67,30),(72,30)] },
        NoteTransition { from: (68, 67), to: &[(67,40),(69,30),(65,30)] },
        // Fallbacks
        NoteTransition { from: (72, 74), to: &[(74,30),(72,20),(76,30),(71,20)] },
        NoteTransition { from: (69, 71), to: &[(71,30),(72,30),(74,20),(69,20)] },
        NoteTransition { from: (65, 67), to: &[(67,30),(69,30),(72,20),(65,20)] },
        NoteTransition { from: (62, 60), to: &[(60,30),(62,30),(64,20),(65,20)] },
    ],
};

// ── MINIMAL ────────────────────────────────────────────────
// C major scale, repeating short motifs.
// High probability of returning to nearby notes = hypnotic loops.

pub static MINIMAL: MarkovPreset = MarkovPreset {
    name: "Minimal",
    seeds: &[60, 62, 64, 65, 67],
    durations: &[
        (250, 30), (300, 30), (400, 25), (500, 15),
    ],
    velocities: &[
        (42, 20), (50, 40), (58, 30), (65, 10),
    ],
    transitions: &[
        NoteTransition { from: (60, 62), to: &[(62,15),(64,40),(62,30),(60,15)] },
        NoteTransition { from: (62, 64), to: &[(64,15),(65,35),(62,35),(67,15)] },
        NoteTransition { from: (64, 65), to: &[(65,15),(67,35),(64,35),(62,15)] },
        NoteTransition { from: (65, 67), to: &[(67,15),(69,35),(65,35),(64,15)] },
        NoteTransition { from: (67, 69), to: &[(69,15),(71,30),(67,40),(65,15)] },
        NoteTransition { from: (69, 71), to: &[(71,15),(72,25),(69,45),(67,15)] },
        NoteTransition { from: (71, 72), to: &[(72,10),(71,30),(69,40),(67,20)] },
        NoteTransition { from: (72, 71), to: &[(71,20),(69,40),(67,30),(72,10)] },
        NoteTransition { from: (71, 69), to: &[(69,20),(67,40),(65,30),(71,10)] },
        NoteTransition { from: (69, 67), to: &[(67,20),(65,40),(64,30),(69,10)] },
        NoteTransition { from: (67, 65), to: &[(65,20),(64,40),(62,30),(67,10)] },
        NoteTransition { from: (65, 64), to: &[(64,20),(62,40),(60,30),(65,10)] },
        NoteTransition { from: (64, 62), to: &[(62,20),(60,40),(64,30),(65,10)] },
        NoteTransition { from: (62, 60), to: &[(60,30),(62,40),(64,20),(67,10)] },
        NoteTransition { from: (60, 60), to: &[(60,20),(62,40),(64,30),(65,10)] },
        NoteTransition { from: (64, 64), to: &[(64,20),(65,30),(67,30),(62,20)] },
    ],
};

// ── CLASSICAL ──────────────────────────────────────────────
// G major. Mix of melody (upper voice) and bass motion.
// Follows I-IV-V-I voice leading.

pub static CLASSICAL: MarkovPreset = MarkovPreset {
    name: "Classical",
    seeds: &[55, 59, 62, 67, 71], // G3 B3 D4 G4 B4
    durations: &[
        (300, 20), (400, 35), (600, 25), (800, 20),
    ],
    velocities: &[
        (45, 20), (55, 40), (65, 30), (72, 10),
    ],
    transitions: &[
        // G major chord (G B D)
        NoteTransition { from: (55, 59), to: &[(59,30),(62,35),(67,25),(55,10)] },
        NoteTransition { from: (59, 62), to: &[(62,25),(64,35),(67,25),(59,15)] },
        NoteTransition { from: (62, 67), to: &[(67,30),(69,30),(71,25),(64,15)] },
        // C major chord (C E G) — IV
        NoteTransition { from: (60, 64), to: &[(64,30),(67,35),(72,20),(62,15)] },
        NoteTransition { from: (64, 67), to: &[(67,25),(69,30),(72,25),(64,20)] },
        NoteTransition { from: (67, 72), to: &[(72,25),(74,25),(71,30),(69,20)] },
        // D major chord (D F# A) — V
        NoteTransition { from: (62, 66), to: &[(66,30),(69,30),(74,25),(62,15)] },
        NoteTransition { from: (66, 69), to: &[(69,30),(71,30),(74,25),(67,15)] },
        NoteTransition { from: (69, 74), to: &[(74,25),(72,30),(71,25),(69,20)] },
        // Leading tone resolution B->C, F#->G
        NoteTransition { from: (71, 72), to: &[(72,50),(74,25),(67,25)] },
        NoteTransition { from: (66, 67), to: &[(67,50),(69,25),(62,25)] },
        // Stepwise descending
        NoteTransition { from: (74, 72), to: &[(72,30),(71,30),(69,25),(67,15)] },
        NoteTransition { from: (72, 71), to: &[(71,25),(69,35),(67,25),(72,15)] },
        NoteTransition { from: (71, 69), to: &[(69,25),(67,35),(66,20),(71,20)] },
        NoteTransition { from: (69, 67), to: &[(67,30),(66,25),(64,25),(69,20)] },
        NoteTransition { from: (67, 64), to: &[(64,30),(62,35),(60,20),(67,15)] },
    ],
};

// ── DRONE ──────────────────────────────────────────────────
// D minor. Very slow, pedal on D+A, narrow melodic range.

pub static DRONE: MarkovPreset = MarkovPreset {
    name: "Drone",
    seeds: &[50, 57, 62, 65, 69], // D3 A3 D4 F4 A4
    durations: &[
        (1200, 15), (1600, 25), (2000, 30), (2500, 20), (3000, 10),
    ],
    velocities: &[
        (18, 20), (25, 35), (33, 30), (40, 15),
    ],
    transitions: &[
        NoteTransition { from: (50, 57), to: &[(57,30),(60,30),(62,25),(65,15)] },
        NoteTransition { from: (57, 62), to: &[(62,30),(65,30),(67,25),(60,15)] },
        NoteTransition { from: (62, 65), to: &[(65,30),(67,25),(69,25),(62,20)] },
        NoteTransition { from: (65, 69), to: &[(69,25),(67,30),(65,30),(72,15)] },
        NoteTransition { from: (69, 67), to: &[(67,30),(65,30),(62,25),(69,15)] },
        NoteTransition { from: (67, 65), to: &[(65,30),(62,30),(60,25),(57,15)] },
        NoteTransition { from: (65, 62), to: &[(62,30),(60,30),(57,25),(65,15)] },
        NoteTransition { from: (62, 60), to: &[(60,30),(57,35),(62,25),(65,10)] },
        NoteTransition { from: (60, 57), to: &[(57,30),(50,25),(62,30),(65,15)] },
        NoteTransition { from: (57, 50), to: &[(50,30),(57,40),(62,30)] },
        // Pedal oscillation
        NoteTransition { from: (50, 50), to: &[(50,20),(57,40),(62,40)] },
        NoteTransition { from: (57, 57), to: &[(57,20),(62,40),(65,40)] },
    ],
};

// ── CHAOS ──────────────────────────────────────────────────
// Atonal. Wide leaps, chromatic, unpredictable.

pub static CHAOS: MarkovPreset = MarkovPreset {
    name: "Chaos",
    seeds: &[48, 55, 63, 71, 80, 88],
    durations: &[
        (100, 15), (200, 20), (400, 20), (700, 20), (1000, 15), (1400, 10),
    ],
    velocities: &[
        (15, 15), (35, 20), (55, 20), (70, 25), (88, 20),
    ],
    transitions: &[
        NoteTransition { from: (48, 55), to: &[(55,15),(71,15),(80,15),(63,15),(42,15),(88,10),(36,15)] },
        NoteTransition { from: (55, 71), to: &[(71,15),(48,15),(85,15),(60,15),(77,15),(43,15),(91,10)] },
        NoteTransition { from: (71, 80), to: &[(80,15),(55,15),(63,15),(90,15),(48,15),(75,10),(36,15)] },
        NoteTransition { from: (80, 63), to: &[(63,15),(88,15),(48,15),(74,15),(55,15),(40,10),(96,10)] },
        NoteTransition { from: (63, 88), to: &[(88,15),(50,15),(72,15),(60,15),(85,15),(45,10),(36,10)] },
        NoteTransition { from: (88, 48), to: &[(48,15),(75,15),(60,15),(84,15),(55,15),(90,10),(40,10)] },
        NoteTransition { from: (48, 63), to: &[(63,15),(77,15),(54,15),(88,15),(45,15),(70,10),(36,10)] },
        NoteTransition { from: (63, 55), to: &[(55,15),(80,15),(48,15),(69,15),(88,15),(41,10),(96,10)] },
    ],
};

// ── All presets ─────────────────────────────────────────────

pub fn get_preset(name: &str) -> &'static MarkovPreset {
    match name {
        "Jazz"      => &JAZZ,
        "Minimal"   => &MINIMAL,
        "Classical" => &CLASSICAL,
        "Drone"     => &DRONE,
        "Chaos"     => &CHAOS,
        _           => &AMBIENT, // default
    }
}
