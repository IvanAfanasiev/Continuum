//! RiffCell is the overall identity of the seed, not the fourth layer.
//!
//! One seed gives one style riff-foundation: a mask of accents on
//! 1-2 measures, repeated phrases and variable A/B. The mask doesn't sound
//! itself: it colors already existing media - probability,
//! articulation, accents and harmonic roots of bass/chords. That's why
//! the riff can be heard in the bass, chords and melody, but no new voice
//! appears in the ensemble.
//!
//! The pattern grammar is style-specific (deep swings on offbeats, club —
//! fast beats, rock - downstroke, phonk - sparse
//! half-time, proto - hypnotic quarters, lofi - lazy
//! eighths). seed chooses a mutation, but not a genre.

use crate::arranger::{splitmix64, SongContext};

/// Two bars maximum: more is no longer a riff, but a form.
const MAX_STEPS: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RiffKind {
    None,
    Deep,
    Club,
    Rock,
    Phonk,
    Proto,
    Lofi,
}

impl RiffKind {
    fn salt(self) -> u64 {
        match self {
            RiffKind::None => 0,
            RiffKind::Deep => 0x0DEE,
            RiffKind::Club => 0xC1B5,
            RiffKind::Rock => 0x0B0C,
            RiffKind::Phonk => 0xF0A,
            RiffKind::Proto => 0x9A70,
            RiffKind::Lofi => 0x10F1,
        }
    }

    /// A and B are two characters of the same grammar, not two different riffs.
    fn patterns(self) -> &'static [f32; 16] {
        // For a style with 16 steps per measure: mask per measure.
        match self {
            RiffKind::None => &EMPTY,
            RiffKind::Deep => &DEEP_A,
            RiffKind::Club => &CLUB_A,
            RiffKind::Rock => &ROCK_A,
            RiffKind::Phonk => &PHONK_A,
            RiffKind::Proto => &PROTO_A,
            RiffKind::Lofi => &LOFI_A,
        }
    }

    fn patterns_b(self) -> &'static [f32; 16] {
        match self {
            RiffKind::None => &EMPTY,
            RiffKind::Deep => &DEEP_B,
            RiffKind::Club => &CLUB_B,
            RiffKind::Rock => &ROCK_B,
            RiffKind::Phonk => &PHONK_B,
            RiffKind::Proto => &PROTO_B,
            RiffKind::Lofi => &LOFI_B,
        }
    }

    /// Harmonic intervals in scale degrees: riff foundation
    /// bass and chords. The first one is always 0 - the anchor does not move away.
    fn root_intervals(self) -> &'static [u8] {
        match self {
            RiffKind::None => &[0],
            RiffKind::Deep => &[0, 2],
            RiffKind::Club => &[0, 2, 4],
            RiffKind::Rock => &[0, 2, 4],
            RiffKind::Phonk => &[0, 2, 3],
            RiffKind::Proto => &[0, 2],
            RiffKind::Lofi => &[0, 2, 4],
        }
    }

    /// After how many steps can the root change? Deep/rock/phonk
    /// hold it for half a beat, club - beat: the movement is audible, but the bass
    /// doesn't turn into a melody.
    fn root_hold(self) -> usize {
        match self {
            RiffKind::Club => 4,
            _ => 8,
        }
    }
}

/// A compiled riff mask for the current phrase.
#[derive(Clone, Copy, Debug)]
pub struct RiffCell {
    kind: RiffKind,
    variant: u8,
    bars: u8,
    steps_per_measure: usize,
    accents: [f32; MAX_STEPS],
    roots: [u8; MAX_STEPS],
}

const EMPTY: [f32; 16] = [0.0; 16];

// Deep: soft offbeat pushes, the pattern is shifted from the quarters.
const DEEP_A: [f32; 16] = [
    0.0, 0.0, 0.9, 0.0, 0.0, 0.0, 0.85, 0.0, 0.0, 0.0, 0.8, 0.0, 0.0, 0.0, 0.9, 0.0,
];
const DEEP_B: [f32; 16] = [
    0.0, 0.0, 0.9, 0.0, 0.0, 0.0, 0.85, 0.0, 0.0, 0.0, 0.8, 0.0, 0.0, 0.25, 0.9, 0.0,
];

// Club: strong quarters and fast moves in between.
const CLUB_A: [f32; 16] = [
    1.0, 0.0, 0.7, 0.0, 0.9, 0.0, 0.7, 0.0, 1.0, 0.0, 0.7, 0.0, 0.9, 0.0, 0.8, 0.35,
];
const CLUB_B: [f32; 16] = [
    1.0, 0.0, 0.0, 0.7, 0.9, 0.0, 0.65, 0.0, 1.0, 0.0, 0.8, 0.0, 0.0, 0.6, 0.9, 0.0,
];

// Rock: downstroke by quarter, answer in between.
const ROCK_A: [f32; 16] = [
    1.0, 0.0, 0.0, 0.0, 0.9, 0.0, 0.7, 0.0, 1.0, 0.0, 0.0, 0.0, 0.9, 0.0, 0.7, 0.0,
];
const ROCK_B: [f32; 16] = [
    1.0, 0.0, 0.0, 0.6, 0.0, 0.0, 0.9, 0.0, 0.0, 0.6, 0.0, 0.0, 1.0, 0.0, 0.7, 0.0,
];

// Phonk: half-time, sparse beats, air between them.
const PHONK_A: [f32; 16] = [
    1.0, 0.0, 0.0, 0.0, 0.9, 0.0, 0.0, 0.0, 0.0, 0.0, 0.85, 0.0, 1.0, 0.0, 0.7, 0.0,
];
const PHONK_B: [f32; 16] = [
    1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.9, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.8, 0.0,
];

// Proto: hypnotic quarters; B adds a short pickup.
const PROTO_A: [f32; 16] = [
    1.0, 0.0, 0.0, 0.0, 0.9, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.9, 0.0, 0.0, 0.0,
];
const PROTO_B: [f32; 16] = [
    1.0, 0.0, 0.0, 0.0, 0.9, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.9, 0.0, 0.5, 0.0,
];

// Lofi: Lazy eighth notes, syncopation on the second half.
const LOFI_A: [f32; 16] = [
    1.0, 0.0, 0.0, 0.0, 0.6, 0.0, 0.0, 0.0, 0.9, 0.0, 0.0, 0.0, 0.6, 0.0, 0.0, 0.0,
];
const LOFI_B: [f32; 16] = [
    1.0, 0.0, 0.0, 0.0, 0.6, 0.0, 0.0, 0.4, 0.9, 0.0, 0.0, 0.0, 0.6, 0.0, 0.0, 0.0,
];

impl RiffCell {
    /// Empty Mask: Normal guide path, no riff.
    pub const NONE: Self = Self {
        kind: RiffKind::None,
        variant: 0,
        bars: 1,
        steps_per_measure: 16,
        accents: [0.0; MAX_STEPS],
        roots: [0; MAX_STEPS],
    };

    /// Construct a phrase mask. Determined (style, seed, phrase):
    /// same seed - same riff, different phrase - A/B variation.
    pub fn generate(kind: RiffKind, song: &SongContext, steps_per_measure: usize) -> Self {
        if kind == RiffKind::None || steps_per_measure == 0 {
            return Self::NONE;
        }
        let spm = steps_per_measure.min(16);
        let seed = splitmix64(song.seed_hash ^ kind.salt());
        // A/B lasts the entire phrase (two bars), rather than flickering across bars.
        let variant = (song.phrase_index & 1) as u8;
        let bars = if spm * 2 <= MAX_STEPS && splitmix64(seed ^ 0xBA25) & 1 == 0 {
            2
        } else {
            1
        };
        let base = if variant == 0 {
            kind.patterns()
        } else {
            kind.patterns_b()
        };
        let mut cell = Self {
            kind,
            variant,
            bars,
            steps_per_measure: spm,
            accents: [0.0; MAX_STEPS],
            roots: [0; MAX_STEPS],
        };
        // Seed signature: one added and one softened share. This
        // enough for the seeds to differ in riff, but the grammar of the style
        // was not blurred by random accents.
        let lift = (splitmix64(seed ^ 0x51A7) % spm as u64) as usize;
        let soften = (splitmix64(seed ^ 0x50F7) % spm as u64) as usize;
        // The third mutation of the seed is a short response in the second half of the measure:
        // An A/B riff has 3-5 edits rather than just one.
        let echo = spm / 2 + (splitmix64(seed ^ 0xECA0) % (spm as u64 / 2).max(1)) as usize;
        let turn = if spm > 4 {
            4 + (splitmix64(seed ^ 0xC0DE) % (spm as u64 - 4)) as usize
        } else {
            spm / 2
        };
        for bar in 0..bars as usize {
            for (step, &base_w) in base.iter().enumerate().take(spm) {
                let mut w = base_w;
                if step == lift {
                    w = (w + 0.18).min(1.0);
                }
                if step == soften && w > 0.0 {
                    w *= 0.82;
                }
                if step == echo && w < 0.3 {
                    w = 0.45;
                }
                // The second bar is not a copy: a short pickup at the end.
                if bar == 1 && step == turn && w < 0.2 {
                    w = 0.35;
                }
                cell.accents[bar * spm + step] = w.clamp(0.0, 1.0);
            }
        }
        // Harmonic foundation: the first step of a cell is always the root,
        // Then the intervals are selected by the seed and last half a beat/beat.
        let intervals = kind.root_intervals();
        let hold = kind.root_hold().min(spm).max(1);
        for bar in 0..bars as usize {
            let mut step = 0;
            while step < spm {
                let idx = if bar == 0 && step == 0 {
                    0
                } else {
                    let h = splitmix64(
                        seed ^ (bar as u64).wrapping_mul(0x9E37_79B9)
                            ^ (step as u64).wrapping_mul(0x85EB_CA6B),
                    );
                    (h % intervals.len() as u64) as usize
                };
                let root = intervals[idx];
                for k in 0..hold.min(spm - step) {
                    cell.roots[bar * spm + step + k] = root;
                }
                step += hold;
            }
        }
        cell
    }

    pub fn is_active(&self) -> bool {
        self.kind != RiffKind::None
    }

    pub fn variant(&self) -> u8 {
        self.variant
    }

    pub fn bars(&self) -> u8 {
        self.bars
    }

    /// Riff accent at step 0..1. The phrase holds the pattern, the measure of the cell
    /// selected by global measure: 2-measure cell is not confused
    /// at the boundaries of sections.
    pub fn accent(&self, song: &SongContext, step_in_measure: usize) -> f32 {
        if !self.is_active() {
            return 0.0;
        }
        self.accents[self.index(song, step_in_measure)]
    }

    /// Harmonic interval (scale degrees) in this step.
    pub fn root(&self, song: &SongContext, step_in_measure: usize) -> u8 {
        if !self.is_active() {
            return 0;
        }
        self.roots[self.index(song, step_in_measure)]
    }

    fn index(&self, song: &SongContext, step_in_measure: usize) -> usize {
        let bar = if self.bars == 2 {
            song.measure_index.saturating_sub(1) % 2
        } else {
            0
        };
        (bar * self.steps_per_measure + step_in_measure.min(self.steps_per_measure - 1)) % MAX_STEPS
    }

    /// Probability weight: riff beats sound more often, empty beats sound less often.
    pub fn probability(accent: f32) -> f32 {
        0.90 + 0.25 * accent.clamp(0.0, 1.0)
    }

    /// Volume emphasis: The riff is audible, but it doesn't overwhelm the meter.
    pub fn velocity(accent: f32) -> f32 {
        1.0 + 0.10 * accent.clamp(0.0, 1.0)
    }

    /// Shift in the choice of note length: in the accent the note stretches, in the void -
    /// Briefly speaking. This is an articulation, not a multiplier.
    pub fn gate_bias(accent: f32) -> f32 {
        0.10 * accent.clamp(0.0, 1.0) - 0.03
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song(seed: u64, phrase: usize, measure: usize) -> SongContext {
        SongContext {
            section: crate::arranger::SongSection::MainMotif,
            measure_index: measure,
            phrase_index: phrase,
            energy: 1.0,
            progress: 0.5,
            is_last_measure: false,
            section_measure_index: 1,
            previous_section: crate::arranger::SongSection::MainMotif,
            stage: 1,
            cycle: 0,
            stage_gains: [1.0; 8],
            layers: crate::piece::layers_for(crate::arranger::SongSection::MainMotif),
            seed_hash: seed,
            density: 1.0,
            motif: [0; 16],
            motif_b: [0; 16],
            use_alt_motif: false,
            prog: [0, 0, 0, 0],
            tonic_walk: 0,
        }
    }

    #[test]
    fn same_seed_same_cell() {
        let a = RiffCell::generate(RiffKind::Deep, &song(11, 3, 5), 16);
        let b = RiffCell::generate(RiffKind::Deep, &song(11, 3, 5), 16);
        assert_eq!(a.accents, b.accents);
        assert_eq!(a.roots, b.roots);
        assert_eq!(a.variant, b.variant);
    }

    #[test]
    fn phrase_alternates_a_b() {
        let a = RiffCell::generate(RiffKind::Club, &song(11, 2, 5), 16);
        let b = RiffCell::generate(RiffKind::Club, &song(11, 3, 7), 16);
        assert_eq!(a.variant, 0);
        assert_eq!(b.variant, 1);
        assert_ne!(a.accents, b.accents, "A and B must differ");
    }

    #[test]
    fn seeds_give_distinct_riffs() {
        let mut seen = std::collections::HashSet::new();
        for seed in 0..64u64 {
            let cell = RiffCell::generate(RiffKind::Deep, &song(seed, 0, 1), 16);
            seen.insert((cell.accents.map(|v| (v * 100.0) as u8), cell.roots));
        }
        assert!(
            seen.len() >= 16,
            "riff barely depends on seed: {}",
            seen.len()
        );
    }

    #[test]
    fn kind_grammar_differs() {
        let deep = RiffCell::generate(RiffKind::Deep, &song(7, 0, 1), 16);
        let club = RiffCell::generate(RiffKind::Club, &song(7, 0, 1), 16);
        let rock = RiffCell::generate(RiffKind::Rock, &song(7, 0, 1), 16);
        assert_ne!(deep.accents, club.accents);
        assert_ne!(club.accents, rock.accents);
    }

    #[test]
    fn first_root_is_tonic() {
        for kind in [
            RiffKind::Deep,
            RiffKind::Club,
            RiffKind::Rock,
            RiffKind::Phonk,
            RiffKind::Proto,
            RiffKind::Lofi,
        ] {
            let cell = RiffCell::generate(kind, &song(9, 0, 1), 16);
            assert_eq!(cell.root(&song(9, 0, 1), 0), 0, "{kind:?} drifts off root");
        }
    }
}
