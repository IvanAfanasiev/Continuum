//! Styles are data-directions, not scores.
//!
//! Each style declares: mode, tempo, swing, instrument palette and
//! one guide [`RoleGuide`] per slot. How notes are born from the weights
//! is decided by the shared engine [`render_style`]: weight 1.0 is a structural
//! genre anchor (almost always plays, priority 0.95; the kick anchor always plays),
//! intermediate weights are probabilities scaled by section energy. No
//! step numbers in the style code.
//!
//! Palette (`Style::ensemble`) - style colors, not a rigid composition:
//! [`Style::cast`] selects a seed cast from it, leaving the foundation,
//! signatures and transition slots. There is also not just one form per style: several
//! written options are selected by the seed ([`Style::arrangement_for_seed`]),
//! and each goes through theme → build-up → drop → post-chorus.
//!
//! The rules set the channel, the seeds determine the water: cast, form, motif, groove and
//! timbre selections change from seed to seed. Short audible repetitions
//! are forbidden by contracts; pieces last 30–60 seconds.
//!
//! Thin mixes: palette - 5-9 slots, each with a unique job.
//! Double texture/second lead “just in case” - in manual slots,
//! rather than into the cast's foundation: density is proven by measurement, not by sensation.

pub mod cast;
pub mod club;
pub mod contract;
pub mod lofi;
pub mod riff;
pub mod transition;

use rand::rngs::StdRng;
use rand::Rng;

use crate::arranger::{SongContext, SongSection};
use crate::core::{base_step_secs, swing_step_secs};
use crate::instruments::{InstrumentRole, NoteEvent, Slot};

/// Style catalog shelf for front sorting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StyleGroup {
    Club,
    Rock,
    Chill,
}

impl StyleGroup {
    /// Canonical name for front and sort.
    pub fn name(self) -> &'static str {
        match self {
            StyleGroup::Club => "club",
            StyleGroup::Rock => "rock",
            StyleGroup::Chill => "chill",
        }
    }

    /// Shelf order in group sorting.
    pub fn order(self) -> usize {
        match self {
            StyleGroup::Club => 0,
            StyleGroup::Rock => 1,
            StyleGroup::Chill => 2,
        }
    }
}

/// Everything the engine needs to generate one step.
pub struct StepContext<'a> {
    pub step: usize,
    pub step_in_measure: usize,
    pub base_step_secs: f32,
    pub active_tracks: &'a [usize],
    pub song: &'a SongContext,
    pub rng: &'a mut StdRng,
    /// Front intensity knob (default 1.0): energy multiplier.
    /// Higher up - denser and brighter, below - thinner and quieter.
    pub intensity: f32,
}

/// How to sound the triggering of the guide.
#[derive(Clone, Copy)]
pub enum Voicing {
    /// Percussion instrument: fixed pitch set (kick 36, snare 38...).
    Kit(u8),
    /// Pitch instrument: scale degree from the current phrase + octave.
    /// `jitter` - wandering through neighboring steps (melodic life),
    /// 0 = strictly hold the step.
    Scale {
        degree: usize,
        octave: i8,
        jitter: usize,
    },
}

/// Single slot guide: trends, not instructions.
///
/// Architecture “role plan → performers” (see vision):
/// `weights` is the ROLE PLAN, common to all its performers
/// (same `&'static` slice for several slots). Single role slots
/// play one rhythmic skeleton, but each with their own voice, their own
/// octave and its own gate roll (the slot is included in the hash): heterophony,
/// not a unison robot. Volumes weigh performers.
/// `alt`/`poles` - individual paints on top of the plan (for now).
///
/// `weights` — bar step weights (length = steps_per_measure style):
/// 1.0 - anchor (almost always plays: priority 0.95, downbeat breathes),
/// 0.0 - never, between - fixed seed gate threshold.
/// Final probability: `w * energy² * energy_response`.
/// Gate NOT dice: a step sounds if the hash(seed, slot, step, phrase) is below the threshold.
/// The same seed - the same flow of phrases; another seed is different.
/// The square of energy keeps the verses sparse and the drop tight.
/// `poles` - a set of 2–4 ready-made grooves (a beat!): the phrase selects one
/// pole deterministically from (seed, slot, phrase). Different phrases - different
/// beat, same seed - same endless stream of grooves.
/// If poles are specified, `weights` is ignored.
#[derive(Clone, Copy)]
pub struct RoleGuide {
    pub weights: &'static [f32],
    pub poles: Option<&'static [&'static [f32]]>,
    pub energy_response: f32,
    /// Off-beat 8ths sound longer (open hats by themselves).
    pub long_offbeats: bool,
    /// A dictionary of note lengths in steps (16's) from which the generation selects
    /// by meter, phrase and character of the seed. This is not a multiplier: length -
    /// the same composition solution as height and pitch. One element -
    /// fixed length (textures, drums), several - live
    /// articulation. The values are non-integer where the gate, not the step, matters:
    /// hat 0.3/1.1 (closed/open), shaker 0.4.
    pub gates: &'static [f32],
    /// Tie on a mono layer: probability that the repeat is the same height while
    /// the note still sounds will be extended, not re-articulated. Applies to
    /// bass (Mono): polyphonic pools add voices themselves.
    /// 0 = always re-attack (staccato pulse).
    pub tie_repeats: f32,
    pub vel_base: f32,
    pub vel_energy: f32,
    pub vel_jitter: f32,
    pub voice: Voicing,
    /// Probability of jumping up an octave (bass overflow).
    pub octave_up: f32,
    /// Register ceiling: jump up does not apply if the note
    /// will exceed it. Keeps the bass down without harsh guidance.
    pub top_pitch: u8,
    /// High Note Muting: Velocity multiplier an octave above MIDI 40.
    /// 1.0 = disabled. Bass at 0.6: unsuccessfully high notes sound quieter,
    /// low - at full strength (classical with drums).
    pub vel_high_cut: f32,
    /// Follow the seed motif: the step is taken from `song.motif`
    /// (the melodic DNA of the track) rather than drifting along the scale.
    /// So the lead plays the seed’s recognizable theme - “a remix of a famous song.”
    pub follow_motif: bool,
    /// Chord intervals in scale degrees (pads/keys play triad).
    /// None + Chords role + no motif = default triad [0, 2, 4].
    /// The melody and drums are always single.
    pub chord: Option<&'static [i8]>,
    /// Fifth bass: offbeat eighths go to fifths - bassline
    /// moves, not tramples. Only the role of Bass, only by flag.
    pub bass_fifth: bool,
    /// Arpeggio: chord intervals are played one at a time in a circular manner
    /// (fingerpicking), not a stack. The interval source is `chord`.
    /// One string at a time instead of a clicking mess of chords.
    pub arp: bool,
    /// Sub-skew 0..1: dampens string/click/presence in favor of clean
    /// sinus Club bass is deep and smooth, rock bass is stringed (0.0).
    /// Only the bass voice reads.
    pub sub_bias: f32,
    /// Bass ceiling: what has crept higher is folded down an octave.
    /// Default 45 (A2, sub). Rock bass riffs with guitar above (52).
    pub bass_top: u8,
    /// Rest with phrases 0..1: the proportion of phrases where the theme carrier breathes more often and
    /// quiet. This is not silence: a rarefied phrase leaves a short pickup
    /// in the last quarter there is a question and answer, not a break.
    /// The roll is independent per slot. 0.0 = always plays.
    pub phrase_rest: f32,
    /// Poles by section: the slot's groove is re-rolled for each section
    /// (Main has its own beat, Drop its own), rather than standing alone for the entire track.
    /// The anchors of the form: the drop is recognizable every time, the verse is its own.
    /// The choice is deterministic (seed, slot, section), and the first measure of the new
    /// sections smoothly flows from the previous palette.
    /// Without the poles, the flag does nothing.
    pub section_poles: bool,
}

impl RoleGuide {
    /// Silent guide: the slot is silent and does not touch the RNG.
    pub const SILENT: Self = Self {
        weights: &[],
        poles: None,
        energy_response: 0.0,
        long_offbeats: false,
        gates: &[1.0],
        tie_repeats: 0.0,
        vel_base: 0.0,
        vel_energy: 0.0,
        vel_jitter: 0.0,
        voice: Voicing::Kit(0),
        octave_up: 0.0,
        top_pitch: 127,
        vel_high_cut: 1.0,
        follow_motif: false,
        chord: None,
        bass_fifth: false,
        sub_bias: 0.0,
        bass_top: 45,
        arp: false,
        phrase_rest: 0.0,
        section_poles: false,
    };

    pub fn is_silent(&self) -> bool {
        // It is silent only if neither no bar weight and no pole sounds:
        // poles are groove, not decoration, and empty weights with poles
        // must play (otherwise the role disappears from the ensemble in silence).
        let weights_silent = self.weights.iter().all(|&w| w <= 0.0);
        let poles_silent = self
            .poles
            .is_none_or(|poles| poles.iter().all(|p| p.iter().all(|&w| w <= 0.0)));
        weights_silent && poles_silent
    }

    /// Drum template: fixed pitch, the rest by default.
    /// Called with overlaps (`..RoleGuide::kit(...)`).
    pub const fn kit(weights: &'static [f32], pitch: u8, vel: f32) -> Self {
        Self {
            weights,
            poles: None,
            energy_response: 1.0,
            long_offbeats: false,
            gates: &[1.0],
            tie_repeats: 0.0,
            vel_base: vel,
            vel_energy: 0.0,
            vel_jitter: 0.05,
            voice: Voicing::Kit(pitch),
            octave_up: 0.0,
            top_pitch: 127,
            vel_high_cut: 1.0,
            follow_motif: false,
            chord: None,
            bass_fifth: false,
            sub_bias: 0.0,
            bass_top: 45,
            arp: false,
            phrase_rest: 0.0,
            section_poles: false,
        }
    }

    /// Pitch preparation: scale level + octave + jitter.
    pub const fn pitched(
        weights: &'static [f32],
        degree: usize,
        octave: i8,
        jitter: usize,
        gates: &'static [f32],
        vel: f32,
    ) -> Self {
        Self {
            weights,
            poles: None,
            energy_response: 1.0,
            long_offbeats: false,
            gates,
            tie_repeats: 0.0,
            vel_base: vel,
            vel_energy: 0.0,
            vel_jitter: 0.05,
            voice: Voicing::Scale {
                degree,
                octave,
                jitter,
            },
            octave_up: 0.0,
            top_pitch: 127,
            vel_high_cut: 1.0,
            follow_motif: false,
            chord: None,
            bass_fifth: false,
            sub_bias: 0.0,
            bass_top: 45,
            arp: false,
            phrase_rest: 0.0,
            section_poles: false,
        }
    }
}

/// Melodic signature of style: how the theme lives inside a two-bar phrase.
/// This is data, not code: onset mask, cell order, attraction to
/// chord and length. Neutral [`Melody::DENSE`] - test path
/// engine: without mask, attraction and stretching, as was the case before phrases.
#[derive(Clone, Copy, Debug)]
pub struct Melody {
    /// 16 eighth-note phrases (2 measures): 1 - theme slot sounds.
    /// Bit 0 is the first slot (downbeat), bit 15 is the last eighth note.
    pub onsets: u16,
    /// The downbeat proportion where the degree extends to the chord's tone.
    /// 0 disables attraction.
    pub chord_snap: f32,
    /// Multiplier of the note length of the theme carrier (1.0 - as in the guide's dictionary).
    pub sustain: f32,
    /// The order of four theme cells (4 steps each) within a phrase.
    /// Repeating the cell is an audible hook, rearranging it is development.
    pub cells: [u8; 4],
    /// Apply a phrase grid: mask, cells and attraction.
    /// False is the neutral path of the engine (a dense line at each step).
    pub phrased: bool,
}

impl Melody {
    /// Neutral melody: dense, without stylistic add-ons.
    pub const DENSE: Self = Self {
        onsets: u16::MAX,
        chord_snap: 0.0,
        sustain: 1.0,
        cells: [0, 1, 2, 3],
        phrased: false,
    };
}

/// Onset mask from the read record: the most significant four bits - the first
/// four-eighth phrases (slots 0..3), minor - the last (12..15).
pub(crate) const fn phrase_mask(bits: u16) -> u16 {
    bits.reverse_bits()
}

/// The mask is not a constant in style: the section and the visit shift the emphasis of the phrase.
/// The drop has its own pattern of onsets, the post-chorus has its own; inside the section
/// the shift lasts for a couple of visits, so the phrase is recognized.
fn phrase_onsets(base: u16, song: &SongContext) -> u16 {
    let shift = match song.section {
        SongSection::BuildUp => 1usize,
        SongSection::Drop => 3,
        SongSection::PostChorus => 2,
        SongSection::MainMotif => 0,
    } + (song.stage / 2) % 4;
    base.rotate_left(shift.min(15) as u32)
}

/// The order of the theme cells for the section and visit. Verse and drop sing basic
/// hook; build-up rearranges the middle, post-chorus repeats the first cell.
/// Then the theme develops in pairs of visits: the hook (cell 0) is, and three
/// response cells are rearranged deterministically from (seed, visit).
/// The seed's DNA - the same four cells - is intact, there are no verbatim repetitions.
fn phrase_cells(base: [u8; 4], song: &SongContext) -> [u8; 4] {
    let mut cells = match song.section {
        SongSection::BuildUp => [base[0], base[2], base[1], base[3]],
        SongSection::PostChorus => [base[0], base[2], base[0], base[3]],
        _ => base,
    };
    let window = song.stage / 2;
    if window > 0 {
        let h = crate::arranger::splitmix64(
            song.seed_hash ^ (window as u64).wrapping_mul(0x5EED_C0DE),
        );
        let a = [cells[1], cells[2], cells[3]];
        // Three permutations of the answer: none leaves the order the same -
        // Development can be heard already on the second couple of visits.
        let answer: [u8; 3] = match h % 3 {
            0 => [a[1], a[2], a[0]],
            1 => [a[2], a[0], a[1]],
            _ => [a[1], a[0], a[2]],
        };
        cells[1] = answer[0];
        cells[2] = answer[1];
        cells[3] = answer[2];
    }
    cells
}

/// Theme step index for phrase slot (0..16): four cells each
/// four steps, the order of the cells is determined by the style and section. `sequence` —
/// sequence by visits: response cells move 0..15 steps,
/// hook (slot 0) is set - the seed's DNA is heard, but the phrases are not cloned.
fn phrase_theme_index(cells: [u8; 4], slot: usize, len: usize, sequence: usize) -> usize {
    let cell = cells[(slot / 4) % cells.len()] as usize;
    let index = cell * 4 + slot % 4;
    if slot == 0 {
        index % len.max(1)
    } else {
        (index + sequence) % len.max(1)
    }
}

/// Harmonic signature of the style for the current seed: progression, tempo
/// harmonies, tonal shifts and density. The style is set by the pool, seed
/// selects an option within the pool - the seeds remain different, but each
/// the style lives in its own harmonic and tonal world.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Harmony {
    /// The mode of style: harmony and transitions cannot diverge from it.
    pub scale: &'static [u8],
    /// Chord progression (scale degrees).
    pub prog: [u8; 4],
    /// Bars per chord: 2 - movement, 4 - breathing, 8 - foundation.
    pub bar_len: usize,
    /// Shifts the entire key of the style, semitones.
    pub key_shift: i8,
    /// Style density multiplier (seed density remains in SongContext).
    pub density: f32,
    /// How the theme lives inside the phrase: mask, cells, attraction.
    pub melody: Melody,
}

impl Harmony {
    /// Style + seed → specific harmony. The style pool is not contractually empty;
    /// just in case, the empty pool is rolled back to the general one.
    fn for_style(style: &dyn Style, seed_hash: u64) -> Self {
        let pool = style.progressions();
        let (prog, bar_len) = if pool.is_empty() {
            let len = prog_bar_len(seed_hash);
            (
                crate::arranger::PROGS[(seed_hash % crate::arranger::PROGS.len() as u64) as usize],
                len,
            )
        } else {
            let rhythm = style.harmonic_rhythm();
            let len = if rhythm.is_empty() {
                prog_bar_len(seed_hash)
            } else {
                // G is separate from the choice of progression: harmony tempo and
                // the move itself should not be made in pairs (otherwise 4 progressions ×
                // 2 tempos give a total of 8 combinations).
                let h = crate::arranger::splitmix64(seed_hash ^ 0x4841_524D);
                rhythm[(h % rhythm.len() as u64) as usize]
            };
            let h = crate::arranger::splitmix64(seed_hash ^ 0x5052_4F47);
            (pool[(h % pool.len() as u64) as usize], len.max(1))
        };
        Self {
            scale: style.scale(),
            prog,
            bar_len,
            key_shift: style.key_shift(),
            density: style.density().clamp(0.5, 1.5),
            melody: style.melody(),
        }
    }

    /// Neutral Harmony for Guide Tests: Exactly What
    /// lies in `SongContext`, without style add-ons. Lad not here
    /// included: `play_guide` receives it as a separate argument, field
    /// `scale` reads only the transition (the test sets it explicitly there).
    #[cfg(test)]
    fn neutral(song: &SongContext) -> Self {
        Self {
            scale: &[],
            prog: song.prog,
            bar_len: prog_bar_len(song.seed_hash),
            key_shift: 0,
            density: 1.0,
            melody: Melody::DENSE,
        }
    }
}

pub trait Style: Send + Sync {
    fn name(&self) -> &str;
    /// Catalog shelf for sorting front: Club (house, techno, trance,
    /// dubstep, drum and bass, electro, rave), Rock (rock, hard, metal),
    /// Chill (calm, like lofi). Default - Club.
    fn group(&self) -> StyleGroup {
        StyleGroup::Club
    }
    /// Ensemble politics of style: rigid relations between roles and
    /// mandatory variation of execution. By default it doesn't change anything
    /// so that other styles behave as before.
    fn style_contract(&self) -> contract::StyleContract {
        contract::StyleContract::default()
    }
    /// Default temp. The front can override it with a live tempo knob.
    fn bpm(&self) -> f32;
    fn steps_per_measure(&self) -> usize;
    fn swing_ratio(&self) -> f32;
    fn scale(&self) -> &'static [u8];
    /// Harmonic progressions of style: shifts of scale degrees. seed
    /// selects one from the pool, so the accompaniment is not repeated
    /// from seed to seed, but moves in the character of the style (club - i-VI-III-VII,
    /// lo-fi - soft ii/iv-moves, rock - circle of fifths, etc.).
    fn progressions(&self) -> &'static [[u8; 4]] {
        &crate::arranger::PROGS
    }
    /// Harmonic rhythm of the style: how many bars the chord lasts.
    /// seed selects an option from the pool. 2 - live movement, 8 - drone.
    fn harmonic_rhythm(&self) -> &'static [usize] {
        &[2, 4, 8]
    }
    /// Style key shift in semitones: the same seed in
    /// different styles sound in different keys (C, F, D, E...).
    fn key_shift(&self) -> i8 {
        0
    }
    /// Style density: multiplier to seed density. Rock thicker, lofi
    /// more rarefied - with the same seed and the same form.
    fn density(&self) -> f32 {
        1.0
    }
    /// Melodic signature of the style: phrase onset mask, cell order,
    /// the pull of strong beats to the chord and the length of the theme. Default -
    /// dense neutral line: the style without a signature plays as before.
    fn melody(&self) -> Melody {
        Melody::DENSE
    }
    /// Basic style score: his face in shape. seed chooses an option
    /// from [`Style::arrangement_for_seed`], so the pieces are of the same style
    /// differ in dramaturgy, and not just in theme.
    fn arrangement(&self) -> crate::arranger::Score {
        crate::arranger::ENERGETIC_SCORE
    }
    /// Shape of the piece for the seed: 2–3 written options per style. All
    /// options go through the theme → build-up → hit → post-chorus; order and
    /// lengths are different. Default - basic score.
    fn arrangement_for_seed(&self, _seed: u64) -> crate::arranger::Score {
        self.arrangement()
    }
    /// Style arrangement: who plays in the theme, build-up, drop and post-chorus.
    /// The order is [`crate::piece::LAYER_ORDER`]. This is the written arrangement:
    /// the club keeps its riff in the post-chorus,
    /// in lofi the pad keeps the theme. Default - General Arr.
    fn arrangement_layers(&self) -> crate::piece::Layers {
        crate::piece::default_layers()
    }
    /// How style glues pieces together: smooth overlap (voice of the next
    /// comes in over the groove) or a sharp cut at the edge of a bar. Energetic
    /// styles roll the groove through the junction, calm ones close quietly.
    fn transition(&self) -> crate::piece::Transition {
        crate::piece::Transition::Rolling { bars: 2 }
    }
    /// Musical entry from build-up to drop: each style has its own gesture.
    /// `None` - no gesture, a regular groove at the junction.
    fn transition_gesture(&self) -> Option<transition::Spec> {
        None
    }
    /// Riff identity: style mask of accents/roots from seed.
    /// By default there is no riff - the style works on the same guides.
    fn riff_kind(&self) -> riff::RiffKind {
        riff::RiffKind::None
    }
    /// Style instrument palette: paints from which it is assembled
    /// compound. Not all palette slots sound at the same time - specific
    /// set selects seed ([`Style::cast`]). The volumes are canonical,
    /// slots([`Slot::base_volume`]); The preset does not hardcode the numbers.
    fn ensemble(&self) -> &'static [Slot];
    /// Signature instruments: without them, the style loses face (riff guitar
    /// rock, Rhodes lo-fi). The cast always includes them. Default - no
    /// mandatory beyond the foundation.
    fn signature_slots(&self) -> &'static [Slot] {
        &[]
    }
    /// Seed composition: subset of the palette plus signature instruments
    /// and transition slots. Determined; manually editing the mix in the mixer
    /// still more important.
    fn cast(&self, seed: u64) -> Vec<Slot> {
        let required: Vec<Slot> = self
            .transition_gesture()
            .map(|spec| {
                spec.fill_slots
                    .iter()
                    .chain(spec.tail_slots.iter())
                    .copied()
                    .collect()
            })
            .unwrap_or_default();
        cast::from_palette(
            self.ensemble(),
            seed,
            self.name(),
            self.signature_slots(),
            &required,
        )
    }
    /// Slot guides are in order [`Slot::ALL`].
    fn slot_guides(&self) -> [RoleGuide; Slot::COUNT];
    /// Reverb room: fraction wet (0 = dry, like a club hit on an array).
    fn reverb_wet(&self) -> f32 {
        0.16
    }
    /// Room size 0..1 (room → cathedral).
    fn reverb_size(&self) -> f32 {
        0.5
    }
    /// Master high cutoff, Hz. Lo-fi is muddy, the club is surgical.
    fn treble_cutoff(&self) -> f32 {
        19000.0
    }
    /// Energy Shape: Maps section energy to operating density.
    /// A flat curve (lofi) removes explosive buildups/genre drops,
    /// and not prohibitions in the code.
    fn shape_energy(&self, energy: f32) -> f32 {
        energy
    }
    /// Sidechain depth 0..2 (pump multiplier): rock presses hard,
    /// lofi can barely breathe. Default 1.0 is the nature of the engine.
    fn sidechain_depth(&self) -> f32 {
        1.0
    }

    fn base_step_secs(&self) -> f32 {
        base_step_secs(self.bpm(), self.steps_per_measure())
    }

    fn swing_duration(&self, step_in_measure: usize) -> f32 {
        swing_step_secs(self.base_step_secs(), self.swing_ratio(), step_in_measure)
    }
}

/// General generation engine: one slot cycle for all styles.
/// A slot sounds only if its id is in active tracks (volume > 0) -
/// disabled instruments do not load either the generation or the DSP.
/// Composition affects generation: slots of the same role share density
/// (`1/sqrt(n)` for non-anchor probabilities) - turned off the second lead,
/// the remaining one plays thicker and picks up the space. Genre anchors
/// (weight 1.0: beat, foundation) cannot be divided - the foundation is not interrupted.
/// The motif does not disappear from mute: if all theme carriers are turned off, it
/// picks up the first melodic pitch slot (Lead→Chords→Bass).
/// Harmony is not lost due to mute: if all chords are turned off
/// (pads/keys), leads and chops are condensed into dyads (root + third) -
/// harmonized melody instead of emptiness. We do not cover bass and kick:
/// turned off bottom - the will of the user, not a hole (and someone else's articulation
/// If you don’t replace it, dirt will come out, not the foundation).
/// Determined by the composition: the same seed + the same set = the same music.
/// Render order: kicks first (the foundation announces the transition),
/// then the rest - the bass already knows whether it’s a downbeat or a transition gesture.
pub fn render_style(style: &dyn Style, sctx: &mut StepContext) -> Vec<NoteEvent> {
    let guides = style.slot_guides();
    let scale = style.scale();
    let energy = (style.shape_energy(sctx.song.energy) * sctx.intensity).clamp(0.0, 1.5);
    let policy = style.style_contract();
    // Style sets the harmonic world: progression, tempo of harmony and
    // key. seed chooses an option within the style, not on top of it.
    let harmony = Harmony::for_style(style, sctx.song.seed_hash);
    // Seed Identity: One riff mask per phrase, shared by
    // bass/chords/theme carriers. Not a layer, but a paint of existing ones.
    let riff = riff::RiffCell::generate(style.riff_kind(), sctx.song, style.steps_per_measure());
    // Style transition: gesture of the last build-up measure and its tail.
    // The transition notes come first (the kick manages to announce the foundation),
    // and the slots that the gesture owns do not play the usual game.
    let mut transition = transition::render(
        style.transition_gesture(),
        sctx.song,
        sctx.step_in_measure,
        style.steps_per_measure(),
        sctx.base_step_secs,
        sctx.active_tracks,
        &harmony,
    );
    // How many real-sounding carriers does each role have - division map
    // density. Silent guides (zero weights and poles) do not count:
    // otherwise, live performers share density with emptiness.
    let mut role_n = [0usize; 8];
    for (i, s) in Slot::ALL.iter().enumerate() {
        if sctx.active_tracks.contains(&s.id()) && !guides[i].is_silent() {
            role_n[s.role().index()] += 1;
        }
    }
    let is_active = |s: Slot| sctx.active_tracks.contains(&s.id());
    // The theme is alive as long as at least one of its bearers is alive.
    let has_carrier = Slot::ALL
        .iter()
        .enumerate()
        .any(|(i, s)| is_active(*s) && guides[i].follow_motif && !guides[i].is_silent());
    // Harmony is alive as long as at least one chord slot is alive.
    let has_chords = Slot::ALL.iter().enumerate().any(|(i, s)| {
        is_active(*s) && Slot::ALL[i].role() == InstrumentRole::Chords && !guides[i].is_silent()
    });
    // Backup carrier: first pitch melodic by role rank.
    // The drums cannot sing the theme - only Scale voices.
    let fallback: Option<usize> = if has_carrier {
        None
    } else {
        Slot::ALL
            .iter()
            .enumerate()
            .filter(|(i, s)| {
                is_active(**s)
                    && !guides[*i].is_silent()
                    && matches!(guides[*i].voice, Voicing::Scale { .. })
                    && carrier_rank(s.role()).is_some()
            })
            .min_by_key(|(_, s)| (carrier_rank(s.role()).unwrap_or(99), s.id()))
            .map(|(_, s)| s.id())
    };
    let mut notes = std::mem::take(&mut transition.notes);
    // Theme carriers at this step: active followers + pick-up.
    // The leader leads the phrase in full, the rest - echo (see play_guide):
    // for the first time the theme has a first voice rather than a choir of soloists.
    let mut carriers: Vec<usize> = Slot::ALL
        .iter()
        .enumerate()
        .filter(|(i, s)| is_active(**s) && guides[*i].follow_motif && !guides[*i].is_silent())
        .map(|(_, s)| s.id())
        .collect();
    if carriers.is_empty() {
        if let Some(fb) = fallback {
            carriers.push(fb);
        }
    }
    let leader = theme_leader(&carriers, sctx.song.seed_hash, sctx.song.stage);
    // Single pool of melodic voices for volume balance:
    // active and real sounding (not SILENT).
    let active_melodic: Vec<usize> = Slot::ALL
        .iter()
        .enumerate()
        .filter(|(i, s)| is_active(**s) && !guides[*i].is_silent() && is_melodic_role(s.role()))
        .map(|(_, s)| s.id())
        .collect();
    // Phase 1: kicks. The foundation is the first thing so that the DSP duck on the kick drum is ready
    // to the moment of notes of the remaining layers (the order is important for the engine, not the generation).
    for (i, slot) in Slot::ALL.iter().enumerate() {
        if slot.role() != InstrumentRole::Kick {
            continue;
        }
        if !sctx.active_tracks.contains(&slot.id()) {
            continue;
        }
        // Arrange by layers: the plan of the piece writes who plays in this section.
        // A silent group is not “quieter”, but silent: points on the wheel and sound
        // coincide because the solution is the same.
        if sctx.song.layers & (1 << slot.group().index()) == 0 {
            continue;
        }
        let guide = &guides[i];
        if guide.is_silent() {
            continue;
        }
        // The transition owns the slot at this step: its note is already in `notes`.
        if transition.owns(*slot) {
            continue;
        }
        let n = role_n[slot.role().index()].max(1) as f32;
        let slot_notes = play_guide_with_riff(
            guide,
            scale,
            sctx.song,
            energy,
            sctx.step_in_measure,
            style.steps_per_measure(),
            sctx.base_step_secs,
            slot.id(),
            i,
            1.0 / n.sqrt(),
            fallback == Some(slot.id()),
            false,
            // Kiki don’t carry the theme: they don’t need leadership.
            false,
            &harmony,
            &riff,
            sctx.rng,
        );
        notes.extend(slot_notes);
    }
    // Kick mask at this step: bass and other roles may give way to the foundation
    // (ensemble politics) rather than fighting it.
    let kick_this_step = notes.iter().any(|n| {
        Slot::from_id(n.instrument_id)
            .map(|s| s.role() == InstrumentRole::Kick)
            .unwrap_or(false)
    });
    // Melodic onset counter per step: contract density budget.
    let mut melodic_onsets = 0usize;

    // Phase 2: the rest. The order inside is stable (Slot::ALL).
    for (i, slot) in Slot::ALL.iter().enumerate() {
        if slot.role() == InstrumentRole::Kick {
            continue;
        }
        if !sctx.active_tracks.contains(&slot.id()) {
            continue;
        }
        // Arrange by layers: the plan of the piece writes who plays in this section.
        if sctx.song.layers & (1 << slot.group().index()) == 0 {
            continue;
        }
        let guide = &guides[i];
        if guide.is_silent() {
            continue;
        }
        // The transition owns the slot at this step: its note is already in `notes`.
        if transition.owns(*slot) {
            continue;
        }
        // Ensemble Policy: The bass gives way to the kick at this step.
        if slot.role() == InstrumentRole::Bass && policy.bass_avoids_kick && kick_this_step {
            continue;
        }
        // There is no morphing: the pole is chosen by the phrase, the density is chosen by energy.
        // The drawing inside the section is stable, it is re-dealt between phrases.
        let n = role_n[slot.role().index()].max(1) as f32;
        // Condensation into dyads - only melodic pitching when empty
        // chords. The rest play as written.
        let melodic = is_melodic_role(slot.role());
        // Contract density budget: no more than N melodic onsets per step.
        if melodic
            && policy.max_melodic_onsets_per_step > 0
            && melodic_onsets >= policy.max_melodic_onsets_per_step
        {
            continue;
        }
        let thicken = !has_chords
            && matches!(slot.role(), InstrumentRole::Lead)
            && matches!(guide.voice, Voicing::Scale { .. });
        let mut slot_notes = play_guide_with_riff(
            guide,
            scale,
            sctx.song,
            energy,
            sctx.step_in_measure,
            style.steps_per_measure(),
            sctx.base_step_secs,
            slot.id(),
            i,
            // Theme carriers divide the space more softly: their grids are shifted
            // on 1–2 eighth notes (echo), and do not coincide, so it is hard
            // 1/sqrt(n) made the theme too quiet in rock/lo-fi.
            if guide.follow_motif {
                1.0 / n.powf(0.35)
            } else {
                1.0 / n.sqrt()
            },
            fallback == Some(slot.id()),
            thicken,
            // The leader of the phrase is in full force, the rest of the carriers are in echo.
            leader == Some(slot.id()),
            &harmony,
            &riff,
            sctx.rng,
        );
        // Register strip of the contract: fold the notes that have crawled down.
        if melodic {
            if let Some(ceiling) = policy.melodic_ceiling {
                for note in &mut slot_notes {
                    while note.midi_pitch > ceiling {
                        note.midi_pitch = note.midi_pitch.saturating_sub(12);
                    }
                }
            }
            melodic_onsets += slot_notes.len();
        }
        notes.extend(slot_notes);
    }
    // Mandatory performance variation: light velocity arch of the phrase.
    // The drawing is the same, but the phrase breathes - not a flat shelf.
    if policy.phrase_velocity_arc {
        let arc = phrase_arc(sctx.song.phrase_index);
        for note in &mut notes {
            note.velocity = (note.velocity * arc).clamp(0.1, 1.0);
        }
    }

    // A single balance of parts: the leader of the theme (and if the theme is not carried by the melodic
    // voice - selected melodic) a little louder, the rest are melodic
    // - support. There is no longer a second, independent leader in the composer:
    // The hierarchy is the same for theme echo and volume balance.
    apply_leader_balance(
        &mut notes,
        balance_leader(&active_melodic, leader, sctx.song.stage),
    );
    notes
}

/// Melodic roles are those that sound in the upper register: lead and chords.
/// The bass and drums hold the foundation and do not participate in the balance.
fn is_melodic_role(role: InstrumentRole) -> bool {
    matches!(role, InstrumentRole::Lead | InstrumentRole::Chords)
}

/// Balance leader: leader of the theme if it is melodic; otherwise -
/// deterministic rotation according to active melodic voices.
/// Less than two votes - there is nothing to balance (we don’t push one).
fn balance_leader(
    active_melodic: &[usize],
    theme_lead: Option<usize>,
    stage: usize,
) -> Option<usize> {
    if active_melodic.len() < 2 {
        return None;
    }
    theme_lead
        .filter(|l| active_melodic.contains(l))
        .or_else(|| Some(active_melodic[stage % active_melodic.len()]))
}

/// Intentionally discreet balance: generation does not argue with handles
/// user, only slightly illuminated by the leading voice.
const LEADER_BOOST: f32 = 1.08;
const SUPPORT_DIP: f32 = 0.95;

fn apply_leader_balance(notes: &mut [NoteEvent], leader: Option<usize>) {
    let Some(lead_id) = leader else { return };
    for note in notes.iter_mut() {
        let Some(role) = Slot::from_id(note.instrument_id).map(|s| s.role()) else {
            continue;
        };
        if !is_melodic_role(role) {
            continue;
        }
        if note.instrument_id == lead_id {
            note.velocity = (note.velocity * LEADER_BOOST).min(1.0);
        } else {
            note.velocity *= SUPPORT_DIP;
        }
    }
}

/// Role rank for the backup theme carrier: the leader sings first,
/// bass - last (holds the bottom, not the melody). `None` - role
/// cannot sing the theme (drums, textures).
fn carrier_rank(role: InstrumentRole) -> Option<u8> {
    match role {
        InstrumentRole::Lead => Some(0),
        InstrumentRole::Chords => Some(2),
        InstrumentRole::Bass => Some(3),
        _ => None,
    }
}

/// Position of a beat within a musical phrase (phrase = two measures).
fn phrase_position(song: &SongContext, step_in_measure: usize, steps_per_measure: usize) -> usize {
    let bar_in_phrase = song.measure_index.saturating_sub(1) % 2;
    bar_in_phrase * steps_per_measure + step_in_measure
}

/// Phrase grid slot (0..16): two eighth notes per step. The theme mask lives
/// in eighth notes, so the 16th beat falls into the same slot as its neighbor.
fn phrase_slot(song: &SongContext, step_in_measure: usize, steps_per_measure: usize) -> usize {
    phrase_position(song, step_in_measure, steps_per_measure) / 2
}

/// Pull the degree to the nearest tone of the current chord: downbeat
/// keeps harmony, and does not fly by. We move no more than
/// step and do not go beyond the outline of the theme: the passing notes and the arch are intact.
fn snap_to_chord(
    degree: usize,
    chord_root: usize,
    scale_len: usize,
    contour_max: usize,
) -> usize {
    if scale_len == 0 || degree > contour_max {
        return degree;
    }
    let mut best = degree;
    let mut best_dist = usize::MAX;
    // Triad plus seventh (in pentatonic scale - its equivalent: none):
    // strong beats extend not only to 1/3/5, but also to the soft 7th -
    // The cadence sounds like a move, not like an empty interval.
    let upper = if scale_len >= 7 { 6usize } else { 5usize };
    for interval in [0usize, 2, 4, upper] {
        let target = (chord_root + interval) % scale_len;
        if target > contour_max {
            continue;
        }
        let dist = target.abs_diff(degree);
        if dist < best_dist {
            best_dist = dist;
            best = target;
        }
    }
    if best_dist <= 1 {
        best
    } else {
        degree
    }
}

/// Phrase turn zone: the last quarter of a two-bar phrase.
/// Here the bearer of the theme, which “breathes,” leaves a short pickup.
fn phrase_turn_zone(steps_per_measure: usize, song: &SongContext, step_in_measure: usize) -> bool {
    let zone_start = 2 * steps_per_measure - steps_per_measure / 4;
    phrase_position(song, step_in_measure, steps_per_measure) >= zone_start
}

/// Anchor priority: even the downbeat sometimes breathes (1 in 20 hits
/// passed by the seed gate). The face of the genre is kept statistically,
/// The absolute “always sounds” is nowhere else to be found.
const ANCHOR_PRIOR: f32 = 0.95;
/// Anchor gate salt: an independent roll from the rest of the gates in the slot.
const ANCHOR_SALT: u64 = 0xA11CE;
/// Theme density with a phrase-mask eighth turned off (was 0 -
/// the theme has fallen silent, now it is a quiet passage to the next accent).
const MASK_OFF_DENSITY: f32 = 0.70;
/// The theme density is in sixteenths between the onsets of the mask.
const MASK_SUBDIV_DENSITY: f32 = 0.60;

/// Step weight taking into account the track pole: the beat is fixed by pole selection;
/// without poles — by bar weights. The `section_poles` flag mixes the section
/// into the selection salt: the groove is re-dealt per section (the drop has its own beat).
/// The first bar of the new section is a crossfade of palettes: the old beat is not
/// breaks off and flows into a new one at the end of the measure.
fn pole_step_weight(
    guide: &RoleGuide,
    song: &SongContext,
    key: usize,
    step_in_measure: usize,
    steps_per_measure: usize,
) -> f32 {
    let base_w = guide.weights.get(step_in_measure).copied().unwrap_or(0.0);
    let Some(poles) = guide.poles.filter(|p| !p.is_empty()) else {
        return base_w;
    };
    let current = pole_for(
        poles,
        song,
        key,
        step_in_measure,
        guide.section_poles.then_some(song.section),
    );
    if guide.section_poles
        && song.section_measure_index == 1
        && song.previous_section != song.section
    {
        let previous = pole_for(
            poles,
            song,
            key,
            step_in_measure,
            Some(song.previous_section),
        );
        let t = step_in_measure as f32 / steps_per_measure.saturating_sub(1).max(1) as f32;
        previous * (1.0 - t) + current * t
    } else {
        current
    }
}

/// Pole for a specific section: salt - (seed, slot, section).
fn pole_for(
    poles: &[&[f32]],
    song: &SongContext,
    key: usize,
    step_in_measure: usize,
    section: Option<SongSection>,
) -> f32 {
    let mut salt = song.seed_hash ^ (key as u64).wrapping_mul(0x9E3779B9);
    if let Some(section) = section {
        salt ^= (section as u64).wrapping_mul(0x5EED);
    }
    let pick = crate::arranger::splitmix64(salt) % poles.len() as u64;
    poles[pick as usize]
        .get(step_in_measure)
        .copied()
        .unwrap_or(0.0)
}

/// Decay of chord tones: the root is at full strength, the top is quieter.
const CHORD_ATTEN: [f32; 4] = [1.0, 0.75, 0.65, 0.6];

/// Accent priority is uniform: every slot participates in section visits.
fn accent_prior(_instrument_id: usize) -> f32 {
    1.0
}

/// Chromatic tonic shift from the seed (−2..=+2 semitones).
/// The style specifies the mode (a family of intervals), the seed specifies the absolute height:
/// different seeds sound in different keys. The range is deliberately narrow:
/// voice registers are calculated with a reserve (the bass does not go into infrasound,
/// leads do not go beyond D6). Completely determined by the seed.
pub(crate) fn tonic_shift_for(seed_hash: u64) -> i8 {
    (crate::arranger::splitmix64(seed_hash ^ 0x701C) % 5) as i8 - 2
}

/// Octave of the theme from the seed (−1/0/+1 for the entire track): one seed sings
/// high, the other - tenor. Goes with the whole theme (motif and pickup
/// together), rather than with a separate slot. Completely determined.
pub(crate) fn motif_octave(seed_hash: u64) -> i8 {
    (crate::arranger::splitmix64(seed_hash ^ 0x0C7A) % 3) as i8 - 1
}

/// Leader of the theme for the section visit: one medium leads to the full,
/// the rest echo. Rotation is determined (seed, visit).
/// Frozen per section: the first vote is held for the entire section of the form
/// and changes only at the border (the composition is not shuffled within the song).
/// It’s empty - there’s no one to lead (everyone is silent as written).
pub(crate) fn theme_leader(carriers: &[usize], seed_hash: u64, stage: usize) -> Option<usize> {
    if carriers.is_empty() {
        return None;
    }
    let h =
        crate::arranger::splitmix64(seed_hash ^ 0x1EAD ^ (stage as u64).wrapping_mul(0x9E3779B9));
    Some(carriers[(h % carriers.len() as u64) as usize])
}

/// Echo share: the one who echoes keeps so many of his steps for himself.
/// A shared roll for all echoing carriers - the echo sounds together. Echoing grid
/// shifted by 1–2 eighth notes (see `echo_shift`), so this is an imitation,
/// not unison; 0.95 keeps the second vote almost next to the leader.
pub(crate) const LEAD_ECHO: f32 = 0.95;

/// Seed swing: 0.93..1.07 multiplier to style swing - for each seed
/// its own quality (audible!), the groove doesn’t fall apart (±10% was given by drunk
/// sixteenths). Deterministic.
pub(crate) fn swing_var(seed_hash: u64) -> f32 {
    let h = crate::arranger::splitmix64(seed_hash ^ 0x5716);
    0.93 + 0.14 * ((h >> 32) as f32 / (u32::MAX as f32))
}

/// Modal from seed: Dorian sixth / Phrygian second /
/// blues note. The same range with a different character.
///
/// Alterations are only enabled where they add a new color, not
/// they break the harmony already chosen by the style: the Phrygian second is not “corrected”
/// to natural minor, and the Dorian sixth does not rise to Phrygian
/// scale with high sixth. Otherwise, the style would lose its scale on some of the seeds.
fn modal_tone(scale: &[u8], idx: usize, seed_hash: u64) -> u8 {
    let root = scale[idx];
    let natural_minor = scale.len() == 7
        && scale[1].saturating_sub(scale[0]) == 2
        && scale[5].saturating_sub(scale[4]) == 1;
    match (scale.len(), (seed_hash % 3) as u8) {
        (7, 1) if idx == 5 && natural_minor => root.saturating_add(1),
        (7, 2) if idx == 1 && natural_minor => root.saturating_sub(1),
        (5, 1) if idx == 3 => root.saturating_add(1),
        _ => root,
    }
}
/// Deterministic probability gate: same (seed, slot, step, salt) -
/// same solution. We take the salt from a monotonous phrase (increases every 2 measures),
/// therefore, the groove holds the phrase and is re-dealt further: there are no repetitions,
/// but the same seed always gives the same infinite stream (reproducibility).
/// Unlike the dice for each step, the chosen one plays accurately.
fn fixed_gate(seed_hash: u64, slot_idx: usize, step: usize, salt: u64) -> f32 {
    let h = crate::arranger::splitmix64(
        seed_hash
            ^ (slot_idx as u64).wrapping_mul(0x9E3779B9)
            ^ (step as u64).wrapping_mul(0x85EBCA6B)
            ^ salt,
    );
    (h >> 32) as f32 / (u32::MAX as f32)
}

/// Seed Articulation: Attack/release multipliers for soft voices.
/// Static on (seed, slot) - “this seed is softer”, not bumpy inside
/// track. The range is narrow: the character of the instruments is intact, the seeds are distinguishable.
/// Shock fields are ignored (their transient is the face of the genre).
pub(crate) fn envelope_shape(seed_hash: u64, slot_idx: usize) -> (f32, f32) {
    let h = crate::arranger::splitmix64(
        seed_hash ^ (slot_idx as u64).wrapping_mul(0xC0FFEE) ^ 0xA771C04,
    );
    let hi = (h >> 32) as f32 / (u32::MAX as f32);
    let lo = (h & 0xFFFF_FFFF) as f32 / (u32::MAX as f32);
    (0.8 + 1.0 * hi, 0.8 + 0.8 * lo)
}

/// Articulation character of the seed 0.85..1.25, static on (seed, slot):
/// Some seeds are drawn-out, others are abrupt. This is the Length SELECT offset
/// (see `choose_gate`), not a multiplier: the length itself remains musical
/// value from the guide's dictionary.
pub(crate) fn duration_shape(seed_hash: u64, slot_idx: usize) -> f32 {
    let h =
        crate::arranger::splitmix64(seed_hash ^ (slot_idx as u64).wrapping_mul(0xC0FFEE) ^ 0xD0A4);
    0.85 + 0.4 * ((h >> 32) as f32 / (u32::MAX as f32))
}

/// Metric step force 0..1: downbeat - 1, quarters - 0.65,
/// eighth notes - 0.35, sixteenth notes - 0. On a downbeat the note stretches,
/// on a weak one - in short: the phrase breathes, and does not chop in even sixteenths.
fn metric_strength(step_in_measure: usize, steps_per_measure: usize) -> f32 {
    let q = (steps_per_measure / 4).max(1);
    if step_in_measure.is_multiple_of(steps_per_measure.max(1)) {
        1.0
    } else if step_in_measure.is_multiple_of(q) {
        0.65
    } else if step_in_measure.is_multiple_of((q / 2).max(1)) {
        0.35
    } else {
        0.0
    }
}

/// The note length in steps is a generation decision, not a timbre constant.
///
/// The guide's dictionary specifies the range; meter, phrase, section energy and character
/// the seed chooses a point within it. The choice is a quantile, not an exact value:
/// on a strong beat, a long note is more likely, on a weak beat, a short note.
/// Jitter is deterministic (seed, slot, position in phrase): length pattern
/// repeats the phrase rather than flickering every measure. Drums and textures
/// have a vocabulary of one element and play in a fixed manner.
fn choose_gate(
    guide: &RoleGuide,
    song: &SongContext,
    slot_idx: usize,
    step_in_measure: usize,
    steps_per_measure: usize,
    motif: bool,
    riff_bias: f32,
) -> f32 {
    let gates = guide.gates;
    let Some(&last) = gates.last() else {
        return 1.0;
    };
    if gates.len() == 1 {
        return last;
    }
    // Off-beat eighth notes drag on: open hi-hat by itself.
    if guide.long_offbeats {
        let eighth = (steps_per_measure / 8).max(1);
        if step_in_measure % (eighth * 2) == eighth {
            return last;
        }
    }
    // Cadence: the last quarter of the last measure of the section is long
    // note. The section ends with a full stop rather than a break on the downbeat.
    let q = (steps_per_measure / 4).max(1);
    if song.is_last_measure && step_in_measure + q >= steps_per_measure {
        return last;
    }
    // Character of the seed: 0 - abrupt, 1 - drawn-out.
    let legato = ((duration_shape(song.seed_hash, slot_idx) - 0.85) / 0.4).clamp(0.0, 1.0);
    // The pattern of lengths develops: for accompaniment - from phrase to phrase,
    // the theme - from section to section. Within the phrase/section the motif holds
    // its own rhythm, but the same measure does not sound for years at one length.
    let variation = if motif { song.stage } else { song.phrase_index };
    // Non-identical repeats: flickering shift by bar number. One variation
    // per measure, not per step — that is how it sings “the same text, played differently.”
    let repeat = song.measure_index.saturating_sub(1);
    let pos = phrase_position(song, step_in_measure, steps_per_measure)
        .wrapping_add(variation.wrapping_mul(131))
        .wrapping_add(repeat.wrapping_mul(7));
    let jitter = fixed_gate(song.seed_hash, slot_idx, pos, 0x6A7E) - 0.5;
    let strength = metric_strength(step_in_measure, steps_per_measure);
    // Calm sections draw out notes, drops are shorter and sharper.
    let calm = 1.0 - song.energy.clamp(0.0, 1.0);
    let qpos = (0.08 + 0.50 * strength + 0.20 * legato + 0.10 * calm + 0.50 * jitter + riff_bias)
        .clamp(0.0, 0.999);
    // The last gate for long_offbeats is an open hat: it belongs to
    // only offbeat, otherwise open hats will scatter over weak beats.
    let choice_len = if guide.long_offbeats {
        (gates.len() - 1).max(1)
    } else {
        gates.len()
    };
    let idx = (qpos * (choice_len - 1) as f32).round() as usize;
    gates[idx.min(choice_len - 1)]
}

/// Tie roll: deterministic (seed, slot, step). The same solution
/// same seed, but the tie pattern is lively, and not “every second” mechanically.
pub(crate) fn tie_chance(seed_hash: u64, slot_idx: usize, step: usize) -> f32 {
    fixed_gate(seed_hash, slot_idx, step, 0x71E5)
}

/// The usual path without a riff: style without identity and isolation tests.
#[cfg(test)]
#[allow(clippy::too_many_arguments)]
fn play_guide(
    guide: &RoleGuide,
    scale: &[u8],
    song: &SongContext,
    energy: f32,
    step_in_measure: usize,
    steps_per_measure: usize,
    base_step_secs: f32,
    instrument_id: usize,
    slot_idx: usize,
    role_share: f32,
    force_motif: bool,
    force_chords: bool,
    theme_lead: bool,
    rng: &mut StdRng,
) -> Vec<NoteEvent> {
    play_guide_with_riff(
        guide,
        scale,
        song,
        energy,
        step_in_measure,
        steps_per_measure,
        base_step_secs,
        instrument_id,
        slot_idx,
        role_share,
        force_motif,
        force_chords,
        theme_lead,
        &Harmony::neutral(song),
        &riff::RiffCell::NONE,
        rng,
    )
}

/// A line of one slot per step: groove, articulation and pitch.
/// `riff` is a general seed mask: it does not sound itself, but recolors
/// probability, length, loudness and harmonic roots of the carriers.
#[allow(clippy::too_many_arguments)]
fn play_guide_with_riff(
    guide: &RoleGuide,
    scale: &[u8],
    song: &SongContext,
    energy: f32,
    step_in_measure: usize,
    steps_per_measure: usize,
    base_step_secs: f32,
    instrument_id: usize,
    slot_idx: usize,
    // Role density fraction (1/sqrt(n) media): 1.0 = single
    // carrier, plays at full strength. Anchors (weight 1.0) are not scaled.
    role_share: f32,
    // Forced theme carrier: composition without follow_motif - step
    // taken from the seed motif. Voice acting (pad triads) and guide octave
    // are preserved: the pad plays the chord from the theme step, not the melody.
    force_motif: bool,
    // Condensation into a dyad: empty chords - the lead sings the root + third.
    // Harmony without the muddiness of triads and without changing the voice.
    force_chords: bool,
    // The leader of the theme on this phrase: leads to the full. Other media -
    // echo (reduced density, same steps). Tests are called with true:
    // the voice in isolation sounds as written.
    theme_lead: bool,
    harmony: &Harmony,
    riff: &riff::RiffCell,
    rng: &mut StdRng,
) -> Vec<NoteEvent> {
    // Step weight: track pole or bar weights. No morphing or substitutions:
    // The density of a section is determined only by energy and structure.
    // The slot role is needed early (general plan and kick anchors): pure id function.
    let role = Slot::from_id(instrument_id).map(|s| s.role());
    // Thematic medium: native follow_motif or line-up pick-up.
    // One flag for all step decisions: mask, gate, echo, length.
    let use_motif = guide.follow_motif || force_motif;
    // Cadance: The last step of the last measure of a section. It always sounds
    // and comes to the tonic - the section closes, not ends.
    let cadence = use_motif && song.is_last_measure && step_in_measure + 1 == steps_per_measure;
    // The emphasis of the riff is on this step. An empty mask gives 0 and changes nothing:
    // a style without a riff lives on the same guides.
    let riff_accent = riff.accent(song, step_in_measure);
    let riff_bias = if riff.is_active() {
        riff::RiffCell::gate_bias(riff_accent)
    } else {
        0.0
    };
    // Phrase grid of the theme: eighth notes, onset mask, cells. Neutral
    // path (`Melody::DENSE`) does not include it - engine tests see
    // the same tight line at every step. The mask does not apply to
    // accompaniment: it has its own groove.
    let phrased = use_motif && harmony.melody.phrased;
    // Echo instead of unison: echoing carrier enters 1–2 eighth notes later
    // leader and sings the same cells with a shifted grid. This is an imitation
    // and not a choir of soloists - and the theme does not fall silent on the steps of the leader.
    let echo = phrased && !theme_lead && !cadence;
    let echo_shift = if echo { 1 + (slot_idx % 2) } else { 0 };
    // The phrase slot is also needed for the neutral path: it reads all 16
    // steps of the push-pull theme, even when the onset mask is turned off.
    let slot = (phrase_slot(song, step_in_measure, steps_per_measure) + echo_shift) % 16;
    // The mask is accents, not a wall: the theme sounds on off eighth notes
    // less often, but does not disappear. Hard mute gave 3 notes per measure and theme
    // drowned in the mix (measuring: lofi 39/12s, rock 125/12s instead of ~57/160).
    // The sixteenth notes between the onsets are ghosts of the phrase, even quieter.
    let mut phrase_density = 1.0f32;
    if phrased && !cadence {
        let onsets = phrase_onsets(harmony.melody.onsets, song);
        if onsets & (1u16 << slot) == 0 {
            phrase_density *= MASK_OFF_DENSITY;
        }
        if !step_in_measure.is_multiple_of(2) {
            phrase_density *= MASK_SUBDIV_DENSITY;
        }
    }
    // General plan (general phrase): all theme carriers use one gate roll -
    // the sparse ones play a subset of the dense ones (the same steps, not their own).
    // Each layer goes into its own paint (voice, octave, velocity),
    // but strikes together: heterophony instead of a choir of soloists.
    // Drums, bass and accompaniment use their own: each work
    // your pattern, otherwise the groove will stick together in unison.
    let groove_key = if use_motif { 10_077 } else { slot_idx };
    // Breathing of a phrase: the carrier of the theme sometimes plays less often and more quietly, but not
    // falls silent. The last quarter of the phrase is a short pickup: phrase
    // answers rather than breaks off. The leader and the lone supporter are not breathing.
    let breath = guide.follow_motif
        && !theme_lead
        && guide.phrase_rest > 0.0
        && fixed_gate(song.seed_hash, slot_idx, song.phrase_index, 0x2E57) < guide.phrase_rest;
    let breath_turn = breath && phrase_turn_zone(steps_per_measure, song, step_in_measure);

    // Groove pole: the beat is registered by choosing a pole (seed, plan; when
    // section_poles - also a section: the drop has its own beat) and holds.
    // It is only enlivened by phrase gates and seed swing. The same seed -
    // same beat; another seed is different.
    let weight = pole_step_weight(guide, song, groove_key, step_in_measure, steps_per_measure);
    // The cadence also sounds where the weight of the plan is zero: the end of the section -
    // a written dot, not a hole in the drawing.
    if weight <= 0.0 && !cadence {
        return Vec::new();
    }
    // Spot accents - arrangement, not flashing: present
    // whole sections (gate from the visit), and not phrases.
    // End-to-end stage without `% 64`: short period gave verbatim
    // repeat effects every 64 visits.
    let prior = accent_prior(instrument_id);
    if !cadence
        && prior < 1.0
        && fixed_gate(song.seed_hash, groove_key, song.stage, 0xA66E47) >= prior
    {
        return Vec::new();
    }

    // Grid path: probabilities + seed gate.
    // Kick anchors are absolute: the foundation always hits, without breathing.
    // The remaining anchors breathe with a priority of 0.95.
    let kick_anchor = weight >= 1.0 && matches!(role, Some(InstrumentRole::Kick));
    let probability = if cadence {
        1.0
    } else if weight >= 1.0 {
        // Anchor is a strong priority, not a guarantee: the downbeat breathes.
        // Except for the kick: there’s a guarantee.
        ANCHOR_PRIOR
    } else {
        // Seed density scales all probabilities: sparse
        // and thick seeds of the same style. Role share: several carriers -
        // each less often (sharing space), one - in full force.
        // Anchors go with their own priority and do not scale.
        // The riff and breath of a phrase shifts probability, but does not create
        // guarantees: anchors remain anchors.
        let mut p = weight
            * energy
            * energy
            * guide.energy_response
            * song.density
            * harmony.density
            * role_share
            * phrase_density;
        if riff.is_active() {
            p *= riff::RiffCell::probability(riff_accent);
        }
        if breath {
            if breath_turn {
                // Pickup phrases: the last quarter does not drown in rarefaction.
                p = p.max(0.75);
            } else {
                p *= 1.0 - 0.55 * guide.phrase_rest;
            }
        }
        p.clamp(0.0, 1.0)
    };
    // Gate seed: the decision depends on (seed, plan, measure, section visit).
    //
    // Previously, salt used the phrase: the drum pattern was re-dealt every two
    // beats, and at the junction of the phrase the beats appeared and disappeared - at
    // hearing is a “jump” (flow measurement: lo-fi-kick has 831 versus 32
    // inside a phrase). Now the salt holds the VISIT section: inside the section
    // there is only one groove, it changes at the interface - like a live band.
    // Anchors (weight >= 1.0) hold the general skeleton: their salt does not include
    // beat, and the breath of the downbeat (5% of skips) should not “blink” on
    // every phrase.
    // Theme carriers share the roll: the sparse enters into the steps of the dense.
    // Anchors use a separate roll (their own salt) - skipping the anchor
    // does not entail skipping other steps and vice versa.
    // The kick-anchor gate does not work at all.
    let bar_in_phrase = song.measure_index.saturating_sub(1) % 2;
    let salt = if weight >= 1.0 {
        // Anchors (downbeat) breathe according to the phrase: 5% of passes - live pulse.
        ANCHOR_SALT ^ (song.phrase_index as u64)
    } else {
        // Other notes (syncopation, strokes) - their own salt to the phrase: drawing
        // develops rather than repeats the previous phrase verbatim.
        (song.phrase_index as u64)
            .wrapping_mul(2)
            .wrapping_add(bar_in_phrase as u64)
    };
    if !kick_anchor
        && probability < 1.0
        && fixed_gate(song.seed_hash, groove_key, step_in_measure, salt) >= probability
    {
        return Vec::new();
    }
    // Theme hierarchy: one leads (the section leader), the rest echo.
    // A general cast on all echoers: the echo sounds together, in the same steps.
    // Salt - section visit, in step with leadership: the composition of the echo stands,
    // while the leader stands.
    // The nesting of the base gates is intact: sparse and so a subset.
    if use_motif
        && !theme_lead
        && !cadence
        && fixed_gate(song.seed_hash, 7777, step_in_measure, song.stage as u64) >= LEAD_ECHO
    {
        return Vec::new();
    }

    let mut velocity = (guide.vel_base
        + energy * guide.vel_energy
        + rng.gen_range(-guide.vel_jitter..=guide.vel_jitter))
    .clamp(0.1, 1.0);
    // The breath of the phrase and the emphasis of the riff are the dynamics of the performance, not the layer:
    // the sparse phrase is quieter, the riff beat is a little louder.
    if breath && !breath_turn {
        velocity *= 1.0 - 0.30 * guide.phrase_rest;
    }
    if riff.is_active() {
        velocity *= riff::RiffCell::velocity(riff_accent);
    }

    // Articulation of the seed for soft voices: the same note in the same voice -
    // with different softness for different seeds. The drums go as is.
    let (attack_mul, release_mul) = envelope_shape(song.seed_hash, slot_idx);

    // Length - generation solution: guide dictionary + meter + phrase + seed.
    // Previously there was a timbre constant (duration_mult) with multipliers: all
    // the notes of the instrument were of the same length, the style and the seed changed only its center.
    let gate_steps = choose_gate(
        guide,
        song,
        slot_idx,
        step_in_measure,
        steps_per_measure,
        use_motif,
        riff_bias,
    );
    // Theme duration: style decides whether a note extends across a rest
    // masks. Not shorter than half a step: a click instead of a note is not musical.
    let gate_steps = if phrased {
        (gate_steps * harmony.melody.sustain).max(0.5)
    } else {
        gate_steps
    };
    let duration = gate_steps
        * base_step_secs
        // Human micro-variation ±4%: notes even to the millisecond
        // sound mechanical, but the length class (16th, 8th, quarter) is intact.
        * (1.0
            + 0.04
                * (fixed_gate(song.seed_hash, slot_idx, step_in_measure, 0xD07A) * 2.0 - 1.0));

    // Drums: single note fixed pitch, shortcut.
    if let Voicing::Kit(pitch) = guide.voice {
        if guide.vel_high_cut < 1.0 {
            let octaves_above = ((pitch as f32 - 40.0) / 12.0).max(0.0);
            velocity = (velocity * guide.vel_high_cut.powf(octaves_above)).clamp(0.05, 1.0);
        }
        if let Some(r) = role {
            velocity *= song.stage_gain(r);
        }
        // Hierarchy of beats in drums: downbeat - full voice (×1.25),
        // the third - a little quieter, weak - as written. The beat is rocking
        // dynamics, and not a flat wall of identical hits.
        velocity *= beat_accent(step_in_measure, steps_per_measure);
        if velocity < 0.05 {
            return Vec::new();
        }
        return vec![NoteEvent::new(instrument_id, pitch, velocity, duration)];
    }

    // `line_fold` - how many octaves the entire theme line goes down,
    // so that the register ceiling does not cut individual notes (otherwise the line
    // jumps an octave in the middle of a phrase).
    let (tone_base, degree, octaves, line_fold) = match guide.voice {
        Voicing::Kit(_) => unreachable!("kit handled above"),
        Voicing::Scale {
            degree: voice_degree,
            octave,
            jitter,
        } => {
            if scale.is_empty() {
                return Vec::new();
            }
            // Rotation of scale from seed: same set of sounds, different tone
            // center - the seeds sound in different “keys”, the range is intact.
            let rotation = (song.seed_hash % scale.len() as u64) as usize;
            // Seed motif: a recognizable theme instead of wandering around the scale.
            // Rotation is applied to it too - the theme develops with harmony.
            // The accompaniment (not the motif) moves according to the lead progression:
            // the chords change every 2 measures, remaining in scale.
            // The forced carrier follows the same theme path as the native one.
            let use_motif = guide.follow_motif || force_motif;
            // The chord of the current measure is common to the theme and accompaniment.
            // The strong beats of the theme are pulled to its tones, the bass walks by roots.
            // On the last measure of the section, the accompaniment is on the tonic:
            // cadence is a written point, not a random chord progression.
            let chord_root = harmony.prog
                [(song.measure_index.saturating_sub(1) / harmony.bar_len) % harmony.prog.len()]
                as usize;
            let prog_shift = if use_motif || song.is_last_measure {
                0
            } else {
                // The accompaniment follows the progression of the style: bass - roots
                // along with everyone (there is no more drone on one note),
                // the rest - in full. Harmonic rhythm is also a style:
                // the club changes the chord every 2 bars, the phonk holds 4,
                // lofi breathes at 8. seed chooses an option within the style.
                // The bass register keeps the fold, the wander keeps the guide jitter.
                // The progression follows the session-wide measure: a reset at
                // at each junction of sections turned the movement into trampling
                // on the first chord in any short section.
                chord_root
            };
            let (degree, jitter, contour_max) = if use_motif {
                // Live scale: section theme (A everywhere, B on Drop/PostChorus)
                // plus a phrase grid (mask, cells, cadence). seed's DNA
                // preserved, but there are no verbatim repetitions.
                // No response transposition: third up on odd phrases
                // desynchronized layers (halftone clashes up to 37% of steps).
                // Cadance: The last step of the section is the tonic (step 0).
                // The section closes (development is heard: the phrase somewhere
                // came), the next one opens with its theme.
                let theme = if song.use_alt_motif {
                    &song.motif_b
                } else {
                    &song.motif
                };
                // Motif - 16 steps per two beats: four cells
                // four steps. The order of the cells determines the style and
                // section: cell repeat - hook, rearrangement - development.
                // The neutral melody keeps the order 0,1,2,3 and sounds
                // like the previous long theme.
                let cells = phrase_cells(harmony.melody.cells, song);
                // Sequence: visiting a section shifts the response by 0..15 steps
                // themes. The first two visits keep the written outline
                // (line test and hearing), then the theme develops.
                // The hook and cadence do not move - the form is intact.
                let sequence = if song.stage <= 1 {
                    0
                } else {
                    song.stage % 16
                };
                let mut motif_step = if cadence {
                    0
                } else {
                    theme[phrase_theme_index(cells, slot, theme.len(), sequence)] as usize
                };
                // Top of the outline: the entire theme (all cells), not the window.
                // The line should fit into the scale without an octave break.
                let theme_max = theme.iter().copied().max().unwrap_or(0) as usize;
                let contour_max = theme_max + voice_degree + 1;
                // Attraction of downbeats to a chord: style decides
                // how often and how far. We move no more
                // than a step - the passing notes and the arch are intact.
                let strong = step_in_measure.is_multiple_of((steps_per_measure / 4).max(1));
                if harmony.melody.phrased
                    && strong
                    && harmony.melody.chord_snap > 0.0
                    && !cadence
                    && fixed_gate(song.seed_hash, 0x5A17, slot, song.stage as u64)
                        < harmony.melody.chord_snap
                {
                    motif_step =
                        snap_to_chord(motif_step, chord_root, scale.len(), theme_max);
                }
                // Voice shift - counting line: guide sets your step
                // (lead guitar - a third above the riff), the theme sets
                // circuit. Previously, the guide shift was lost and all media
                // they sang in unison: instead of two voices, there was one.
                let contour = motif_step + voice_degree;
                (contour, 0, contour_max)
            } else {
                let mut d = (voice_degree + prog_shift) % scale.len().max(1);
                // Common foundation of the seed: the accompaniment goes not only
                // by progression, but also by the harmonic intervals of the riff.
                // These are the same notes, but their center moves with the bass.
                // On the last measure of the section, the riff does not move the root:
                // The cadence closes with the tonic.
                if riff.is_active() && !song.is_last_measure {
                    d = (d + riff.root(song, step_in_measure) as usize) % scale.len().max(1);
                }
                // Fifth bass: off-beat eighths go to fifths -
                // The bassline moves rather than tramples on the tonic.
                if guide.bass_fifth
                    && !song.is_last_measure
                    && matches!(role, Some(InstrumentRole::Bass))
                {
                    let eighth = steps_per_measure / 8;
                    if eighth > 0 && step_in_measure % (eighth * 2) == eighth {
                        let fifth = if scale.len() == 7 { 4 } else { 3 };
                        d = (d + fifth) % scale.len().max(1);
                    }
                }
                (d, jitter, 0)
            };
            let drift = if jitter > 0 {
                // Downbeats hold harmony: jitter collapses -
                // the written step sounds (skeleton chord tone).
                // Weak lobes are passing (wandering as before).
                // Same idea as even/odd in Markov sampling v1:
                // meter skeleton, ornament - flesh. Deterministic.
                let q = (steps_per_measure / 4).max(1);
                if step_in_measure.is_multiple_of(q) {
                    0
                } else {
                    // The wandering is also fixed by the seed and the section visit:
                    // inside the section the same note in the same place, then -
                    // retake at the border (and not in the middle of the song).
                    (crate::arranger::splitmix64(
                        song.seed_hash
                            ^ (slot_idx as u64).wrapping_mul(0xC0FFEE)
                            ^ (step_in_measure as u64).wrapping_mul(0x9E3779B9)
                            ^ (song.stage as u64).wrapping_mul(0x85EBCA6B)
                            ^ 0xD1F7,
                    ) % (jitter as u64 + 1)) as usize
                }
            } else {
                0
            };
            // The tonic is ready in the context (arranger's incremental walk):
            // We take the Lada module here. A visit, not a phrase: the center is not
            // interrupts the theme within the form section.
            let tonic = song.tonic_walk.rem_euclid(scale.len().max(1) as i64) as usize;
            let len = scale.len();
            // The center of harmony is the tonic plus the rotation of the seed. This is CLASS
            // sound, not height: the outline of the theme lies on top and does not
            // is wrapped according to the modulus of the scale. Previously, the amount was taken according to
            // the entire module, and as soon as it passed the top of the scale,
            // the contour turned back: the line broke abruptly
            // seventh and back in the middle of the phrase (there was no arch - “hump”).
            let base_class = (tonic + rotation + drift) % len;
            // Line base - without contour: contour (theme level or
            // chord) is added when selecting a step, otherwise the line
            // loses the theme.
            let tone_base: i32 = if use_motif {
                // The outline must fit entirely into harmony: if it doesn’t fit,
                // the whole line goes down one step (the register is even,
                // the form of the theme is intact).
                let overflow = base_class + contour_max + 1;
                let shift_down = overflow.saturating_sub(len);
                // The line does not go below the zero degree of the scale: otherwise
                // rem_euclid would return the top of the scale and the line would jump
                // up an octave in the middle of a phrase.
                base_class.saturating_sub(shift_down) as i32
            } else {
                (tonic + drift + rotation) as i32
            };
            // The octave is common to the chord tones; We check the rise by the root.
            let root_idx = (tone_base + degree as i32).rem_euclid(len as i32) as usize;
            // Absolute height - from seed (family tone + tonic shift)
            // and style (its tonality over the seed shift).
            let shift = tonic_shift_for(song.seed_hash) as i32 + harmony.key_shift as i32;
            let root = modal_tone(scale, root_idx, song.seed_hash) as i32 + shift;
            let bump = guide.octave_up > 0.0
                // Lead register ceiling: above D6 (86) - icy whistle,
                // not music. We take the minimum from the guide and the cap.
                // The roll is shared across the plan: lifts happen in unison, not at random.
                // A visit to the section is in step with the tonic: the register costs the entire section.
                && root.saturating_add(12) <= guide.top_pitch.min(86) as i32
                && fixed_gate(
                    song.seed_hash,
                    groove_key,
                    step_in_measure,
                    0x0C74 ^ (song.stage as u64),
                ) < guide.octave_up;
            // Octave of the theme from the seed: motif and pickup go together.
            // Contrast in pairs of sections: even pairs - low, odd -
            // an octave higher. Verse/chorus in registers, not just
            // volume. The boundary is the boundary of the section, not the middle of the song.
            // The top is secured by caps below.
            let seed_octave = if use_motif {
                motif_octave(song.seed_hash)
            } else {
                0
            };
            let window_octave = if use_motif {
                let w = ((song.stage / 2) % 2) as i8;
                // Direction - from voice register: fold 86 eats shift
                // up for high voices entirely (74+12→86→74: sections
                // sound the same), so we move the high ones down.
                // The rule is static (guide + seed): the unison does not stratify.
                if octave + seed_octave >= 2 {
                    -w
                } else {
                    w
                }
            } else {
                0
            };
            // Register ceiling: the theme line goes down WHOLELY, and not
            // separate notes. Otherwise, the top of the note is cut off by an octave.
            // down, but the lower ones remain - the line breaks in the middle of the phrase.
            let octaves_line = octave + seed_octave + window_octave;
            let line_fold = if use_motif {
                // The top of the line is by enumerating steps, not by the maximum
                // steps: modal version (Phrygian second) lowers
                // degree, and “top degree” does not mean “top note.”
                // If we counted by degrees, the top note would remain higher
                // burl and cut one at a time: the line was torn into an octave.
                let mut top = 0i32;
                for k in 0..=contour_max {
                    let idx = (tone_base + k as i32).rem_euclid(len as i32) as usize;
                    let p = modal_tone(scale, idx, song.seed_hash) as i32;
                    if p > top {
                        top = p;
                    }
                }
                let top = top + shift + 12 * octaves_line as i32;
                let cap = guide.top_pitch.min(86) as i32;
                if top > cap {
                    (top - cap) / 12 + 1
                } else {
                    0
                }
            } else {
                0
            };
            (
                tone_base,
                degree,
                octaves_line + if bump { 1 } else { 0 },
                line_fold,
            )
        }
    };

    // Chord scoring: pads and keys play triads (or intervals)
    // from the guide), melody - single notes. Condensation (empty chords) -
    // dyad root + third for leads: harmony without turbidity. All tones are in harmony.
    // Arpeggio: one interval per hit, in the round (fingerpicking).
    let intervals: &[i8] = match guide.chord {
        Some(iv) => iv,
        None if matches!(role, Some(InstrumentRole::Chords)) && !guide.follow_motif => &[0, 2, 4],
        None if force_chords => &[0, 2],
        _ => &[0],
    };
    let arp_one;
    let intervals: &[i8] = if guide.arp && intervals.len() > 1 {
        arp_one = [intervals[(step_in_measure / 2) % intervals.len()]];
        &arp_one
    } else {
        intervals
    };
    let mut out = Vec::with_capacity(intervals.len());
    for (ti, iv) in intervals.iter().enumerate() {
        let idx = (tone_base + degree as i32 + *iv as i32).rem_euclid(scale.len() as i32) as usize;
        // Line shift (line_fold) - general for the phrase: register ceiling
        // does not cut individual notes, otherwise the line breaks in octave jumps.
        let mut tone = (modal_tone(scale, idx, song.seed_hash) as i32
            + tonic_shift_for(song.seed_hash) as i32
            + harmony.key_shift as i32
            + 12 * octaves as i32
            - 12 * line_fold)
            .clamp(0, 255) as u8;
        // The ceiling of the theme: the seed-octave could stand above the guide -
        // fold it octave down (same class, no whistle).
        if guide.follow_motif || force_motif {
            while tone > guide.top_pitch.min(86) {
                tone = tone.saturating_sub(12);
            }
        }
        // We keep the bottom down: fold the burl that has crawled above, octave down.
        // The harmony is the same (same pitch class). Cap from the guide: sub 45,
        // rock riffs higher (52) following the guitar. Pitch kick (808)
        // sits in the same sub-register as the bass - common register
        // law of the bottom; drum kits (fixed pitch) are not affected by this.
        let low_fold = matches!(role, Some(InstrumentRole::Bass))
            || (matches!(role, Some(InstrumentRole::Kick))
                && matches!(guide.voice, Voicing::Scale { .. }));
        if low_fold {
            while tone > guide.bass_top {
                tone -= 12;
            }
        }
        let atten = if ti < CHORD_ATTEN.len() {
            CHORD_ATTEN[ti]
        } else {
            0.5
        };
        let mut v = velocity * atten;
        // Muting high notes - for each tone separately.
        if guide.vel_high_cut < 1.0 {
            let octaves_above = ((tone as f32 - 40.0) / 12.0).max(0.0);
            v = (v * guide.vel_high_cut.powf(octaves_above)).clamp(0.05, 1.0);
        }
        // Beat accents: the downbeat speaks loudest, then in descending order.
        // Without a hierarchy of volume, the beat drowns in a flat wall (everyone knocks
        // equal) - the meter is audible even when the pattern is syncopated.
        v *= beat_accent(step_in_measure, steps_per_measure);
        // Breathing of the measure: theme carriers swell slightly towards the middle (±10%).
        // Arc v1 within the phrase: peak of development - not at the border, phrase
        // breathes, and does not knock with even hits. Theme only (drums)
        // and the bass stays on the meter without changing the volume).
        if guide.follow_motif || force_motif {
            let pos = step_in_measure as f32 / steps_per_measure.max(1) as f32;
            v *= 1.0 + 0.10 * (std::f32::consts::PI * pos).sin();
        }
        // Phrase volume: balance of parts according to the circle step.
        // Table zero = silence (there is no snare in the intro - and there shouldn’t be).
        if let Some(r) = role {
            v *= song.stage_gain(r);
        }
        // Bending the bass under the foundation - only with a slider
        // (smooth curve from the kick). The second pit generator does not:
        // this gave a double dip on every syncopated kick.
        if v >= 0.05 {
            let mut note =
                NoteEvent::new(instrument_id, tone, v, duration).shaped(attack_mul, release_mul);
            note.sub_bias = guide.sub_bias.clamp(0.0, 1.0);
            out.push(note);
        }
    }
    out
}

/// Harmonic rhythm of the lead (measures per chord): 2 - movement,
/// 4 - middle, 8 - foundation. The choice is determined by the seed:
/// Some seeds are harmoniously alive, others are static. Pure function.
pub(crate) fn prog_bar_len(seed_hash: u64) -> usize {
    const LENS: [usize; 3] = [2, 4, 8];
    LENS[(crate::arranger::splitmix64(seed_hash ^ 0x6A12) % 3) as usize]
}

/// Eight phrase arc (16 bars): inhale towards the middle and exhale towards the end.
/// The multiplier is limited to 0.94..1.0 - the form breathes, but does not wobble
/// the volume varies from section to section and does not interfere with the clip.
fn phrase_arc(phrase_index: usize) -> f32 {
    let phase = (phrase_index % 8) as f32 / 7.0;
    let lift = 1.0 - (2.0 * phase - 1.0).abs();
    0.94 + 0.06 * lift
}

/// Beat accent 1.0..1.25: downbeat - full voice, third beat -
/// a little quieter, weak - as written. Pure step function.
pub(crate) fn beat_accent(step_in_measure: usize, steps_per_measure: usize) -> f32 {
    let q = (steps_per_measure / 4).max(1);
    match (step_in_measure / q) % 4 {
        0 => 1.25,
        2 => 1.12,
        _ => 1.0,
    }
}

/// Style factory by name (case and spaces are ignored).
/// `None` for unknown names - the caller warns the user
/// instead of quietly turning on the wrong style.
pub fn make_style(name: &str) -> Option<Box<dyn Style>> {
    use club::ClubStyle;
    use lofi::LofiStyle;
    match name.trim().to_lowercase().as_str() {
        "club" => Some(Box::new(ClubStyle::new())),
        "lofi" => Some(Box::new(LofiStyle::new())),
        _ => None,
    }
}

/// All available styles: names for CLI and Flutter.
pub fn style_names() -> &'static [&'static str] {
    &["club", "lofi"]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arranger::SongSection;

    /// Full style palette: performance tests check the guidelines
    /// on all colors, and not on a specific seed cast.
    fn palette_active(style: &dyn Style) -> Vec<usize> {
        style.ensemble().iter().map(|s| s.id()).collect()
    }

    fn test_song(progress: f32) -> SongContext {
        SongContext {
            section: SongSection::MainMotif,
            measure_index: 1,
            phrase_index: 0,
            energy: 1.0,
            progress,
            is_last_measure: false,
            section_measure_index: 1,
            previous_section: SongSection::MainMotif,
            stage: 1,
            cycle: 0,
            stage_gains: [1.0; 8],
            layers: crate::piece::layers_for(SongSection::MainMotif),
            seed_hash: 42,
            density: 1.0,
            motif: [0, 2, 4, 2, 0, 3, 1, 4, 4, 3, 1, 4, 2, 0, 1, 3],
            motif_b: [1, 4, 0, 3, 2, 4, 2, 0, 4, 0, 1, 3, 3, 2, 0, 4],
            use_alt_motif: false,
            prog: [0, 0, 0, 0],
            tonic_walk: 0,
        }
    }

    #[test]
    fn energy_shapes_are_sane() {
        // The curve of each style: monotonous, within reasonable limits,
        // drop (1.0) is not quieter than intro (0.0).
        for name in style_names() {
            let style = make_style(name).expect("known style");
            let lo = style.shape_energy(0.0);
            let hi = style.shape_energy(1.0);
            let mid = style.shape_energy(0.5);
            assert!(lo.is_finite() && hi.is_finite(), "{name}: non-finite");
            assert!(
                (0.0..=1.2).contains(&lo) && (0.0..=1.2).contains(&hi),
                "{name}: out of range ({lo}, {hi})"
            );
            assert!(hi >= lo, "{name}: inverted curve");
            assert!(
                mid >= lo && mid <= hi,
                "{name}: non-monotonic ({lo}, {mid}, {hi})"
            );
        }
    }

    #[test]
    fn guide_data_matches_grid() {
        // The weights must match the style grid,
        // otherwise the steps are silently extinguished (.get → None → 0.0).
        // Whitelist - only from known slots.
        for name in style_names() {
            let style = make_style(name).expect("known style");
            let spm = style.steps_per_measure();
            for slot in style.ensemble() {
                assert!(Slot::ALL.contains(slot), "{name}: unknown slot in ensemble");
            }
            for (i, guide) in style.slot_guides().iter().enumerate() {
                assert_eq!(guide.weights.len(), spm, "{name} slot {i}: weights len");
                if let Some(poles) = guide.poles {
                    assert!(!poles.is_empty(), "{name} slot {i}: empty poles");
                    for (j, pole) in poles.iter().enumerate() {
                        assert_eq!(pole.len(), spm, "{name} slot {i} pole {j}: len");
                    }
                }
            }
        }
    }

    #[test]
    fn every_style_has_its_own_form_and_arrangement() {
        // The style should differ in structure, and not just in timbres:
        // its own form (section lengths) and its own arrangement (who plays in the sections).
        // Otherwise, all styles are one skeleton with different instruments.
        let mut forms: Vec<Vec<String>> = Vec::new();
        let mut arrangements: Vec<Vec<u8>> = Vec::new();
        for name in style_names() {
            let style = make_style(name).expect("known style");
            let sig: Vec<String> = style
                .arrangement()
                .cycle
                .iter()
                .map(|(s, bars)| format!("{s:?}{bars}"))
                .collect();
            assert!(!forms.contains(&sig), "{name}: form matches another style");
            forms.push(sig);
            let layers = style.arrangement_layers();
            assert_eq!(layers.len(), 4, "{name}: arrangement does not have four sections");
            arrangements.push(layers.to_vec());
        }
        for i in 0..arrangements.len() {
            for j in (i + 1)..arrangements.len() {
                assert_ne!(
                    arrangements[i], arrangements[j],
                    "styles {i} and {j} play the same"
                );
            }
        }
    }

    #[test]
    fn seed_picks_a_cast_from_the_palette() {
        // Cast is a subset of the palette, but the foundation, signatures and slots
        // transitions remain. Different seeds give different compositions.
        use crate::instruments::Slot;
        use std::collections::HashSet;
        for name in style_names() {
            let style = make_style(name).expect("known style");
            let palette = style.ensemble();
            let mut casts = HashSet::new();
            for seed in 0..24u64 {
                let cast = style.cast(seed);
                for slot in &cast {
                    assert!(
                        palette.contains(slot),
                        "{name}/{seed}: {slot:?} outside the palette"
                    );
                }
                for slot in [Slot::Bass, Slot::Kick, Slot::Snare] {
                    assert!(cast.contains(&slot), "{name}/{seed}: {slot:?} lost");
                }
                for slot in style.signature_slots() {
                    assert!(
                        cast.contains(slot),
                        "{name}/{seed}: signature {slot:?} lost"
                    );
                }
                if let Some(spec) = style.transition_gesture() {
                    for slot in spec.fill_slots.iter().chain(spec.tail_slots.iter()) {
                        assert!(
                            cast.contains(slot),
                            "{name}/{seed}: transition slot {slot:?} lost"
                        );
                    }
                }
                casts.insert(cast);
            }
            assert!(
                casts.len() >= 2,
                "{name}: cast is the same for all seeds ({casts:?})"
            );
        }
    }

    #[test]
    fn seed_picks_a_form_not_just_a_motif() {
        // The form is also a decision of the seed: the style has several written
        // options, otherwise the pieces would differ only in theme.
        use std::collections::HashSet;
        for name in style_names() {
            let style = make_style(name).expect("known style");
            let shapes: HashSet<Vec<(SongSection, usize)>> = (0..24u64)
                .map(|seed| style.arrangement_for_seed(seed).cycle.to_vec())
                .collect();
            assert!(
                shapes.len() >= 2,
                "{name}: one form for all seeds ({shapes:?})"
            );
        }
    }

    #[test]
    fn every_seed_gets_the_full_arc_in_the_window() {
        // Pieces of any style and seed reach the hit and return to
        // theme, within 30–60 s; no section is cut off.
        use crate::arranger::SongSection;
        for name in style_names() {
            let style = make_style(name).expect("known style");
            let bpm = style.bpm();
            for seed in [1u64, 2, 3, 42, 777] {
                let score = style.arrangement_for_seed(seed);
                for index in 0..3u64 {
                    let plan = crate::piece::plan_piece(
                        seed,
                        bpm,
                        score.cycle,
                        index,
                        0,
                        style.transition(),
                        style.arrangement_layers(),
                    );
                    let secs = plan.bars() as f32 * 4.0 * 60.0 / bpm;
                    assert!(
                        (30.0..=60.0).contains(&secs),
                        "{name}/{seed} piece {index}: {secs:.1} s ({} bars)",
                        plan.bars()
                    );
                    let at = |want| plan.body.iter().position(|s| s.section == want);
                    let main = at(SongSection::MainMotif)
                        .unwrap_or_else(|| panic!("{name}/{seed}: no theme"));
                    let build = at(SongSection::BuildUp)
                        .unwrap_or_else(|| panic!("{name}/{seed}: no build-up"));
                    let drop =
                        at(SongSection::Drop).unwrap_or_else(|| panic!("{name}/{seed}: no drop"));
                    let post = at(SongSection::PostChorus)
                        .unwrap_or_else(|| panic!("{name}/{seed}: no post-chorus"));
                    assert!(
                        main < build && build < drop && drop < post,
                        "{name}/{seed}: arc broken: {:?}",
                        plan.body
                    );
                    for section in &plan.body {
                        assert!(
                            section.bars >= 2 && section.bars % 2 == 0,
                            "{name}/{seed}: section is not a whole phrase: {section:?}"
                        );
                    }
                    assert_eq!(
                        plan.joint.section,
                        SongSection::PostChorus,
                        "{name}/{seed}: the seam must be the finale"
                    );
                }
            }
        }
    }

    #[test]
    fn style_transitions_sound_at_the_seam() {
        // The style transition is not a flag in the guide, but a gesture on the seam:
        // the last measure of the build-up receives the transition notes. Everyone has
        // style has its own dictionary and its own slot, but no one should remain silent.
        use crate::instruments::Slot;
        for name in style_names() {
            let style = make_style(name).expect("known style");
            let spec = style
                .transition_gesture()
                .unwrap_or_else(|| panic!("{name}: no transition"));
            let spm = style.steps_per_measure();
            let active = palette_active(&*style);
            let mut rng = rand::SeedableRng::seed_from_u64(424242);
            let mut arranger = crate::arranger::Arranger::new(424242);
            let mut song = arranger.advance_measure();
            let mut found = false;
            for _ in 0..160 {
                if song.section == SongSection::BuildUp && song.is_last_measure {
                    found = true;
                    break;
                }
                song = arranger.advance_measure();
            }
            assert!(found, "{name}: the piece has no last build-up measure");
            let mut heard = 0usize;
            for step in 0..spm {
                let mut sctx = StepContext {
                    step,
                    step_in_measure: step,
                    base_step_secs: style.base_step_secs(),
                    active_tracks: &active,
                    song: &song,
                    rng: &mut rng,
                    intensity: 1.0,
                };
                let notes = render_style(&*style, &mut sctx);
                if spec.fill_slots.iter().any(|slot| {
                    notes
                        .iter()
                        .any(|n| Slot::from_id(n.instrument_id) == Some(*slot))
                }) {
                    heard += 1;
                }
            }
            assert!(heard > 0, "{name}: the seam gesture is inaudible");
        }
    }

    #[test]
    fn arrangement_layers_mute_the_rest() {
        // Arrange in layers: the plan is written by who is playing. The silent group is silent
        // for real (not “quieter”), otherwise the points on the wheel and the sound again
        // will disperse.
        use crate::instruments::{InstrumentRole, Slot};
        for name in style_names() {
            let style = make_style(name).expect("known style");
            let ensemble = style.ensemble();
            let active: Vec<usize> = Slot::ALL
                .iter()
                .filter(|s| ensemble.contains(s))
                .map(|s| s.id())
                .collect();
            let mut rng = rand::SeedableRng::seed_from_u64(424242);
            let mut arranger = crate::arranger::Arranger::new(424242);
            let mut song = arranger.advance_measure();
            // Only groove: no theme, no pump.
            song.layers = crate::piece::L_GROOVE;
            let mut heard_lead = 0;
            let mut heard_groove = 0;
            for step in 0..64 {
                let spm = style.steps_per_measure();
                let sim = step % spm;
                if sim == 0 && step > 0 {
                    song = arranger.advance_measure();
                    song.layers = crate::piece::L_GROOVE;
                }
                let mut sctx = StepContext {
                    step,
                    step_in_measure: sim,
                    base_step_secs: style.base_step_secs(),
                    active_tracks: &active,
                    song: &song,
                    rng: &mut rng,
                    intensity: 1.0,
                };
                for note in render_style(&*style, &mut sctx) {
                    let role = Slot::from_id(note.instrument_id).map(|s| s.role());
                    match role {
                        Some(InstrumentRole::Lead | InstrumentRole::Chords) => heard_lead += 1,
                        Some(
                            InstrumentRole::Kick | InstrumentRole::Snare | InstrumentRole::Hat,
                        ) => heard_groove += 1,
                        _ => {}
                    }
                }
            }
            assert_eq!(heard_lead, 0, "{name}: upper layers play under the groove mask");
            assert!(heard_groove > 0, "{name}: the groove is silent under its own mask");
        }
    }

    fn motif_pitches(motif: [u8; 16]) -> Vec<u8> {
        // Skeleton height test via anchor-absolute kick-role (id 2):
        // there are no gates - the frame has pure height without breathing gaps.
        let guide = RoleGuide {
            weights: &[1.0; 16],
            follow_motif: true,
            ..RoleGuide::pitched(&[1.0; 16], 0, 2, 0, &[1.0], 0.8)
        };
        let scale = [48, 50, 52, 53, 55, 57, 59];
        let mut song = test_song(0.5);
        song.motif = motif;
        song.stage_gains = [1.0; 8];
        let mut rng = rand::SeedableRng::seed_from_u64(3);
        (0..8)
            .filter_map(|step| {
                play_guide(
                    &guide,
                    &scale,
                    &song,
                    1.0,
                    step * 2,
                    16,
                    0.1,
                    2,
                    5,
                    1.0,
                    false,
                    false,
                    true,
                    &mut rng,
                )
                .into_iter()
                .next()
                .map(|n| n.midi_pitch)
            })
            .collect()
    }

    #[test]
    fn alt_motif_switches_theme() {
        // Same lead, different A/B themes - different melodies.
        // use_alt_motif toggles the array, everything else is the same.
        let a = motif_pitches([0, 2, 4, 2, 0, 3, 1, 4, 4, 3, 1, 4, 2, 0, 1, 3]);
        let mut song_b = test_song(0.5);
        song_b.motif = [0, 2, 4, 2, 0, 3, 1, 4, 4, 3, 1, 4, 2, 0, 1, 3];
        song_b.motif_b = [4, 4, 0, 0, 2, 2, 4, 4, 1, 3, 3, 0, 0, 4, 2, 1];
        song_b.use_alt_motif = true;
        let guide = RoleGuide {
            weights: &[1.0; 16],
            follow_motif: true,
            ..RoleGuide::pitched(&[1.0; 16], 0, 2, 0, &[1.0], 0.8)
        };
        let scale = [48, 50, 52, 53, 55, 57, 59];
        let mut rng = rand::SeedableRng::seed_from_u64(3);
        // Test through anchor-absolute (id 2): height without gaps.
        let b: Vec<u8> = (0..8)
            .filter_map(|step| {
                play_guide(
                    &guide,
                    &scale,
                    &song_b,
                    1.0,
                    step * 2,
                    16,
                    0.1,
                    2,
                    5,
                    1.0,
                    false,
                    false,
                    true,
                    &mut rng,
                )
                .into_iter()
                .next()
                .map(|n| n.midi_pitch)
            })
            .collect();
        assert_eq!(a.len(), 8);
        assert_eq!(b.len(), 8);
        assert_ne!(a, b, "alt motif ignored");
    }

    #[test]
    fn motif_shapes_melody() {
        // The same lead with different motifs gives different melodies -
        // seed's motif can be heard.
        let a = motif_pitches([0, 2, 4, 2, 0, 3, 1, 4, 4, 3, 1, 4, 2, 0, 1, 3]);
        let b = motif_pitches([4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4]);
        assert_eq!(a.len(), 8);
        assert_ne!(a, b, "motif ignored");
        // Determinism: the same motif - the same notes.
        assert_eq!(
            a,
            motif_pitches([0, 2, 4, 2, 0, 3, 1, 4, 4, 3, 1, 4, 2, 0, 1, 3])
        );
    }

    #[test]
    fn soft_breath_thins_phrase_but_keeps_pickup() {
        // The rest of a phrase is not silence: the carrier plays less often and more quietly, but
        // the last quarter of the phrase leaves a pickup. Let's compare one
        // and the same phrase with phrase_rest 0.0 and 1.0.
        fn probe(rest: f32) -> (usize, usize) {
            let guide = RoleGuide {
                weights: &[0.9; 16],
                follow_motif: true,
                phrase_rest: rest,
                ..RoleGuide::pitched(&[0.9; 16], 0, 1, 0, &[1.0], 0.7)
            };
            let scale = [48, 50, 52, 53, 55, 57, 59];
            let mut song = test_song(0.5);
            song.measure_index = 2; // second half of the phrase
            song.phrase_index = 3;
            let mut rng = rand::SeedableRng::seed_from_u64(3);
            let (mut head, mut turn) = (0usize, 0usize);
            for step in 0..16 {
                let n = play_guide(
                    &guide,
                    &scale,
                    &song,
                    1.0,
                    step,
                    16,
                    0.1,
                    Slot::Pluck.id(),
                    Slot::Pluck.index(),
                    1.0,
                    false,
                    false,
                    false,
                    &mut rng,
                )
                .len();
                if step >= 12 {
                    turn += n;
                } else {
                    head += n;
                }
            }
            (head, turn)
        }
        let normal = probe(0.0);
        let breath = probe(1.0);
        assert!(
            breath.0 < normal.0,
            "breath did not thin the phrase: {breath:?} vs {normal:?}"
        );
        assert!(breath.1 > 0, "breath dropped the pickup: {breath:?}");
    }

    #[test]
    fn motif_uses_second_half_in_second_bar() {
        // 16-step theme: the first measure of the phrase sings the first 8 steps,
        // second - last 8. Previously, both halves of the phrase were sung by one and
        // the same measure of the theme, so the “two-bar motif” was a fiction.
        let motif: [u8; 16] = [0, 0, 0, 0, 0, 0, 0, 0, 4, 4, 4, 4, 4, 4, 4, 4];
        let guide = RoleGuide {
            weights: &[1.0; 16],
            follow_motif: true,
            ..RoleGuide::pitched(&[1.0; 16], 0, 2, 0, &[1.0], 0.8)
        };
        let scale = [48, 50, 52, 53, 55, 57, 59];
        let probe = |measure: usize| -> Vec<u8> {
            let mut song = test_song(0.5);
            song.stage = 0; // rot = 0: theme index without window shift
            song.measure_index = measure;
            song.motif = motif;
            let mut rng = rand::SeedableRng::seed_from_u64(3);
            (0..8)
                .filter_map(|step| {
                    // id 2 = Kick: anchor-absolute, gate does not breathe.
                    play_guide(
                        &guide,
                        &scale,
                        &song,
                        1.0,
                        step * 2,
                        16,
                        0.1,
                        2,
                        5,
                        1.0,
                        false,
                        false,
                        true,
                        &mut rng,
                    )
                    .into_iter()
                    .next()
                    .map(|n| n.midi_pitch)
                })
                .collect()
        };
        let bar1 = probe(1);
        let bar2 = probe(2);
        assert_eq!(bar1.len(), 8, "first bar must voice all 8 theme slots");
        assert_eq!(bar2.len(), 8, "second bar must voice all 8 theme slots");
        assert!(
            bar1.iter().all(|p| *p == bar1[0]),
            "first bar must sing theme[0..8]: {bar1:?}"
        );
        assert!(
            bar2.iter().all(|p| *p == bar2[0]),
            "second bar must sing theme[8..16]: {bar2:?}"
        );
        assert_ne!(bar1[0], bar2[0], "both bars sang the same half");
    }

    #[test]
    fn theme_contour_survives_the_scale_edge() {
        // The theme outline is not reversed on the top degree of the scale.
        // Previously, the sum of steps was taken modulo the length of the scale, and how
        // as soon as it crossed the top, the line jumped down a seventh
        // and back in the middle of the phrase: there was no arch, there was a “hump.”
        let style = make_style("club").expect("club");
        let scale = style.scale();
        let score = style.arrangement();
        // Line guide: anchor (always plays), motif, high
        // the floor so that the bottom does not fold down an octave.
        let guide = RoleGuide {
            weights: &[1.0; 16],
            follow_motif: true,
            bass_top: 100,
            ..RoleGuide::pitched(&[1.0; 16], 0, 2, 0, &[1.0], 0.8)
        };
        for seed in [
            42u64, 1000, 1007, 1014, 1021, 1028, 1035, 1042, 1049, 2001, 2002,
        ] {
            let mut arranger = crate::arranger::Arranger::with_score(seed, score);
            let song = arranger.advance_measure();
            let mut rng = rand::SeedableRng::seed_from_u64(seed);
            let mut line: Vec<u8> = Vec::new();
            for step in 0..16 {
                // id 2 = Kick: anchor-absolute, gate does not breathe.
                let notes = play_guide(
                    &guide, scale, &song, 1.0, step, 16, 0.1, 2, 2, 1.0, false, false, true,
                    &mut rng,
                );
                if let Some(n) = notes.first() {
                    line.push(n.midi_pitch);
                }
            }
            assert_eq!(line.len(), 16, "seed {seed}: the line must be continuous");
            for w in line.windows(2) {
                let d = (w[1] as i32 - w[0] as i32).abs();
                assert!(
                    d < 7,
                    "seed {seed}: the line breaks with a {d}-semitone jump: {line:?}"
                );
            }
            // The line follows the shape of the theme: adjacent steps of the theme are different
            // pitches, and not one note for the entire measure.
            let distinct: std::collections::BTreeSet<u8> = line.iter().copied().collect();
            assert!(
                distinct.len() >= 3,
                "seed {seed}: the theme is inaudible (one note): {line:?}"
            );
        }
    }

    #[test]
    fn non_anchor_gates_develop_across_phrase_bars() {
        // The second measure of the phrase receives its gate G: drawing
        // develops rather than repeating the first bar verbatim.
        // Anchors are an exception (the skeleton holds the phrase), here the weights are 0.5.
        let guide = RoleGuide {
            weights: &[0.5; 16],
            ..RoleGuide::pitched(&[0.5; 16], 0, 1, 0, &[1.0], 0.7)
        };
        let scale = [48, 50, 52, 53, 55, 57, 59];
        let mask = |seed: u64, measure: usize| -> Vec<bool> {
            let mut song = test_song(0.5);
            song.seed_hash = seed;
            song.measure_index = measure;
            song.phrase_index = 0;
            song.stage = 0;
            let mut rng = rand::SeedableRng::seed_from_u64(1);
            (0..16)
                .map(|step| {
                    !play_guide(
                        &guide, &scale, &song, 1.0, step, 16, 0.1, 4, 3, 1.0, false, false, true,
                        &mut rng,
                    )
                    .is_empty()
                })
                .collect()
        };
        let differing = (0..64u64).filter(|s| mask(*s, 1) != mask(*s, 2)).count();
        assert!(
            differing >= 16,
            "second bar cloned the first: {differing}/64 seeds differ"
        );
    }

    #[test]
    fn section_pole_crossfade_starts_from_previous() {
        // The first measure of the new section is a crossfade: the old beat still sounds
        // on the downbeat and gives way to the new one towards the end of the bar. Next section
        // holding his palette.
        let guide = RoleGuide {
            weights: &[0.0; 8],
            poles: Some(POLES),
            section_poles: true,
            ..RoleGuide::kit(&[0.0; 8], 36, 0.9)
        };
        let mut song = test_song(0.5);
        song.section = SongSection::Drop;
        song.previous_section = SongSection::MainMotif;
        song.section_measure_index = 1;
        let previous = pole_for(POLES, &song, 5, 0, Some(SongSection::MainMotif));
        let current = pole_for(POLES, &song, 5, 0, Some(SongSection::Drop));
        assert_eq!(
            pole_step_weight(&guide, &song, 5, 0, 8),
            previous,
            "first step must still belong to the previous palette"
        );
        assert_eq!(
            pole_step_weight(&guide, &song, 5, 7, 8),
            pole_for(POLES, &song, 5, 7, Some(SongSection::Drop)),
            "last step must arrive at the new palette"
        );
        let mid = pole_step_weight(&guide, &song, 5, 4, 8);
        let t = 4.0 / 7.0;
        let expected = previous * (1.0 - t) + current * t;
        assert!(
            (mid - expected).abs() < 1e-6,
            "crossfade is not linear: {mid} vs {previous}->{current}"
        );
        song.section_measure_index = 2;
        assert_eq!(
            pole_step_weight(&guide, &song, 5, 0, 8),
            pole_for(POLES, &song, 5, 0, Some(SongSection::Drop)),
            "second measure must hold the new palette"
        );
    }

    #[test]
    fn riff_colors_accompaniment_roots() {
        // Riff - general harmony: accompaniment steps move
        // along with its roots, not just its progression.
        let scale = [48, 50, 52, 53, 55, 57, 59];
        let guide = RoleGuide {
            weights: &[1.0; 16],
            ..RoleGuide::pitched(&[1.0; 16], 0, 0, 0, &[1.0], 0.8)
        };
        let mut song = test_song(0.5);
        let cell = (0..64u64)
            .find_map(|seed| {
                song.seed_hash = seed;
                let cell = riff::RiffCell::generate(riff::RiffKind::Deep, &song, 16);
                (0..16)
                    .any(|step| cell.root(&song, step) > 0)
                    .then_some(cell)
            })
            .expect("deep riff never leaves the tonic");
        let mut differs = false;
        for step in 0..16 {
            if cell.root(&song, step) == 0 {
                continue;
            }
            let mut rng_a = rand::SeedableRng::seed_from_u64(3);
            let plain = play_guide(
                &guide,
                &scale,
                &song,
                1.0,
                step,
                16,
                0.1,
                Slot::Bass.id(),
                Slot::Bass.index(),
                1.0,
                false,
                false,
                true,
                &mut rng_a,
            );
            let mut rng_b = rand::SeedableRng::seed_from_u64(3);
            let colored = play_guide_with_riff(
                &guide,
                &scale,
                &song,
                1.0,
                step,
                16,
                0.1,
                Slot::Bass.id(),
                Slot::Bass.index(),
                1.0,
                false,
                false,
                true,
                &Harmony::neutral(&song),
                &cell,
                &mut rng_b,
            );
            if let (Some(a), Some(b)) = (plain.first(), colored.first()) {
                if a.midi_pitch != b.midi_pitch {
                    differs = true;
                    break;
                }
            }
        }
        assert!(differs, "riff roots never reach the accompaniment");
    }

    #[test]
    fn section_poles_vary_groove_by_section() {
        // Poles by sections: one seed - different bits in different sections
        // (the drop has its own), the same section — the same beat (form anchor).
        use crate::arranger::SongSection;
        fn pole_for(seed: u64, section: SongSection) -> Vec<usize> {
            let guide = RoleGuide {
                weights: &[0.0; 8],
                poles: Some(POLES),
                section_poles: true,
                ..RoleGuide::kit(&[0.0; 8], 36, 0.9)
            };
            let mut song = test_song(0.0);
            song.seed_hash = seed;
            song.section = section;
            // We compare the clean palettes of the sections, without the crossfade of the first measure.
            song.previous_section = section;
            let mut rng = rand::SeedableRng::seed_from_u64(1);
            (0..8)
                .filter(|sim| {
                    !play_guide(
                        &guide,
                        &[48],
                        &song,
                        1.0,
                        *sim,
                        8,
                        0.1,
                        2,
                        1,
                        1.0,
                        false,
                        false,
                        true,
                        &mut rng,
                    )
                    .is_empty()
                })
                .collect()
        }
        use crate::arranger::SongSection::*;
        // Determinism: same section, same groove.
        assert_eq!(
            pole_for(11, Drop),
            pole_for(11, Drop),
            "section groove not stable"
        );
        // At least one seed from the first 16 sections gives different bits.
        let varied = (0..16u64).any(|seed| {
            let a = pole_for(seed, MainMotif);
            pole_for(seed, Drop) != a
                || pole_for(seed, BuildUp) != a
                || pole_for(seed, PostChorus) != a
        });
        assert!(varied, "section poles never vary the groove");
    }

    #[test]
    fn prog_rhythm_varies_by_seed() {
        // Harmonic rhythm - the seed's signature: 2, 4 and 8 are found,
        // the same seed always gives the same rhythm.
        let mut seen = std::collections::HashSet::new();
        for seed in 0..32u64 {
            let h = crate::arranger::splitmix64(seed ^ 0x51ED);
            let len = prog_bar_len(h);
            assert!([2, 4, 8].contains(&len), "bad rhythm: {len}");
            assert_eq!(len, prog_bar_len(h), "not deterministic");
            seen.insert(len);
        }
        assert_eq!(seen.len(), 3, "rhythm never varies: {seen:?}");
    }

    #[test]
    fn styles_own_harmony_key_and_density() {
        // The style sets the harmonic world, and the seed selects the option inside:
        // progressions are diatonic in their mode, rhythm and tonality are in style,
        // The density is different, but the seeds are not compressed into one stroke.
        let expected = [
            ("club", 0, 1.0),
            ("lofi", 2, 0.78),
        ];
        for (name, key, density) in expected {
            let style = make_style(name).expect("known style");
            assert_eq!(style.key_shift(), key, "{name}: key");
            assert!((style.density() - density).abs() < 1e-6, "{name}: density");
            let len = style.scale().len();
            assert!(!style.progressions().is_empty(), "{name}: no progressions");
            for prog in style.progressions() {
                for degree in prog {
                    assert!(
                        (*degree as usize) < len,
                        "{name}: degree {degree} outside scale of {len}"
                    );
                }
            }
            assert!(!style.harmonic_rhythm().is_empty(), "{name}: no rhythm");
            assert!(
                style
                    .harmonic_rhythm()
                    .iter()
                    .all(|b| [2, 4, 8].contains(b)),
                "{name}: bad harmonic rhythm"
            );

            let mut progs = std::collections::HashSet::new();
            let mut rhythms = std::collections::HashSet::new();
            for seed in 0..32u64 {
                let seed_hash = crate::arranger::splitmix64(seed ^ 0x51ED);
                let h = Harmony::for_style(&*style, seed_hash);
                let again = Harmony::for_style(&*style, seed_hash);
                assert_eq!(
                    (h.prog, h.bar_len, h.key_shift),
                    (again.prog, again.bar_len, again.key_shift),
                    "{name}: unstable"
                );
                progs.insert(h.prog);
                rhythms.insert(h.bar_len);
                assert!(h.bar_len >= 2, "{name}: harmony churns every bar");
            }
            assert!(progs.len() >= 3, "{name}: seeds share one progression");
            if style.harmonic_rhythm().len() > 1 {
                assert!(rhythms.len() >= 2, "{name}: seeds share one rhythm");
            }
        }
    }

    #[test]
    fn strong_beats_hold_chord_tones() {
        // Downbeats (quarters) sing the written step without jitter
        // (skeleton of harmony), weak ones - with wandering (passing).
        // Same mechanism as even/odd in Markov sampling v1,
        // but deterministic and without separate code for the instrument.
        fn pitch(jitter: usize, sim: usize) -> u8 {
            // Kick-anchor probe (id 2): kick anchors do not pass the gate —
            // There is clear height in the frame without breathing gaps.
            let guide = RoleGuide {
                weights: &[1.0; 8],
                ..RoleGuide::pitched(&[1.0; 8], 1, 0, jitter, &[1.0], 0.8)
            };
            let scale = [48, 50, 52, 53, 55, 57, 59];
            let song = test_song(0.5);
            let mut rng = rand::SeedableRng::seed_from_u64(3);
            play_guide(
                &guide, &scale, &song, 1.0, sim, 8, 0.1, 2, 0, 1.0, false, false, true, &mut rng,
            )
            .into_iter()
            .next()
            .map(|n| n.midi_pitch)
            .expect("strong beat silent")
        }
        // Quarters (0 and 4 with a grid of 8): jitter collapsed - the pitch is the same,
        // that the guide has no jitter.
        assert_eq!(pitch(2, 0), pitch(0, 0), "downbeat drifts");
        assert_eq!(pitch(2, 4), pitch(0, 4), "third beat drifts");
        // At least one weak beat (odd ones with a grid of 8) wanders -
        // otherwise ornament is dead. Deterministic: either always or never.
        let moved = (1..8).any(|sim| sim % 2 == 1 && pitch(2, sim) != pitch(0, sim));
        assert!(moved, "passing tones never wander");
    }

    #[test]
    fn section_end_closes_on_tonic() {
        // Cadance: The last step of the section is the tonic, regardless of the motif.
        // The section closes (the phrase came somewhere), the next
        // opens with its theme. Test by anchor carrier (id 2):
        // There are no gates, the frame has pure height.
        fn last_step_pitch(motif: [u8; 16], last_measure: bool) -> u8 {
            let guide = RoleGuide {
                weights: &[1.0; 16],
                follow_motif: true,
                ..RoleGuide::pitched(&[1.0; 16], 0, 2, 0, &[1.0], 0.8)
            };
            let scale = [48, 50, 52, 53, 55, 57, 59];
            let mut song = test_song(0.5);
            song.motif = motif;
            song.is_last_measure = last_measure;
            let mut rng = rand::SeedableRng::seed_from_u64(3);
            play_guide(
                &guide, &scale, &song, 1.0, 15, 16, 0.1, 2, 5, 1.0, false, false, true, &mut rng,
            )
            .into_iter()
            .next()
            .map(|n| n.midi_pitch)
            .expect("cadence step silent")
        }
        // The high motif at the end of the section falls on the same tonic,
        // which is a low motif (both are level 0).
        assert_eq!(
            last_step_pitch([4; 16], true),
            last_step_pitch([0; 16], true),
            "cadence misses tonic"
        );
        // But at the end of the section there is no cadence: the high motif is high.
        assert_ne!(
            last_step_pitch([4; 16], true),
            last_step_pitch([4; 16], false),
            "cadence fires mid-section"
        );
    }

    const POLE_A: &[f32] = &[1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0];
    const POLE_B: &[f32] = &[0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0];
    const POLES: &[&[f32]] = &[POLE_A, POLE_B];

    fn pole_steps(seed_hash: u64, slot_idx: usize) -> Vec<usize> {
        let guide = RoleGuide {
            weights: &[0.0; 8],
            poles: Some(POLES),
            ..RoleGuide::kit(&[0.0; 8], 36, 0.9)
        };
        let mut song = test_song(0.0);
        song.seed_hash = seed_hash;
        let mut rng = rand::SeedableRng::seed_from_u64(1);
        (0..8)
            .filter(|step| {
                !play_guide(
                    &guide,
                    &[48],
                    &song,
                    1.0,
                    *step,
                    8,
                    0.1,
                    2,
                    slot_idx,
                    1.0,
                    false,
                    false,
                    true,
                    &mut rng,
                )
                .is_empty()
            })
            .collect()
    }

    #[test]
    fn pads_play_triads() {
        // The Chords pad sounds as a triad rather than as a single note.
        let guide = RoleGuide {
            weights: &[1.0; 8],
            ..RoleGuide::pitched(&[1.0; 8], 0, 0, 0, &[2.0, 4.0], 0.6)
        };
        let scale = [48, 50, 52, 53, 55, 57, 59];
        let song = test_song(0.5);
        let mut rng = rand::SeedableRng::seed_from_u64(9);
        // id 3 = Pad (Chords role), slot index 2.
        let notes = play_guide(
            &guide, &scale, &song, 1.0, 0, 8, 0.1, 3, 2, 1.0, false, false, true, &mut rng,
        );
        assert_eq!(notes.len(), 3, "pad must voice a triad");
        let mut pitches: Vec<u8> = notes.iter().map(|n| n.midi_pitch).collect();
        pitches.sort_unstable();
        // The third and fifth are above the root, all within an octave.
        assert!(pitches[1] > pitches[0] && pitches[2] > pitches[1]);
        assert!(pitches[2] - pitches[0] <= 12, "triad too wide: {pitches:?}");
    }

    #[test]
    fn leads_never_exceed_d6() {
        // Ice whistling is prohibited: leads of all styles ≤ 86 (D6).
        use crate::instruments::{InstrumentRole, Slot};
        for name in style_names() {
            let style = make_style(name).expect("known style");
            let ensemble = style.ensemble();
            let active: Vec<usize> = Slot::ALL
                .iter()
                .filter(|s| ensemble.contains(s))
                .map(|s| s.id())
                .collect();
            let mut rng = rand::SeedableRng::seed_from_u64(424242);
            let mut arranger = crate::arranger::Arranger::new(424242);
            let mut song = arranger.advance_measure();
            for step in 0..128 {
                let spm = style.steps_per_measure();
                let sim = step % spm;
                if sim == 0 && step > 0 {
                    song = arranger.advance_measure();
                }
                let mut sctx = StepContext {
                    step,
                    step_in_measure: sim,
                    base_step_secs: style.base_step_secs(),
                    active_tracks: &active,
                    song: &song,
                    rng: &mut rng,
                    intensity: 1.0,
                };
                for note in render_style(&*style, &mut sctx) {
                    let role = Slot::from_id(note.instrument_id).map(|s| s.role());
                    if matches!(role, Some(InstrumentRole::Lead)) {
                        assert!(
                            note.midi_pitch <= 86,
                            "{name}: lead too high: {}",
                            note.midi_pitch
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn bass_stays_deep() {
        // Sub below: bass notes in the cap style register (sub 45,
        // rock riffs higher - 52). Those that have crawled away are folded octave down.
        use crate::instruments::{InstrumentRole, Slot};
        for (name, cap) in [("club", 45), ("lofi", 45)] {
            let style = make_style(name).expect("known style");
            let ensemble = style.ensemble();
            let active: Vec<usize> = Slot::ALL
                .iter()
                .filter(|s| ensemble.contains(s))
                .map(|s| s.id())
                .collect();
            let mut rng = rand::SeedableRng::seed_from_u64(424242);
            let mut arranger = crate::arranger::Arranger::new(424242);
            let mut song = arranger.advance_measure();
            let mut heard = 0;
            for step in 0..256 {
                let spm = style.steps_per_measure();
                let sim = step % spm;
                if sim == 0 && step > 0 {
                    song = arranger.advance_measure();
                }
                let mut sctx = StepContext {
                    step,
                    step_in_measure: sim,
                    base_step_secs: style.base_step_secs(),
                    active_tracks: &active,
                    song: &song,
                    rng: &mut rng,
                    intensity: 1.0,
                };
                for note in render_style(&*style, &mut sctx) {
                    let role = Slot::from_id(note.instrument_id).map(|s| s.role());
                    if matches!(role, Some(InstrumentRole::Bass)) {
                        assert!(
                            (24..=cap).contains(&note.midi_pitch),
                            "{name}: bass out of range: {}",
                            note.midi_pitch
                        );
                        heard += 1;
                    }
                }
            }
            assert!(heard > 0, "{name}: bass never plays");
        }
    }

    #[test]
    fn poles_pick_groove_by_seed() {
        // One pole for the entire track: steps only from POLE_A
        // or only from POLE_B. The beat is written and sticks.
        for seed_hash in [0u64, 1, 7, 42, 1000] {
            let steps = pole_steps(seed_hash, 1);
            let in_a = steps.iter().all(|s| POLE_A[*s] > 0.0);
            let in_b = steps.iter().all(|s| POLE_B[*s] > 0.0);
            assert!(in_a || in_b, "seed {seed_hash}: mixed poles {steps:?}");
        }
        // Different seeds choose different poles.
        let grooves: std::collections::HashSet<Vec<usize>> =
            (0..8u64).map(|s| pole_steps(s, 1)).collect();
        assert!(grooves.len() > 1, "pole never varies");
        // The same seed = the same groove (repetition, the beat is written).
        assert_eq!(pole_steps(42, 1), pole_steps(42, 1));
    }

    #[test]
    fn tonic_walk_is_bounded_and_never_loops() {
        // Tonic: start at zero, determinism, step ≤ step between pairs.
        // Direct convolution of steps (O(n) standard, for tests only).
        fn walk(seed_hash: u64, pair: usize, span: usize) -> usize {
            let mut pos: i64 = 0;
            for p in 0..pair {
                pos += crate::arranger::drift_step(seed_hash, p);
            }
            pos.rem_euclid(span.max(1) as i64) as usize
        }
        assert_eq!(walk(42, 0, 7), 0);
        assert_eq!(walk(42, 5, 7), walk(42, 5, 7));
        for seed in [0u64, 1, 42, 777] {
            for ph in 0..200usize {
                let a = walk(seed, ph, 7) as i64;
                let b = walk(seed, ph + 1, 7) as i64;
                let d = (b - a).rem_euclid(7).min((a - b).rem_euclid(7));
                assert!(d <= 1, "seed {seed} phrase {ph}: jump {d}");
            }
        }
        // Non-periodicity: there is no period on a window of 200 pairs ≤ 32
        // (linear +1/pair would give a period of 7 - a scale-length cycle).
        for seed in [5u64, 12345] {
            let path: Vec<usize> = (0..200).map(|ph| walk(seed, ph, 7)).collect();
            let loops = (1..=32).any(|t| path.iter().zip(path.iter().skip(t)).all(|(a, b)| a == b));
            assert!(!loops, "seed {seed}: walk loops");
        }
        // The arranger's incremental walk coincides with the straight line
        // convolution on a long run (600 measures ≈ 75+ pairs of visits):
        // O(1) per measure without changing the music.
        for seed in [0u64, 7, 424242] {
            let seed_hash = crate::arranger::splitmix64(seed ^ 0x51ED);
            let mut arranger = crate::arranger::Arranger::new(seed);
            for _ in 0..600 {
                let ctx = arranger.advance_measure();
                let mut pos: i64 = 0;
                for p in 0..ctx.stage / 2 {
                    pos += crate::arranger::drift_step(seed_hash, p);
                }
                assert_eq!(
                    ctx.tonic_walk, pos,
                    "seed {seed} stage {}: incremental drift diverged",
                    ctx.stage
                );
            }
        }
    }

    #[test]
    fn motif_skeleton_holds_window_develops_across_windows() {
        // Common Phrase: Phrases from the same section share a skeleton (theme window -
        // section visit, tonic - a couple of visits) - solid melody,
        // but is not re-dealt. Development happens across sections: phrase 0 vs 8
        // They sound different, and within 64 phrases they don’t fit into one pattern.
        // Kick role (id 2): absolute anchor, no gates - in the frame
        // pure height skeleton without breathing gaps.
        // Test world: the section lasts 4 phrases (stage = phrase / 4).
        fn phrase_pitches(phrase: usize) -> Vec<u8> {
            let guide = RoleGuide {
                weights: &[1.0; 16],
                follow_motif: true,
                ..RoleGuide::pitched(&[1.0; 16], 0, 2, 0, &[1.0], 0.8)
            };
            let scale = [48, 50, 52, 53, 55, 57, 59];
            let mut song = test_song(0.5);
            song.phrase_index = phrase;
            song.stage = phrase / 4;
            let mut rng = rand::SeedableRng::seed_from_u64(3);
            (0..8)
                .filter_map(|step| {
                    play_guide(
                        &guide,
                        &scale,
                        &song,
                        1.0,
                        step * 2,
                        16,
                        0.1,
                        2,
                        5,
                        1.0,
                        false,
                        false,
                        true,
                        &mut rng,
                    )
                    .into_iter()
                    .next()
                    .map(|n| n.midi_pitch)
                })
                .collect()
        }
        let p0 = phrase_pitches(0);
        assert_eq!(p0.len(), 8);
        assert_eq!(p0, phrase_pitches(0), "same phrase must repeat");
        assert_eq!(p0, phrase_pitches(1), "same section shares skeleton");
        assert_ne!(p0, phrase_pitches(8), "sections must develop");
        // Development without looping: lines in 64 phrases (16 sections)
        // more than single rotation windows - tonic wandering
        // tears the template.
        let distinct: std::collections::HashSet<Vec<u8>> = (0..64).map(phrase_pitches).collect();
        assert!(
            distinct.len() > 8,
            "skeleton loops: {} distinct",
            distinct.len()
        );
    }

    #[test]
    fn motif_fallback_hands_theme_to_first_melodic_slot() {
        // Mute all theme carriers does not kill the motif: keys (not carrier)
        // with pickup they sing the same theme as the guitar, step by step.
        use crate::instruments::Slot;
        let style = make_style("lofi").expect("known style");
        let guides = style.slot_guides();
        let scale = style.scale();
        let spm = style.steps_per_measure();
        let ki = Slot::ALL.iter().position(|s| *s == Slot::Keys).unwrap();
        let song = test_song(0.5);
        // Style sets tonality and density: we set the standard with the same
        // in the same harmonic style as the engine, otherwise height comparison
        // will compare different tones.
        let harmony = Harmony::for_style(&*style, song.seed_hash);
        // Reference line of the motif: the anchor carrier sounds at every step.
        let anchor_motif = RoleGuide {
            weights: &[1.0; 16],
            follow_motif: true,
            ..RoleGuide::pitched(&[1.0; 16], 0, 1, 0, &[1.0], 0.8)
        };
        let mut rng = rand::SeedableRng::seed_from_u64(21);
        // We take the standard with tolerances: the anchor breathes (0.95) - a couple of steps
        // out of 16 may be silent, reconciliation is carried out according to those who sounded.
        let full_line: Vec<(usize, u8)> = (0..16)
            .filter_map(|step| {
                play_guide_with_riff(
                    &anchor_motif,
                    scale,
                    &song,
                    1.0,
                    step,
                    spm,
                    0.1,
                    6,
                    5,
                    1.0,
                    false,
                    false,
                    true,
                    &harmony,
                    &riff::RiffCell::NONE,
                    &mut rng,
                )
                .into_iter()
                .next()
                .map(|n| (step, n.midi_pitch))
            })
            .collect();
        assert!(
            full_line.len() >= 12,
            "anchor lost face: {}/16",
            full_line.len()
        );
        // Keys without a pick-up and with a pick-up are different lines.
        let line = |force: bool| -> Vec<(usize, u8)> {
            let mut rng = rand::SeedableRng::seed_from_u64(22);
            (0..16)
                .filter_map(|step| {
                    play_guide_with_riff(
                        &guides[ki],
                        scale,
                        &song,
                        1.0,
                        step,
                        spm,
                        0.1,
                        Slot::Keys.id(),
                        ki,
                        1.0,
                        force,
                        false,
                        true,
                        &harmony,
                        &riff::RiffCell::NONE,
                        &mut rng,
                    )
                    .into_iter()
                    .next()
                    .map(|n| (step, n.midi_pitch))
                })
                .collect()
        };
        let plain = line(false);
        let forced = line(true);
        assert!(!forced.is_empty(), "fallback silent");
        assert_ne!(plain, forced, "fallback ignored");
        for (step, pitch) in &forced {
            if let Some((_, ref_pitch)) = full_line.iter().find(|(s, _)| s == step) {
                assert_eq!(*pitch, *ref_pitch, "step {step}: not the motif");
            }
        }
        // Engine level: only the keys are active - the pick-up turns on itself,
        // the root of the chord at each step lies on the motif line
        // (the upper thirds of the triad are harmonization of the theme, not departure from it).
        let active = vec![Slot::Keys.id()];
        // Multiple phrases: phrase gate may miss anchor (downbeat
        // breathes) - the contract “the keys alone hold the theme”, and not “in this
        // measure." The pitch is checked only in phrase 0 (the standard is taken there).
        let mut song_m = song.clone();
        let mut rng = rand::SeedableRng::seed_from_u64(23);
        let mut heard = 0;
        for phrase in 0..4usize {
            song_m.phrase_index = phrase;
            let mut sctx = StepContext {
                step: 0,
                step_in_measure: 0,
                base_step_secs: style.base_step_secs(),
                active_tracks: &active,
                song: &song_m,
                rng: &mut rng,
                intensity: 1.0,
            };
            for (sim, expected) in full_line.iter() {
                sctx.step = *sim;
                sctx.step_in_measure = *sim;
                let mut step_pitches = Vec::new();
                for note in render_style(&*style, &mut sctx) {
                    assert_eq!(note.instrument_id, Slot::Keys.id());
                    step_pitches.push(note.midi_pitch);
                }
                if phrase == 0 && !step_pitches.is_empty() {
                    // The first note of the voiceover is the root: the theme is in place.
                    assert_eq!(step_pitches[0], *expected, "render step {sim}: motif lost");
                }
                heard += step_pitches.len();
            }
        }
        assert!(heard > 0, "keys alone play nothing");
    }

    #[test]
    fn empty_chords_thicken_leads_into_dyads() {
        // Empty chords: solo pluck sings in dyads (root+third),
        // the same cry with a live pad - single. The same steps sound:
        // the composition changes the voice acting, not the rhythm. (The guitar is now always
        // triads according to your guide - the fallback does not concern her.)
        use crate::instruments::Slot;
        let style = make_style("lofi").expect("known style");
        let gid = Slot::Pluck.id();
        // In phrases, like a living engine: anchors sometimes breathe past.
        let render = |active: &Vec<usize>| -> Vec<(usize, Vec<u8>)> {
            let mut rng = rand::SeedableRng::seed_from_u64(31);
            let mut out = Vec::new();
            for phrase in 0..4usize {
                let mut song = test_song(0.5);
                song.phrase_index = phrase;
                let mut sctx = StepContext {
                    step: 0,
                    step_in_measure: 0,
                    base_step_secs: style.base_step_secs(),
                    active_tracks: active,
                    song: &song,
                    rng: &mut rng,
                    // Turn it up: we check the quiet cry for voice acting, not for gates
                    // (they do not depend on intensity - the comparison is fair).
                    intensity: 2.0,
                };
                for sim in 0..16 {
                    sctx.step = sim;
                    sctx.step_in_measure = sim;
                    let pitches: Vec<u8> = render_style(&*style, &mut sctx)
                        .into_iter()
                        .filter(|n| n.instrument_id == gid)
                        .map(|n| n.midi_pitch)
                        .collect();
                    out.push((phrase * 16 + sim, pitches));
                }
            }
            out
        };
        let solo = render(&vec![gid]);
        let with_pad = render(&vec![gid, Slot::Pad.id()]);
        let solo_sounding: Vec<_> = solo.iter().filter(|(_, p)| !p.is_empty()).collect();
        assert!(!solo_sounding.is_empty(), "pluck alone silent");
        for (sim, pitches) in &solo_sounding {
            assert_eq!(
                pitches.len(),
                2,
                "solo step {sim}: expected dyad, got {pitches:?}"
            );
            assert_ne!(pitches[0], pitches[1], "dyad in unison");
        }
        // Same seed, same steps - single notes on the pad.
        for ((sim, dyad), (_, single)) in solo.iter().zip(with_pad.iter()) {
            if dyad.is_empty() {
                assert!(single.is_empty(), "step {sim}: gate moved with mix");
            } else {
                assert_eq!(single.len(), 1, "step {sim}: no thinning with pad");
                assert_eq!(single[0], dyad[0], "step {sim}: root moved");
            }
        }
        assert_eq!(solo, render(&vec![gid]), "must repeat");
    }

    #[test]
    fn lofi_kick_stays_sparse() {
        // Boom-bap: kick for 1 + a couple of syncopations, not a machine gun. No more than 4 hits
        // kick and no more than 6 low-end attacks (kick+808+bass) per measure - in any
        // phrases, seeds and guest-808 (pattern doubling is prohibited).
        // Worst case: full default seed mix + form energy peak.
        use crate::instruments::Slot;
        let style = make_style("lofi").expect("known style");
        for seed in [11u64, 12, 42, 777, 424242] {
            let active = palette_active(&*style);
            let mut rng = rand::SeedableRng::seed_from_u64(seed);
            let mut arranger = crate::arranger::Arranger::new(seed);
            let mut song = arranger.advance_measure();
            for _bar in 0..24 {
                let (mut kicks, mut lowend) = (0, 0);
                {
                    let mut sctx = StepContext {
                        step: 0,
                        step_in_measure: 0,
                        base_step_secs: style.base_step_secs(),
                        active_tracks: &active,
                        song: &song,
                        rng: &mut rng,
                        intensity: 1.0,
                    };
                    for sim in 0..16 {
                        sctx.step = sim;
                        sctx.step_in_measure = sim;
                        for note in render_style(&*style, &mut sctx) {
                            if note.instrument_id == Slot::Kick.id() {
                                kicks += 1;
                                lowend += 1;
                            } else if note.instrument_id == Slot::Kick808.id()
                                || note.instrument_id == Slot::Bass.id()
                            {
                                lowend += 1;
                            }
                        }
                    }
                }
                assert!(kicks <= 4, "seed {seed}: {kicks} kicks/bar");
                assert!(lowend <= 6, "seed {seed}: {lowend} low-end/bar");
                song = arranger.advance_measure();
            }
        }
    }

    #[test]
    fn seed_articulation_varies_and_sticks_to_notes() {
        // Articulation: in range, deterministic (seed, slot),
        // differs between seeds and clings to the notes of the guide.
        for seed in [0u64, 1, 42, 777, 424242] {
            for slot in 0..8usize {
                let (a, r) = envelope_shape(seed, slot);
                assert!((0.8..=1.8).contains(&a), "attack out of range: {a}");
                assert!((0.8..=1.6).contains(&r), "release out of range: {r}");
                assert_eq!((a, r), envelope_shape(seed, slot), "not deterministic");
            }
        }
        let shapes: std::collections::HashSet<(u32, u32)> = (0..16u64)
            .map(|s| {
                let (a, r) = envelope_shape(s, 2);
                ((a * 1000.0) as u32, (r * 1000.0) as u32)
            })
            .collect();
        assert!(shapes.len() > 8, "seeds sound alike");
        // Pin: seed 83 - the softest bass of the first 300 (attack ~1.8).
        // The hash path is fair: seed_hash = splitmix(seed ^ 0x51ED), as in Arranger.
        // If the path changes, the pin will fall.
        let h83 = crate::arranger::splitmix64(83 ^ 0x51ED);
        let (soft_a, _) = envelope_shape(h83, 0);
        assert!(soft_a > 1.7, "soft-seed pin moved: {soft_a}");
        // The guide links the shaping seed to the note.
        let guide = RoleGuide {
            weights: &[1.0; 8],
            ..RoleGuide::pitched(&[1.0; 8], 0, 0, 0, &[2.0, 4.0], 0.6)
        };
        let song = test_song(0.5);
        let mut rng = rand::SeedableRng::seed_from_u64(9);
        let notes = play_guide(
            &guide,
            &[48, 50, 52, 53, 55, 57, 59],
            &song,
            1.0,
            0,
            8,
            0.1,
            3,
            2,
            1.0,
            false,
            false,
            true,
            &mut rng,
        );
        assert!(!notes.is_empty());
        let (a, r) = envelope_shape(42, 2);
        assert_eq!((notes[0].attack_mul, notes[0].release_mul), (a, r));
    }

    #[test]
    fn sub_bias_rides_from_guide_to_note() {
        // The style skew goes to the note (club bass 0.8), the default is 0.0.
        use crate::instruments::Slot;
        use club::ClubStyle;
        let guides = ClubStyle::new().slot_guides();
        let bi = Slot::ALL.iter().position(|s| *s == Slot::Bass).unwrap();
        assert_eq!(guides[bi].sub_bias, 0.8);
        let mut guide = RoleGuide {
            weights: &[1.0; 8],
            sub_bias: 0.8,
            ..RoleGuide::pitched(&[1.0; 8], 0, 0, 0, &[1.0], 0.8)
        };
        let song = test_song(0.5);
        let mut rng = rand::SeedableRng::seed_from_u64(9);
        let notes = play_guide(
            &guide,
            &[48, 50, 52, 53, 55, 57, 59],
            &song,
            1.0,
            0,
            8,
            0.1,
            1,
            0,
            1.0,
            false,
            false,
            true,
            &mut rng,
        );
        assert!(!notes.is_empty());
        assert_eq!(notes[0].sub_bias, 0.8);
        guide.sub_bias = 0.0;
        let mut rng = rand::SeedableRng::seed_from_u64(9);
        let notes = play_guide(
            &guide,
            &[48, 50, 52, 53, 55, 57, 59],
            &song,
            1.0,
            0,
            8,
            0.1,
            1,
            0,
            1.0,
            false,
            false,
            true,
            &mut rng,
        );
        assert_eq!(notes[0].sub_bias, 0.0);
    }

    #[test]
    fn style_contract_holds_across_styles() {
        // Phase 1 contract: the face of each style as measurable properties
        // default mix in 24 bars. Freedom to realize within -
        // any refactor is required to keep these numbers (with a margin of ~2x down
        // from measuring: they catch the breakdown, not the taste).
        // Measurements: club kickmax 6, rock 5, lofi 2–3; roles and theme - in PROBE.
        use crate::instruments::Slot;
        use std::collections::HashMap;
        // (style, kick ceiling per bar, role floors for 24 bars, theme floor)
        // Ceilings - according to the densest intentional pole + margin:
        // club drive-eighths give up to 9 hits, this is a feature, not a bug.
        // Club keeps the theme flowing (through the beatmaker’s melody).
        // Bass floor 44, not 48: the piece is longer, and the window is 24 bars
        // Some of the seeds go into a post-chorus, where the bass plays less often.
        type Floors<'a> = &'a [(&'a str, usize)];
        let contract: &[(&str, usize, Floors, usize)] = &[
            (
                "club",
                10,
                &[("kick", 48), ("snare", 24), ("hat", 48), ("bass", 44)],
                24,
            ),
            (
                "lofi",
                4,
                &[
                    ("kick", 12),
                    ("snare", 24),
                    ("hat", 16),
                    ("bass", 12),
                    ("chords", 48),
                    ("lead", 8),
                    ("texture", 24),
                ],
                8,
            ),
        ];
        for (name, kick_cap, role_floors, motif_floor) in contract {
            let style = make_style(name).expect("style");
            for seed in [11u64, 42, 777] {
                // We check the contract on the default cast of the seed: it is his
                // the listener hears, and not the entire palette of style.
                let active: Vec<usize> = style.cast(seed).iter().map(|s| s.id()).collect();
                let mut rng = rand::SeedableRng::seed_from_u64(seed);
                let mut arranger = crate::arranger::Arranger::new(seed);
                let mut song = arranger.advance_measure();
                let mut kick_max = 0;
                let mut roles: HashMap<&str, usize> = HashMap::new();
                let mut motif_n = 0;
                let guides = style.slot_guides();
                for _bar in 0..24 {
                    let mut kicks = 0;
                    let mut sctx = StepContext {
                        step: 0,
                        step_in_measure: 0,
                        base_step_secs: style.base_step_secs(),
                        active_tracks: &active,
                        song: &song,
                        rng: &mut rng,
                        intensity: 1.0,
                    };
                    for sim in 0..16 {
                        sctx.step = sim;
                        sctx.step_in_measure = sim;
                        for note in render_style(&*style, &mut sctx) {
                            let slot = Slot::from_id(note.instrument_id).unwrap();
                            *roles.entry(slot.role().name()).or_insert(0) += 1;
                            if note.instrument_id == Slot::Kick.id() {
                                kicks += 1;
                            }
                            let gi = Slot::ALL.iter().position(|s| *s == slot).unwrap();
                            if guides[gi].follow_motif {
                                motif_n += 1;
                            }
                        }
                    }
                    kick_max = kick_max.max(kicks);
                    song = arranger.advance_measure();
                }
                assert!(
                    kick_max <= *kick_cap,
                    "{name} seed {seed}: {kick_max} kicks/bar"
                );
                for (role, floor) in *role_floors {
                    let got = roles.get(role).copied().unwrap_or(0);
                    assert!(
                        got >= *floor,
                        "{name} seed {seed}: role {role} lost ({got} < {floor})"
                    );
                }
                assert!(
                    motif_n >= *motif_floor,
                    "{name} seed {seed}: theme gone ({motif_n})"
                );
            }
        }
    }

    #[test]
    fn tonic_shift_spans_five_semitones() {
        // Tonic: ±2 range, determinism, all 5 values occur.
        for h in [0u64, 1, 42, 777, 424242, u64::MAX] {
            let s = tonic_shift_for(h);
            assert!((-2..=2).contains(&s), "shift out of range: {s}");
            assert_eq!(s, tonic_shift_for(h), "not deterministic");
        }
        let keys: std::collections::HashSet<i8> = (0..64u64).map(tonic_shift_for).collect();
        assert_eq!(keys.len(), 5, "tonics missing: {keys:?}");
    }

    #[test]
    fn tonic_shift_transposes_line_intact() {
        // The shift transposes the entire line: the contour is the same, the height is not.
        // Two hashes from the same %21 class: rotation (%7) and mode (%3)
        // are the same, only the shift is different. Gates are different - let's compare
        // only steps sounding in both lines.
        let guide = RoleGuide {
            weights: &[1.0; 16],
            follow_motif: true,
            ..RoleGuide::pitched(&[1.0; 16], 0, 1, 0, &[1.0], 0.8)
        };
        let scale = [48, 50, 52, 53, 55, 57, 59];
        let pitch = |hash: u64, step: usize| -> Option<u8> {
            let mut song = test_song(0.5);
            song.seed_hash = hash;
            let mut rng = rand::SeedableRng::seed_from_u64(3);
            play_guide(
                &guide, &scale, &song, 1.0, step, 16, 0.1, 6, 5, 1.0, false, false, true, &mut rng,
            )
            .into_iter()
            .next()
            .map(|n| n.midi_pitch)
        };
        let mut pair = None;
        'search: for h1 in 0..200u64 {
            for h2 in (h1 + 1)..200u64 {
                if h1 % 21 != h2 % 21 {
                    continue;
                }
                if tonic_shift_for(h1) == tonic_shift_for(h2) {
                    continue;
                }
                // The octave of the theme is the same: we measure the shift of the tonic, not the octave.
                if motif_octave(h1) != motif_octave(h2) {
                    continue;
                }
                pair = Some((h1, h2));
                break 'search;
            }
        }
        let (h1, h2) = pair.expect("no pair in range");
        let delta = tonic_shift_for(h2) as i16 - tonic_shift_for(h1) as i16;
        let mut checked = 0;
        for step in (0..16).step_by(2) {
            if let (Some(x), Some(y)) = (pitch(h1, step), pitch(h2, step)) {
                assert_eq!(y as i16 - x as i16, delta, "contour broken");
                checked += 1;
            }
        }
        assert!(checked >= 3, "too few common steps");
    }

    #[test]
    fn lofi_guitar_fingerpicks_arp() {
        // Fingerpicking: one interval per hit, cycling through the triad
        // (sounding step = 1 note). The stacked chords gave a clicking sound
        // mush instead of a string.
        use crate::instruments::Slot;
        let style = lofi::LofiStyle::new();
        assert!(
            (1.4..=1.8).contains(&style.swing_ratio()),
            "swing out of dusty band"
        );
        let guides = style.slot_guides();
        let gi = Slot::ALL.iter().position(|s| *s == Slot::Guitar).unwrap();
        assert!(guides[gi].arp, "guitar must fingerpick");
        let iv = guides[gi].chord.expect("arp source");
        assert_eq!(iv.to_vec(), vec![0i8, 2, 4]);
        let active = vec![Slot::Guitar.id()];
        let mut rng = rand::SeedableRng::seed_from_u64(17);
        let mut heard: Vec<u8> = Vec::new();
        for phrase in 0..8usize {
            let mut song = test_song(0.5);
            song.phrase_index = phrase;
            let mut sctx = StepContext {
                step: 0,
                step_in_measure: 0,
                base_step_secs: style.base_step_secs(),
                active_tracks: &active,
                song: &song,
                rng: &mut rng,
                intensity: 1.0,
            };
            for sim in 0..16 {
                sctx.step = sim;
                sctx.step_in_measure = sim;
                let notes = render_style(&style, &mut sctx);
                // One interval per hit, no stack.
                assert!(notes.len() <= 1, "phrase {phrase} step {sim}: stack");
                heard.extend(notes.iter().map(|n| n.midi_pitch));
            }
        }
        // The picking walks along the strings, and does not stomp on one note.
        let distinct: std::collections::HashSet<u8> = heard.iter().copied().collect();
        assert!(heard.len() >= 24, "strumming gone: {}/8 bars", heard.len());
        assert!(distinct.len() > 2, "arp stuck: {distinct:?}");
    }

    #[test]
    fn bass_line_stays_near_roots() {
        // The bass follows the progression, but the harmonic rhythm
        // 8 bars: the roots stand and hold the phrase, rather than moving every 2 bars.
        let guide = RoleGuide {
            weights: &[1.0; 8],
            ..RoleGuide::pitched(&[1.0; 8], 0, -1, 0, &[1.0], 0.8)
        };
        let scale = [48, 51, 53, 55, 58];
        let mut pitches = Vec::new();
        for measure in (0..32usize).step_by(4) {
            let mut song = test_song(0.5);
            song.measure_index = measure;
            song.prog = [0, 3, 1, 2];
            let mut rng = rand::SeedableRng::seed_from_u64(3);
            if let Some(n) = play_guide(
                &guide, &scale, &song, 1.0, 0, 8, 0.1, 1, 0, 1.0, false, false, true, &mut rng,
            )
            .into_iter()
            .next()
            {
                pitches.push(n.midi_pitch);
            }
        }
        assert!(pitches.len() >= 6, "bass gone: {pitches:?}");
        // The bass follows the progression (there is a spread - not a drone),
        // but sits in the register (fold holds ≤ bass_top) and does not walk
        // on the fingerboard: spread within a sixth.
        let spread = pitches.iter().max().unwrap() - pitches.iter().min().unwrap();
        assert!(
            (4..=10).contains(&spread),
            "bass drones or wanders: {pitches:?}"
        );
        assert!(
            pitches.iter().all(|p| *p <= 45),
            "bass escaped register: {pitches:?}"
        );
    }

    #[test]
    fn role_plans_are_shared_by_performers() {
        // Role plan - one slice for all performers (ptr-equality,
        // meanings do not match): club pluck+guitar share the strum pattern.
        use crate::instruments::Slot;
        let lofi = make_style("lofi").expect("lofi");
        let guides = lofi.slot_guides();
        let at = |s: Slot| {
            Slot::ALL
                .iter()
                .position(|x| *x == s)
                .map(|i| &guides[i])
                .unwrap()
        };
        assert!(std::ptr::eq(
            at(Slot::Pluck).weights.as_ptr(),
            at(Slot::Guitar).weights.as_ptr()
        ));
    }

    #[test]
    fn ensemble_bass_yields_to_kick_in_club() {
        use crate::instruments::{InstrumentRole, Slot};
        for name in ["club"] {
            let style = make_style(name).expect("known style");
            let active = palette_active(&*style);
            let mut rng = rand::SeedableRng::seed_from_u64(7777);
            let mut arranger = crate::arranger::Arranger::new(7777);
            let mut song = arranger.advance_measure();
            for step in 0..64usize {
                let spm = style.steps_per_measure();
                let sim = step % spm;
                if sim == 0 && step > 0 {
                    song = arranger.advance_measure();
                }
                let mut sctx = StepContext {
                    step,
                    step_in_measure: sim,
                    base_step_secs: style.base_step_secs(),
                    active_tracks: &active,
                    song: &song,
                    rng: &mut rng,
                    intensity: 1.0,
                };
                let notes = render_style(&*style, &mut sctx);
                let kick_here = notes.iter().any(|n| {
                    Slot::from_id(n.instrument_id)
                        .map(|s| s.role() == InstrumentRole::Kick)
                        .unwrap_or(false)
                });
                if kick_here {
                    let bass_here = notes.iter().any(|n| {
                        Slot::from_id(n.instrument_id)
                            .map(|s| s.role() == InstrumentRole::Bass)
                            .unwrap_or(false)
                    });
                    assert!(!bass_here, "{name}: bass attacks on a kick step (step {step})");
                }
            }
        }
    }

    #[test]
    fn style_contract_limits_melodic_register_in_deep_and_club() {
        use crate::instruments::Slot;
        let ceilings = [("club", 78u8)];
        for (name, ceiling) in ceilings {
            let style = make_style(name).expect("known style");
            let active = palette_active(&*style);
            let mut rng = rand::SeedableRng::seed_from_u64(999);
            let mut arranger = crate::arranger::Arranger::new(999);
            let mut song = arranger.advance_measure();
            for step in 0..96usize {
                let spm = style.steps_per_measure();
                let sim = step % spm;
                if sim == 0 && step > 0 {
                    song = arranger.advance_measure();
                }
                let mut sctx = StepContext {
                    step,
                    step_in_measure: sim,
                    base_step_secs: style.base_step_secs(),
                    active_tracks: &active,
                    song: &song,
                    rng: &mut rng,
                    intensity: 1.0,
                };
                let notes = render_style(&*style, &mut sctx);
                for n in notes {
                    if let Some(slot) = Slot::from_id(n.instrument_id) {
                        if is_melodic_role(slot.role()) {
                            assert!(
                                n.midi_pitch <= ceiling,
                                "{name}: melodic note {} above ceiling {}",
                                n.midi_pitch,
                                ceiling
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn seed_divergence_stays_high() {
        // Seeds must differ: pitch sets of pairs (step%64, slot,
        // pitch class) are far (Jaccard ≥ 0.4), rhythm sets do not match.
        // Measurements: pitch min 0.50–0.62, rhythm min 0.26–0.32.
        use std::collections::HashSet;
        type PitchSig = HashSet<(usize, usize, u8)>;
        type RhythmSig = HashSet<(usize, usize)>;
        fn signature(style_name: &str, seed: u64) -> (PitchSig, RhythmSig) {
            let style = make_style(style_name).expect("style");
            let active: Vec<usize> = style.cast(seed).iter().map(|s| s.id()).collect();
            let mut rng = rand::SeedableRng::seed_from_u64(seed);
            let mut arranger = crate::arranger::Arranger::new(seed);
            let mut song = arranger.advance_measure();
            let mut pitches = HashSet::new();
            let mut rhythm = HashSet::new();
            for step in 0..256 {
                let spm = style.steps_per_measure();
                let sim = step % spm;
                if sim == 0 && step > 0 {
                    song = arranger.advance_measure();
                }
                let mut sctx = StepContext {
                    step,
                    step_in_measure: sim,
                    base_step_secs: style.base_step_secs(),
                    active_tracks: &active,
                    song: &song,
                    rng: &mut rng,
                    intensity: 1.0,
                };
                for note in render_style(&*style, &mut sctx) {
                    pitches.insert((note.instrument_id, step % 64, note.midi_pitch % 12));
                    rhythm.insert((note.instrument_id, step % 64));
                }
            }
            (pitches, rhythm)
        }
        for name in style_names() {
            let sigs: Vec<_> = (0..8u64).map(|s| signature(name, s)).collect();
            for i in 0..8 {
                for j in (i + 1)..8 {
                    let jac = |a: &PitchSig, b: &PitchSig| {
                        1.0 - a.intersection(b).count() as f32 / a.union(b).count().max(1) as f32
                    };
                    let rjac = |a: &RhythmSig, b: &RhythmSig| {
                        1.0 - a.intersection(b).count() as f32 / a.union(b).count().max(1) as f32
                    };
                    let pd = jac(&sigs[i].0, &sigs[j].0);
                    let rd = rjac(&sigs[i].1, &sigs[j].1);
                    assert!(pd >= 0.4, "{name}: seeds {i}/{j} share pitch ({pd:.2})");
                    assert!(rd > 0.0, "{name}: seeds {i}/{j} share rhythm");
                }
            }
        }
    }

    #[test]
    fn leader_balance_boosts_lead_and_dips_support() {
        // Unified balance: the leader of the melody is louder, the rest are melodic
        // quiet; drums and bass are left untouched. Less than two melodic -
        // there is nothing to balance.
        let lead = crate::instruments::Slot::Pluck.id();
        let pad = crate::instruments::Slot::Pad.id();
        let kick = crate::instruments::Slot::Kick.id();
        let mut notes = vec![
            NoteEvent::new(lead, 60, 0.8, 0.1),
            NoteEvent::new(pad, 60, 0.8, 0.1),
            NoteEvent::new(kick, 36, 0.8, 0.1),
        ];
        let leader = balance_leader(&[lead, pad], Some(lead), 0);
        assert_eq!(leader, Some(lead));
        apply_leader_balance(&mut notes, leader);
        assert!(notes[0].velocity > 0.8, "leader not boosted");
        assert!(notes[1].velocity < 0.8, "support not dipped");
        assert_eq!(notes[2].velocity, 0.8, "drums must stay untouched");
        assert_eq!(balance_leader(&[pad], Some(pad), 0), None);
    }

    #[test]
    fn motif_octave_covers_all_three() {
        // Theme octave from the seed: −1/0/+1 all drop out (otherwise the feature is dead),
        // determined by hash.
        use std::collections::HashSet;
        let seen: HashSet<i8> = (0..64u64)
            .map(|s| motif_octave(crate::arranger::splitmix64(s ^ 0x51ED)))
            .collect();
        assert_eq!(seen, HashSet::from([-1, 0, 1]), "octave stuck: {seen:?}");
        assert_eq!(motif_octave(42), motif_octave(42), "not deterministic");
    }

    #[test]
    fn theme_leader_rotates_deterministically() {
        // Leadership: rotation based on section visits (doesn’t get stuck on one),
        // deterministic, empty = no one to lead, alone = always him.
        // Inside the section, the leader stands - the composition is not shuffled.
        use std::collections::HashSet;
        let carriers = vec![6usize, 18];
        let leaders: HashSet<usize> = (0..16usize)
            .map(|s| theme_leader(&carriers, 42, s).unwrap())
            .collect();
        assert!(leaders.len() > 1, "leadership stuck: {leaders:?}");
        for l in &leaders {
            assert!(carriers.contains(l), "stranger leads: {l}");
        }
        assert_eq!(
            theme_leader(&carriers, 42, 3),
            theme_leader(&carriers, 42, 3),
            "not deterministic"
        );
        assert_eq!(theme_leader(&[], 42, 0), None);
        assert_eq!(theme_leader(&[6], 42, 9), Some(6));
    }

    #[test]
    fn window_octave_shifts_theme_by_octave() {
        // Contrast between pairs of sections: same theme line, shifted evenly
        // per octave - verse/chorus in registers. Flat motif: rotation
        // neutral; uniform shift (integer shape), octave value
        // with tolerance for tonic drift. Sections 0 vs 2 (different pairs).
        // Direction - from register (down for highs): fold 86 otherwise
        // The upward shift eats up entirely and the sections sound the same.
        let guide = RoleGuide {
            weights: &[1.0; 16],
            follow_motif: true,
            ..RoleGuide::pitched(&[1.0; 16], 0, 2, 0, &[1.0], 0.8)
        };
        let scale = [48, 50, 52, 53, 55, 57, 59];
        let pitches = |stage: usize| -> Vec<(usize, u8)> {
            let mut song = test_song(0.5);
            song.stage = stage;
            song.motif = [1; 16];
            let mut rng = rand::SeedableRng::seed_from_u64(3);
            (0..16)
                .filter_map(|sim| {
                    play_guide(
                        &guide, &scale, &song, 1.0, sim, 16, 0.1, 6, 5, 1.0, false, false, true,
                        &mut rng,
                    )
                    .into_iter()
                    .next()
                    .map(|n| (sim, n.midi_pitch))
                })
                .collect()
        };
        let a = pitches(0);
        let b = pitches(2);
        assert!(!a.is_empty() && !b.is_empty(), "theme gone");
        // Compare only steps that sound in both lines (the anchor breathes — skips
        // possible, but there are few of them and they do not break the line).
        let shared: Vec<(u8, u8)> = a
            .iter()
            .filter_map(|(s, p)| b.iter().find(|(t, _)| t == s).map(|(_, q)| (*p, *q)))
            .collect();
        assert!(
            shared.len() >= 12,
            "window lost notes: {} shared",
            shared.len()
        );
        let shift = shared[0].1 as i16 - shared[0].0 as i16;
        // Value - octave ± tonic drift (2 wandering steps - no more
        // ±2 steps ≈ ±4 semitones). For seed 42 it comes out to −9.
        assert!(
            (8..=16).contains(&shift.abs()),
            "window must move ~an octave, got {shift}: {a:?} -> {b:?}"
        );
        for (x, y) in &shared {
            assert_eq!(*y as i16 - *x as i16, shift, "shape bent");
        }
    }

    fn probe_melody(melody: Melody, seed: u64, last: bool) -> Vec<(usize, u8, f32)> {
        let guide = RoleGuide {
            weights: &[1.0; 16],
            follow_motif: true,
            gates: &[1.0, 2.0, 4.0],
            ..RoleGuide::pitched(&[1.0; 16], 0, 1, 0, &[1.0, 2.0, 4.0], 0.8)
        };
        let scale = [48, 50, 52, 53, 55, 57, 59];
        let mut song = test_song(0.5);
        song.seed_hash = seed;
        song.stage = 0;
        song.is_last_measure = last;
        let harmony = Harmony {
            melody,
            ..Harmony::neutral(&song)
        };
        let mut rng = rand::SeedableRng::seed_from_u64(3);
        (0..16)
            .filter_map(|step| {
                play_guide_with_riff(
                    &guide, &scale, &song, 1.0, step, 16, 0.1, 2, 5, 1.0, false, false, true,
                    &harmony, &riff::RiffCell::NONE, &mut rng,
                )
                .into_iter()
                .next()
                .map(|n| (step, n.midi_pitch, n.duration_secs))
            })
            .collect()
    }

    #[test]
    fn melody_mask_marks_theme_onsets_in_readable_bit_order() {
        // The mask is written in the highest tetrad in the first eighths of the phrase.
        assert_eq!(phrase_mask(0b1000_0000_0000_0000), 1);
        assert_eq!(phrase_mask(0b0000_0000_0000_0001), 0x8000);
        let neutral = Melody::DENSE;
        assert_eq!(neutral.onsets, u16::MAX);
        assert!(!neutral.phrased);
        assert_eq!(neutral.sustain, 1.0);
    }

    #[test]
    fn melody_mask_follows_the_section() {
        // The mask is not a constant of style: drop and post-chorus change the accents
        // phrases, and within a section the shift is stable for a couple of visits.
        let base = phrase_mask(0b1000_0001_0000_0000);
        let mut song = test_song(0.5);
        song.section = SongSection::MainMotif;
        song.stage = 0;
        let main = phrase_onsets(base, &song);
        song.section = SongSection::Drop;
        let drop = phrase_onsets(base, &song);
        assert_ne!(main, drop, "drop keeps the main phrase mask");
        song.section = SongSection::Drop;
        assert_eq!(drop, phrase_onsets(base, &song), "mask not deterministic");
        song.stage = 2;
        assert_ne!(drop, phrase_onsets(base, &song), "mask never develops");
    }

    #[test]
    fn melody_mask_accents_phrase_but_does_not_silence_theme() {
        // The mask is accents, not a switch: a sparse phrase plays
        // less often dense, but the theme can be heard on the “off” beats.
        let guide = RoleGuide {
            weights: &[0.7; 16],
            follow_motif: true,
            ..RoleGuide::pitched(&[0.7; 16], 0, 1, 0, &[1.0], 0.8)
        };
        let scale = [48, 50, 52, 53, 55, 57, 59];
        let song = test_song(0.5);
        let base = Harmony::neutral(&song);
        let dense = Harmony {
            melody: Melody::DENSE,
            ..base
        };
        let sparse = Harmony {
            melody: Melody {
                onsets: phrase_mask(0b1000_0000_0000_0001),
                phrased: true,
                ..Melody::DENSE
            },
            ..base
        };
        let count = |h: &Harmony| -> usize {
            (0..16u64)
                .map(|seed| {
                    let mut rng = rand::SeedableRng::seed_from_u64(seed);
                    (0..16)
                        .filter(|step| {
                            !play_guide_with_riff(
                                &guide, &scale, &song, 1.0, *step, 16, 0.1, 2, 5, 1.0, false,
                                false, true, h, &riff::RiffCell::NONE, &mut rng,
                            )
                            .is_empty()
                        })
                        .count()
                })
                .sum()
        };
        let full = count(&dense);
        let accented = count(&sparse);
        assert!(accented > 0, "mask silenced the theme");
        assert!(accented < full, "mask did not shape the phrase: {accented}/{full}");
    }

    #[test]
    fn melody_cells_reorder_the_theme() {
        // The cell order is part of that signature: the same steps of the seed, another
        // the order of the blocks gives a different scale. The first visit does not rotate,
        // so that the comparison is fair.
        let straight = probe_melody(
            Melody {
                cells: [0, 1, 2, 3],
                phrased: true,
                ..Melody::DENSE
            },
            7,
            false,
        );
        let crossed = probe_melody(
            Melody {
                cells: [2, 0, 3, 1],
                phrased: true,
                ..Melody::DENSE
            },
            7,
            false,
        );
        let different = straight
            .iter()
            .zip(crossed.iter())
            .filter(|(a, b)| a.1 != b.1)
            .count();
        assert!(different >= 4, "cells ignored: {straight:?} / {crossed:?}");
    }

    #[test]
    fn melody_sustain_stretches_theme_notes() {
        let base = probe_melody(
            Melody {
                phrased: true,
                ..Melody::DENSE
            },
            11,
            false,
        );
        let long = probe_melody(
            Melody {
                sustain: 2.0,
                phrased: true,
                ..Melody::DENSE
            },
            11,
            false,
        );
        assert!(!base.is_empty() && base.len() == long.len());
        for (a, b) in base.iter().zip(long.iter()) {
            assert_eq!(a.0, b.0, "sustain moved note onsets");
            assert!(
                (b.2 / a.2 - 2.0).abs() < 0.01,
                "sustain ignored: {} -> {}",
                a.2,
                b.2
            );
        }
    }

    #[test]
    fn melody_chord_snap_pulls_strong_beats_only() {
        // Attraction pulls the downbeats toward the tones of the chord (here toward
        // steps 1/3), leaves the weak ones passing.
        let scale = [48, 50, 52, 53, 55, 57, 59];
        let guide = RoleGuide {
            weights: &[1.0; 16],
            follow_motif: true,
            ..RoleGuide::pitched(&[1.0; 16], 0, 1, 0, &[1.0], 0.8)
        };
        let song = test_song(0.5);
        let pitches = |snap: f32| -> Vec<(usize, u8)> {
            let harmony = Harmony {
                prog: [2, 2, 2, 2],
                bar_len: 2,
                melody: Melody {
                    chord_snap: snap,
                    phrased: true,
                    ..Melody::DENSE
                },
                ..Harmony::neutral(&song)
            };
            let mut rng = rand::SeedableRng::seed_from_u64(5);
            (0..16)
                .filter_map(|step| {
                    play_guide_with_riff(
                        &guide, &scale, &song, 1.0, step, 16, 0.1, 2, 5, 1.0, false, false, true,
                        &harmony, &riff::RiffCell::NONE, &mut rng,
                    )
                    .into_iter()
                    .next()
                    .map(|n| (step, n.midi_pitch))
                })
                .collect()
        };
        let plain = pitches(0.0);
        let snapped = pitches(1.0);
        let mut moved_strong = 0;
        for (step, pitch) in &plain {
            let other = snapped.iter().find(|(s, _)| s == step).map(|(_, p)| *p);
            if let Some(other) = other {
                if step % 4 == 0 && *pitch != other {
                    moved_strong += 1;
                } else if step % 4 != 0 {
                    assert_eq!(*pitch, other, "weak beat {step} was snapped");
                }
            }
        }
        assert!(moved_strong >= 2, "chord snap did nothing: {moved_strong}");
    }

    #[test]
    fn melody_cadence_survives_an_empty_mask() {
        // Cadence is a written dividing point: it sounds even when the mask
        // The onsets are empty and the weights of the plan are zero, and comes to the tonic.
        let guide = RoleGuide {
            weights: &[0.0; 16],
            follow_motif: true,
            ..RoleGuide::pitched(&[0.0; 16], 0, 1, 0, &[1.0], 0.8)
        };
        let scale = [48, 50, 52, 53, 55, 57, 59];
        let mut song = test_song(0.5);
        song.is_last_measure = true;
        song.motif = [4; 16];
        let harmony = Harmony {
            melody: Melody {
                onsets: 0,
                phrased: true,
                ..Melody::DENSE
            },
            ..Harmony::neutral(&song)
        };
        let mut rng = rand::SeedableRng::seed_from_u64(3);
        let cadence = play_guide_with_riff(
            &guide, &scale, &song, 1.0, 15, 16, 0.1, 2, 5, 1.0, false, false, true, &harmony,
            &riff::RiffCell::NONE, &mut rng,
        );
        assert_eq!(cadence.len(), 1, "cadence lost behind the mask");
        song.motif = [0; 16];
        let mut rng = rand::SeedableRng::seed_from_u64(3);
        let tonic = play_guide_with_riff(
            &guide, &scale, &song, 1.0, 15, 16, 0.1, 2, 5, 1.0, false, false, true, &harmony,
            &riff::RiffCell::NONE, &mut rng,
        );
        assert_eq!(cadence[0].midi_pitch, tonic[0].midi_pitch, "cadence missed tonic");
    }

    #[test]
    fn beat_accent_orders_downbeat_first() {
        // Hierarchy of beats: the downbeat is loudest (the entire first quarter),
        // the third beat - next, weak - as written.
        // The beat is readable even in syncopation.
        assert_eq!(beat_accent(0, 16), 1.25);
        assert_eq!(beat_accent(3, 16), 1.25);
        assert_eq!(beat_accent(4, 16), 1.0);
        assert!(beat_accent(8, 16) > beat_accent(4, 16));
        assert_eq!(beat_accent(12, 16), 1.0);
    }

    #[test]
    fn phrase_arc_is_bounded_and_breathes() {
        // The arc of phrases does not accumulate: eight phrases - inhale and exhale, multiplier
        // always at 0.94..1.0. Otherwise, by the middle of the piece all the notes are stuck
        // into a clip, and the dynamics would flatten into a shelf.
        let arc: Vec<f32> = (0..16).map(phrase_arc).collect();
        assert!(arc.iter().all(|v| (0.94..=1.0).contains(v)), "{arc:?}");
        assert!(arc[3] > arc[0], "arc does not rise: {arc:?}");
        assert!(arc[7] < arc[3], "arc does not exhale: {arc:?}");
        assert!((arc[0] - arc[8]).abs() < 1e-6, "arc does not reset");
        assert!((arc[7] - arc[15]).abs() < 1e-6, "arc does not cycle");
    }

    #[test]
    fn swing_varies_by_seed_within_band() {
        // Seed swing: 0.93..1.07, deterministic, really varies
        // (min below 0.96, max above 1.04) - seeds have different quality.
        // Club is swinging (deep house 54–58%), not straight.
        for h in [0u64, 1, 42, 777, 424242] {
            let v = swing_var(h);
            assert!((0.93..=1.07).contains(&v), "swing out of band: {v}");
            assert_eq!(v, swing_var(h), "not deterministic");
        }
        let vals: Vec<f32> = (0..64u64).map(swing_var).collect();
        assert!(
            vals.iter().cloned().fold(1.0f32, f32::min) < 0.96
                && vals.iter().cloned().fold(0.0f32, f32::max) > 1.04,
            "swing never varies"
        );
    }

    #[test]
    fn club_swings_for_deep_house() {
        // Deep house quality: club swing at 1.15..1.25 (54–58%), not straight.
        let swing = make_style("club").expect("club").swing_ratio();
        assert!((1.15..=1.25).contains(&swing), "club too straight: {swing}");
    }

    #[test]
    fn kick_vocabularies_differ_by_seed() {
        // Beat vocabulary: concatenated 8-bar kick patterns differ
        // different seeds (each seed has its own beat, not just one per style).
        use crate::instruments::Slot;
        for name in style_names() {
            let style = make_style(name).expect("style");
            let mut pats = std::collections::HashSet::new();
            for seed in 0..12u64 {
                let active: Vec<usize> = style.cast(seed).iter().map(|s| s.id()).collect();
                let mut rng = rand::SeedableRng::seed_from_u64(seed);
                let mut arranger = crate::arranger::Arranger::new(seed);
                let mut song = arranger.advance_measure();
                let mut pat = Vec::new();
                for _bar in 0..8 {
                    let mut sctx = StepContext {
                        step: 0,
                        step_in_measure: 0,
                        base_step_secs: style.base_step_secs(),
                        active_tracks: &active,
                        song: &song,
                        rng: &mut rng,
                        intensity: 1.0,
                    };
                    for sim in 0..16 {
                        sctx.step = sim;
                        sctx.step_in_measure = sim;
                        if render_style(&*style, &mut sctx)
                            .into_iter()
                            .any(|nn| nn.instrument_id == Slot::Kick.id())
                        {
                            pat.push(sim);
                        }
                    }
                    song = arranger.advance_measure();
                }
                pats.insert(pat);
            }
            assert!(
                pats.len() >= 6,
                "{name}: one beat per style ({})",
                pats.len()
            );
        }
    }

    #[test]
    fn kick_leaves_breath_before_downbeat() {
        // Flams at the bar line are forbidden: step 15 of the kick guides
        // is below 0.3 in every style (the 14-15-0 chain broke the beat with a flam).
        // The weights and all poles of the kick slots are checked.
        use crate::instruments::Slot;
        for name in style_names() {
            let style = make_style(name).expect("known style");
            let guides = style.slot_guides();
            for slot in [Slot::Kick, Slot::Kick808] {
                let gi = Slot::ALL.iter().position(|s| *s == slot).unwrap();
                let g = &guides[gi];
                let mut arrs: Vec<&[f32]> = vec![g.weights];
                if let Some(poles) = g.poles {
                    arrs.extend(poles.iter().copied());
                }
                for (ai, arr) in arrs.iter().enumerate() {
                    if arr.len() == style.steps_per_measure() {
                        assert!(
                            arr[15] < 0.3,
                            "{name} {slot:?} arr {ai}: pickup {v} before downbeat",
                            v = arr[15]
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn duration_shape_is_seed_character_not_multiplier() {
        // Articulation character of the seed 0.85..1.25: deterministic and discriminates
        // seeds. This is a length selection bias, not a multiplier: the length itself
        // comes from the guide's dictionary.
        for h in [0u64, 1, 42, 777] {
            let d = duration_shape(h, 3);
            assert!((0.85..=1.25).contains(&d), "out of range: {d}");
            assert_eq!(d, duration_shape(h, 3), "not deterministic");
        }
        let set: std::collections::HashSet<u32> = (0..32u64)
            .map(|h| (duration_shape(h, 3) * 1000.0) as u32)
            .collect();
        assert!(set.len() > 8, "seeds share articulation character");
    }

    #[test]
    fn note_lengths_are_chosen_from_the_guide_vocabulary() {
        // Length - generation decision: meter and seed character choose
        // dictionary element. Long notes are more often on strong beats, short notes -
        // on the weak; different seeds give different length patterns.
        let gates = &[1.0, 2.0, 3.0, 4.0];
        let guide = RoleGuide {
            weights: &[1.0; 16],
            ..RoleGuide::pitched(&[1.0; 16], 0, 0, 0, gates, 0.8)
        };
        let scale = [48, 50, 52, 53, 55, 57, 59];
        let mut seen = std::collections::HashSet::new();
        let (mut long_on_down, mut n_down) = (0.0f32, 0u32);
        let (mut short_on_weak, mut n_weak) = (0.0f32, 0u32);
        for seed in 0..64u64 {
            let mut song = test_song(0.5);
            song.seed_hash = seed;
            for (step, weak) in [(0usize, false), (1usize, true)] {
                let mut rng = rand::SeedableRng::seed_from_u64(seed);
                let notes = play_guide(
                    &guide, &scale, &song, 1.0, step, 16, 0.5, 3, 2, 1.0, false, false, true,
                    &mut rng,
                );
                let Some(note) = notes.first() else {
                    continue;
                };
                let gate = note.duration_secs / 0.5;
                assert!(
                    gates.iter().any(|&g| (g - gate).abs() <= 0.05 * g + 1e-4),
                    "gate {gate} not in vocabulary {gates:?}"
                );
                seen.insert(gate.round() as i32);
                if weak {
                    short_on_weak += gate;
                    n_weak += 1;
                } else {
                    long_on_down += gate;
                    n_down += 1;
                }
            }
        }
        assert!(seen.len() >= 3, "seeds share lengths: {seen:?}");
        assert!(n_down > 16 && n_weak > 16, "too few notes");
        assert!(
            long_on_down / n_down as f32 > short_on_weak / n_weak as f32,
            "downbeats must ring longer than weak sixteenths"
        );
    }

    #[test]
    fn every_style_has_a_musical_gate_vocabulary() {
        // Length Dictionary - Style Data: Positive, Sorted,
        // no empty layers. Extended layers must have a choice, otherwise
        // the length will again become a timbre constant.
        use crate::instruments::Slot;
        // A single gate is a deliberate choice for the rhythmic layer (rock)
        // riff, phonk bell), not a forgotten constant. But everyone
        // style must have real articulation: the bass chooses the length
        // always, and at least three lingering layers have a dictionary.
        const PITCHED: [Slot; 6] = [
            Slot::Bass,
            Slot::Pad,
            Slot::Pluck,
            Slot::Keys,
            Slot::Guitar,
            Slot::GuitarLead,
        ];
        for name in style_names() {
            let style = make_style(name).expect("known style");
            let mut flexible = 0;
            for (i, guide) in style.slot_guides().iter().enumerate() {
                let slot = Slot::ALL[i];
                assert!(!guide.gates.is_empty(), "{name} {slot:?}: empty gates");
                assert!(
                    guide.gates.iter().all(|&g| g > 0.0 && g <= 400.0),
                    "{name} {slot:?}: bad gates {:?}",
                    guide.gates
                );
                assert!(
                    guide.gates.windows(2).all(|w| w[0] < w[1]),
                    "{name} {slot:?}: gates not sorted {:?}",
                    guide.gates
                );
                if !guide.is_silent() && PITCHED.contains(&slot) && guide.gates.len() >= 2 {
                    flexible += 1;
                }
                if !guide.is_silent() && slot == Slot::Bass {
                    assert!(
                        guide.gates.len() >= 2,
                        "{name}: bass must choose its length"
                    );
                }
            }
            assert!(
                flexible >= 3,
                "{name}: only {flexible} pitched layers choose length"
            );
        }
    }

    #[test]
    fn tie_chance_is_stable_and_lively() {
        // The tie roll is deterministic (same seed - same picture), but not
        // "every second": steps give different rolls.
        let a = tie_chance(7, 0, 32);
        assert_eq!(a, tie_chance(7, 0, 32));
        assert!((0.0..=1.0).contains(&a));
        let set: std::collections::HashSet<u32> = (0..64)
            .map(|step| (tie_chance(7, 0, step) * 100.0) as u32)
            .collect();
        assert!(set.len() > 10, "tie pattern too static");
    }

    #[test]
    fn progression_moves_accompaniment_not_motif() {
        // The accompaniment follows a progression (different windows - different steps),
        // the motif is stable with the same progression. Harmonic rhythm -
        // from the seed (2/4/8): a pair of measures 1 vs 11 gives different windows for any
        // rhythm, the progression [0,2,1,3] gives different steps in these windows.
        // All steps are looked at: phrase anchor can skip downbeat
        // (downbeat breathes) - the contract is about the property, and not about the zero step.
        fn degree_at(measure: usize, follow_motif: bool, step: usize) -> Option<u8> {
            let guide = RoleGuide {
                weights: &[1.0; 8],
                follow_motif,
                ..RoleGuide::pitched(&[1.0; 8], 0, 1, 0, &[1.0], 0.8)
            };
            let scale = [48, 50, 52, 53, 55, 57, 59];
            let mut song = test_song(0.5);
            song.measure_index = measure;
            song.prog = [0, 2, 1, 3];
            let mut rng = rand::SeedableRng::seed_from_u64(3);
            play_guide(
                &guide, &scale, &song, 1.0, step, 8, 0.1, 7, 6, 1.0, false, false, true, &mut rng,
            )
            .into_iter()
            .next()
            .map(|n| n.midi_pitch)
        }
        // Measures 1 and 11: Progression windows are different for beats 2, 4 and 8.
        let mut shifted = false;
        // The motif does not depend on the measure.
        let mut stable = false;
        for step in 0..8 {
            if let (Some(a), Some(b)) = (degree_at(1, false, step), degree_at(11, false, step)) {
                assert_ne!(a, b, "step {step}: accompaniment ignores progression");
                shifted = true;
            }
            if let (Some(a), Some(b)) = (degree_at(1, true, step), degree_at(11, true, step)) {
                assert_eq!(a, b, "step {step}: motif follows progression");
                stable = true;
            }
        }
        assert!(shifted && stable, "too few common steps");
    }

    fn shared_hits(share: f32) -> usize {
        // Non-anchor lead (weight 0.5): count triggers on the grid
        // 16 steps × 8 phrases. Share 1.0 = sole holder of the role.
        let guide = RoleGuide {
            weights: &[0.5; 16],
            ..RoleGuide::kit(&[0.5; 16], 60, 0.8)
        };
        let mut total = 0;
        for phrase in 0..8usize {
            let mut song = test_song(0.5);
            song.phrase_index = phrase;
            let mut rng = rand::SeedableRng::seed_from_u64(11);
            total += (0..16)
                .filter(|step| {
                    !play_guide(
                        &guide,
                        &[48],
                        &song,
                        1.0,
                        *step,
                        16,
                        0.1,
                        6,
                        5,
                        share,
                        false,
                        false,
                        true,
                        &mut rng,
                    )
                    .is_empty()
                })
                .count();
        }
        total
    }

    #[test]
    fn role_sharing_thins_crowded_roles_but_spares_anchors() {
        // Two role holders (share 1/sqrt(2)) - each is less likely to be alone,
        // together cover the space without mush. Deterministic.
        let alone = shared_hits(1.0);
        let shared = shared_hits(1.0 / 2.0f32.sqrt());
        assert!(alone > 0, "lead never plays");
        assert!(
            shared < alone,
            "sharing must thin: shared={shared} alone={alone}"
        );
        assert_eq!(shared, shared_hits(1.0 / 2.0f32.sqrt()), "must repeat");
        // Anchors (weight 1.0) cannot be divided, but breathe with a priority of 0.95:
        // the pattern of beats does not depend on the beat, omissions are rare and the same
        // for any share (the gate does not know about the division).
        let anchor = RoleGuide {
            weights: &[1.0; 8],
            ..RoleGuide::kit(&[1.0; 8], 36, 0.9)
        };
        let pattern = |share: f32| -> Vec<bool> {
            let mut song = test_song(0.5);
            let mut out = Vec::new();
            for phrase in 0..16usize {
                song.phrase_index = phrase;
                let mut rng = rand::SeedableRng::seed_from_u64(4);
                for step in 0..8 {
                    out.push(
                        !play_guide(
                            &anchor,
                            &[48],
                            &song,
                            1.0,
                            step,
                            8,
                            0.1,
                            2,
                            0,
                            share,
                            false,
                            false,
                            true,
                            &mut rng,
                        )
                        .is_empty(),
                    );
                }
            }
            out
        };
        let full = pattern(1.0);
        let hits = full.iter().filter(|h| **h).count();
        // Priority 0.95: hits almost always (losses within ~1/10).
        assert!(
            hits * 10 >= full.len() * 9,
            "anchors lost face: {hits}/{}",
            full.len()
        );
        // Dividing into anchors does not work: the pattern is the same for any share.
        assert_eq!(full, pattern(1.0 / 2.0f32.sqrt()), "share moved anchors");
        assert_eq!(full, pattern(0.2), "share moved anchors");
    }
}
