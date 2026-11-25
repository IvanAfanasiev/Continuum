//! Style ensemble contract.
//!
//! The style does not specify independent probabilities of steps, but rigid relationships
//! between roles, density budget and required execution variation.
//! It will expand as styles are transferred to the contract.

/// Ensemble contract for one style.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StyleContract {
    /// The bass does not attack at the step where the kick plays: it gives way to the foundation.
    pub bass_avoids_kick: bool,
    /// The phrase receives a slight velocity arc: it grows towards the end and exhales.
    pub phrase_velocity_arc: bool,
    /// Maximum melodic onsets per step. 0 = no limit.
    /// Keeps layers distinguishable: saturation is contrast, not a wall of notes.
    pub max_melodic_onsets_per_step: usize,
    /// Upper register limit for melodic roles (MIDI). None = no
    /// limit. Folds notes that creep too high down an octave so that layers do not
    /// crowd into one band and no high-frequency clutter remains.
    pub melodic_ceiling: Option<u8>,
}

impl StyleContract {
    pub const fn new(
        bass_avoids_kick: bool,
        phrase_velocity_arc: bool,
        max_melodic_onsets_per_step: usize,
        melodic_ceiling: Option<u8>,
    ) -> Self {
        Self {
            bass_avoids_kick,
            phrase_velocity_arc,
            max_melodic_onsets_per_step,
            melodic_ceiling,
        }
    }
}
