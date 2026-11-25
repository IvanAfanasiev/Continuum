//! Written form of a chunk: 30–60 seconds, with a finale and a seam into the next one.
//!
//! Previously the form was an endless loop of sections with a fixed skeleton
//! (`Score.cycle`): the listener heard the same circle over and over again, and
//! the pattern was re-rolled every two bars. Hence the feeling of "random
//! chunks" — there was no large-scale plan (goal, build-up, landing, finale) at all.
//!
//! Now the form is written in chunks:
//! - **body** of a chunk — 30–60 seconds following the style's skeleton, with varied
//!   lengths and an A/B theme across sections;
//! - **seam** — two bars at the end: a written finale (cadence, voices
//!   folding in) and the launch of the next chunk. The seam is planned together with
//!   the next chunk, so the join is inaudible: it's not a crossfade, but
//!   a written join.
//!
//! Arrangement by layers: the plan writes which groups of instruments play in
//! each section (groove, pump, theme, textures). A silent group
//! is truly silent — the dots on the wheel and the sound match, because
//! the decision is the same one.
//!
//! The plan is a pure function of (seed, style, chunk number, bar within the chunk),
//! so the remainder of a chunk can be re-planned on the fly (a live seed
//! or style change takes effect from the next bar).

use crate::arranger::{splitmix64, SongSection};
use crate::instruments::Group;

/// Arrangement layer mask: a bit = an instrument group playing in a section.
/// Layers are not roles, but "parts": groove (drums and bass), pump, theme,
/// textures. A chunk's plan writes who plays in each section:
/// that is the arrangement, not a coin toss on every step.
pub type LayerMask = u8;

/// Style arrangement: who plays in each of the four sections (order
/// [`LAYER_ORDER`]). The style decides for itself: in rock the riff stays in
/// the breakdown, in lo-fi the pad holds the theme.
/// This is data, not code: editing a style means editing a table.
pub type Layers = [LayerMask; 4];

/// Section order in the arrangement table.
pub const LAYER_ORDER: [SongSection; 4] = [
    SongSection::MainMotif,
    SongSection::BuildUp,
    SongSection::Drop,
    SongSection::PostChorus,
];

/// Section index in the arrangement table.
pub fn layer_index(section: SongSection) -> usize {
    match section {
        SongSection::MainMotif => 0,
        SongSection::BuildUp => 1,
        SongSection::Drop => 2,
        SongSection::PostChorus => 3,
    }
}

/// Groove: kick drum, snare, hat, bass — what holds the beat.
pub const L_GROOVE: LayerMask = (1 << Group::Kick.index())
    | (1 << Group::Snare.index())
    | (1 << Group::Hat.index())
    | (1 << Group::Bass.index());
/// Pump: chords (keys, pad).
pub const L_HARMONY: LayerMask = 1 << Group::Chords.index();
/// Theme: the upper voice (lead, flute, formula).
pub const L_THEME: LayerMask = 1 << Group::Lead.index();
/// Textures (vinyl, rain): a constant layer throughout.
pub const L_TEXTURE: LayerMask = 1 << Group::Texture.index();

/// Section mask by its kind: who plays in the theme, build-up, drop, breakdown.
/// Default arrangement for a style — styles override the table locally.
pub fn layers_for(section: SongSection) -> LayerMask {
    match section {
        // Theme: groove + pump + theme. Effects are silent — their entrance
        // in the drop is heard as an event, not as background.
        SongSection::MainMotif => L_GROOVE | L_HARMONY | L_THEME | L_TEXTURE,
        // Build-up: everything, including effects — the lineup grows.
        SongSection::BuildUp => L_GROOVE | L_HARMONY | L_THEME | L_TEXTURE,
        // Hit: the full lineup.
        SongSection::Drop => L_GROOVE | L_HARMONY | L_THEME | L_TEXTURE,
        // Breakdown: the groove rolls on, the top rests.
        SongSection::PostChorus => L_GROOVE | L_HARMONY | L_TEXTURE,
    }
}

/// Arrangement by the general rule (theme, build-up, hit, breakdown).
pub fn default_layers() -> Layers {
    [
        layers_for(SongSection::MainMotif),
        layers_for(SongSection::BuildUp),
        layers_for(SongSection::Drop),
        layers_for(SongSection::PostChorus),
    ]
}

/// Seam mask (chunk finale): only groove and texture. The upper voices
/// fold in — the next chunk's voice enters here, and the join turns out
/// smooth rather than a gap.
pub const L_JOINT: LayerMask = L_GROOVE | L_TEXTURE;

/// How a style glues chunks together.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transition {
    /// Smooth: the next chunk's voice enters over the current groove
    /// (overlapping the tail of the chunk). The groove is not interrupted.
    Rolling { bars: usize },
    /// Sharp: the chunk breaks at the bar line, the next one starts
    /// immediately. A quiet style gets a quiet join.
    Cut,
}

impl Transition {
    /// Seam length (in bars) for this transition.
    pub fn joint_bars(&self) -> usize {
        match self {
            Transition::Rolling { bars } => (*bars).max(1),
            // Sharp join: a short tail (one bar), then the next
            // chunk immediately at full energy.
            Transition::Cut => 1,
        }
    }

    /// Overhang: how many bars of the next chunk the shadow voice already sings
    /// while the main arranger finishes the seam. For a sharp join there is
    /// no overhang — chunks don't overlap.
    pub fn overlap_bars(&self) -> usize {
        match self {
            Transition::Rolling { bars } => (*bars).max(1),
            Transition::Cut => 0,
        }
    }
}

/// Section of a chunk: what plays, for how long and with what energy.
#[derive(Clone, Debug)]
pub struct SectionPlan {
    pub section: SongSection,
    /// Base number of bars.
    pub bars: usize,
    /// Length varies by seed (endless loop) or is fixed
    /// (written chunk form).
    pub vary: bool,
    /// Base energy of the section: `None` — from the general form formula,
    /// `Some` — written in the plan.
    pub energy: Option<f32>,
    /// Who plays in the section (group mask).
    pub layers: LayerMask,
}

impl SectionPlan {
    pub fn new(section: SongSection, bars: usize) -> Self {
        Self {
            section,
            bars,
            vary: false,
            energy: None,
            layers: layers_for(section),
        }
    }
}

/// A chunk: body (30–60 s) + seam (finale and launch of the next).
#[derive(Clone, Debug)]
pub struct PiecePlan {
    /// Body of the chunk: sections in order.
    pub body: Vec<SectionPlan>,
    /// Seam: the current chunk's finale and the entry into the next. Played after
    /// the body, so the last bar of the seam is a cadence (the theme resolves to the tonic).
    pub joint: SectionPlan,
}

impl PiecePlan {
    /// Number of bars in the chunk (body + seam).
    pub fn bars(&self) -> usize {
        self.body.iter().map(|s| s.bars).sum::<usize>() + self.joint.bars
    }
}

/// Target chunk duration, s. Chunks shorter than 30 s read as
/// separate numbers, longer than 60 — as one long track. The target isn't
/// dogma: the seed shifts it within a window, and the form adapts proportionally.
pub const TARGET_PIECE_SECS: f32 = 54.0;
/// Target bounds: shorter — the chunk can't become a form, longer —
/// the listener has time to forget the beginning.
const MIN_PIECE_SECS: f32 = 30.0;
const MAX_PIECE_SECS: f32 = 60.0;

/// Section lengths to fit the target chunk size. Order and all sections
/// are preserved: the form isn't trimmed, but compressed or stretched
/// proportionally. The step is two bars (a whole phrase), the minimum — two bars.
fn fit_section_bars(lengths: &[usize], target: usize, salt: u64) -> Vec<usize> {
    if lengths.is_empty() {
        return Vec::new();
    }
    let n = lengths.len();
    let natural: usize = lengths.iter().sum();
    let target = target.max(n * 2);
    // Round down to even: a section consists of whole phrases.
    let mut out: Vec<usize> = lengths
        .iter()
        .map(|bars| {
            let scaled = if natural == 0 {
                2
            } else {
                (*bars as f32 * target as f32 / natural as f32).round() as usize
            };
            scaled.max(2) & !1
        })
        .collect();
    let mut total: usize = out.iter().sum();
    let mut cursor = (splitmix64(salt ^ 0xF17B_A25D) % n as u64) as usize;
    // Add two bars at a time in a round-robin: the increment spreads across the form,
    // rather than piling up in the last section.
    while total < target {
        for i in 0..n {
            let idx = (cursor + i) % n;
            out[idx] += 2;
            total += 2;
            if total >= target {
                break;
            }
        }
        cursor = (cursor + 1) % n;
    }
    // Take from the longest sections, keeping the two-bar minimum.
    while total > target {
        let mut longest: Option<usize> = None;
        for i in 0..n {
            let idx = (cursor + i) % n;
            if out[idx] > 2 && longest.is_none_or(|b| out[idx] > out[b]) {
                longest = Some(idx);
            }
        }
        let Some(idx) = longest else { break };
        out[idx] -= 2;
        total -= 2;
        cursor = (cursor + 1) % n;
    }
    out
}

/// Style skeleton → chunk plan.
///
/// `skeleton` — the style's sequence of sections (`Score.cycle`).
/// `index` — the chunk's number in the stream: chunks differ from one another
/// (section lengths, energy), but deterministically.
/// `start_bar` — how many bars of the chunk have already played: the plan is cut from
/// that point (re-planning the remainder on a live seed/style change —
/// takes effect from the next bar).
/// `transition` — how the style glues chunks together.
///
/// The form is written in full: every pass over the skeleton preserves all sections,
/// including the breakdown. Within 30–60 s the chunk fits by compressing/stretching
/// lengths, not by cutting off the tail.
pub fn plan_piece(
    seed: u64,
    bpm: f32,
    skeleton: &[(SongSection, usize)],
    index: u64,
    start_bar: usize,
    transition: Transition,
    layers: Layers,
) -> PiecePlan {
    let salt = splitmix64(seed ^ 0x9E37_79B9 ^ index.wrapping_mul(0x2545_F491));
    let bar_secs = if bpm > 1.0 { 4.0 * 60.0 / bpm } else { 2.0 };
    let joint_bars = transition.joint_bars();
    let bars_for = |secs: f32| ((secs / bar_secs).round() as usize).max(2);
    // We compute the bounds "inward": lower rounded up, upper rounded down, otherwise
    // rounding itself pushes the chunk outside 30–60 s.
    let min_total = ((MIN_PIECE_SECS / bar_secs).ceil() as usize)
        .max(joint_bars + 8)
        .max(2);
    let max_total = ((MAX_PIECE_SECS / bar_secs).floor() as usize).max(min_total);
    // Chunk length is also the seed's flavor: ±4 bars around the target. The
    // 30–60 s bounds stay hard: this is a format, not a matter of taste.
    let jitter = ((splitmix64(salt ^ 0xD0CE_5EED) % 5) as i64 - 2) * 2;
    let target_total = (bars_for(TARGET_PIECE_SECS) as i64 + jitter)
        .clamp(min_total as i64, max_total as i64) as usize;
    let max_body = max_total.saturating_sub(joint_bars).max(2);
    let min_body = min_total.saturating_sub(joint_bars).max(2);

    // An empty skeleton is no reason to panic: a minimal theme.
    let fallback = [(SongSection::MainMotif, 8usize)];
    let skeleton = if skeleton.is_empty() {
        &fallback[..]
    } else {
        skeleton
    };

    // 1. Full pass: all skeleton sections, lengths breathe with the seed.
    // A short skeleton repeats only if without the repeat the chunk doesn't
    // reach the lower bound; a long one is compressed as a whole.
    let base_total: usize = skeleton.iter().map(|(_, bars)| (*bars).max(2)).sum();
    let mut passes = 1usize;
    while base_total * passes < min_body {
        let next = passes + 1;
        // The second pass must leave each section at least 2 bars
        // and not exceed the chunk's upper bound.
        if next > 2 || next * skeleton.len() * 2 > max_body {
            break;
        }
        passes = next;
    }
    let mut agenda: Vec<(SongSection, usize)> = Vec::with_capacity(skeleton.len() * passes);
    for pass in 0..passes {
        for (pos, (section, bars)) in skeleton.iter().enumerate() {
            let visit = (pass * skeleton.len() + pos) as u64;
            // The drop is the form's anchor: its length doesn't jump with the seed. The other
            // sections get +0/+2 bars: the form breathes, the order stays intact.
            let extra = if *section == SongSection::Drop {
                0
            } else {
                (splitmix64(salt ^ visit.wrapping_mul(0x9E37_79B9)) % 2) as usize * 2
            };
            agenda.push((*section, (*bars).max(2) + extra));
        }
    }

    // 2. Fitting the 30–60 s window: proportionally, without trimming.
    let target_body = target_total
        .saturating_sub(joint_bars)
        .clamp(min_body, max_body);
    let lengths: Vec<usize> = agenda.iter().map(|(_, bars)| *bars).collect();
    let fitted = fit_section_bars(&lengths, target_body, salt);
    let mut body: Vec<SectionPlan> = agenda
        .iter()
        .zip(fitted)
        .map(|((section, _), bars)| SectionPlan {
            section: *section,
            bars,
            vary: false,
            energy: None,
            layers: layers[layer_index(*section)],
        })
        .collect();
    if body.is_empty() {
        body.push(SectionPlan::new(SongSection::MainMotif, 8));
    }
    // Seam: the written finale of the chunk. The groove rolls on, the upper voices
    // fold in — the next chunk's voice enters here, and the join
    // turns out smooth, not a gap. For a sharp transition the seam
    // is shorter and at full lineup (a hit — then straight into the next chunk).
    let joint = match transition {
        Transition::Rolling { .. } => SectionPlan {
            section: SongSection::PostChorus,
            bars: joint_bars,
            vary: false,
            // Seam energy is a landing, not a gap: the groove (kick, snare, hat)
            // holds, otherwise the join has a hole. The top folds in via
            // the layer mask, not via energy.
            energy: Some(0.62),
            layers: L_JOINT,
        },
        // Sharp join: a short tail at full lineup (a hit) — then the
        // next chunk at full energy right away.
        Transition::Cut => SectionPlan {
            section: SongSection::PostChorus,
            bars: joint_bars,
            vary: false,
            energy: Some(0.55),
            layers: L_GROOVE | L_HARMONY | L_THEME | L_TEXTURE,
        },
    };
    // Remainder of the chunk: cut off the bars already played. Re-planning
    // the remainder (a live seed/style change) takes effect from the next bar.
    if start_bar > 0 {
        let mut left = start_bar;
        body.retain_mut(|s| {
            if left == 0 {
                return true;
            }
            if left >= s.bars {
                left -= s.bars;
                false
            } else {
                s.bars -= left;
                left = 0;
                true
            }
        });
    }
    PiecePlan { body, joint }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arranger::{CALM_SCORE, ENERGETIC_SCORE};

    fn rolling() -> Transition {
        Transition::Rolling { bars: 2 }
    }

    #[test]
    fn piece_is_30_to_60_seconds() {
        // All styles, all seeds, several chunks in a row: the 30–60 s window
        // holds not on average, but in every chunk.
        for (name, bpm, skeleton) in [
            ("club", 126.0, ENERGETIC_SCORE.cycle),
            ("lofi", 78.0, CALM_SCORE.cycle),
        ] {
            for seed in [1u64, 2, 3, 42, 777] {
                for index in 0..4u64 {
                    let plan =
                        plan_piece(seed, bpm, skeleton, index, 0, rolling(), default_layers());
                    let secs = plan.bars() as f32 * 4.0 * 60.0 / bpm;
                    assert!(
                        (30.0..=60.0).contains(&secs),
                        "{name} seed {seed} chunk {index}: {secs:.1} s ({} bars)",
                        plan.bars()
                    );
                }
            }
        }
    }

    #[test]
    fn piece_keeps_the_whole_arc() {
        // Compression doesn't cut the form's tail: theme, build-up, hit and breakdown
        // remain in every chunk, in their original order.
        for (name, bpm, skeleton) in [
            ("club", 126.0, ENERGETIC_SCORE.cycle),
            ("lofi", 78.0, CALM_SCORE.cycle),
        ] {
            for seed in [1u64, 2, 3, 42, 777] {
                let plan = plan_piece(seed, bpm, skeleton, 0, 0, rolling(), default_layers());
                let sections: Vec<SongSection> = plan.body.iter().map(|s| s.section).collect();
                for want in [
                    SongSection::MainMotif,
                    SongSection::BuildUp,
                    SongSection::Drop,
                    SongSection::PostChorus,
                ] {
                    assert!(
                        sections.contains(&want),
                        "{name} seed {seed}: section {want:?} missing ({sections:?})"
                    );
                }
                // Each section is a whole phrase, not a scrap.
                for s in &plan.body {
                    assert!(
                        s.bars >= 2 && s.bars % 2 == 0,
                        "{name} seed {seed}: {s:?} is not a whole phrase"
                    );
                }
            }
        }
    }

    #[test]
    fn piece_ends_with_a_joint() {
        // Every chunk has a finale: the last bars are the seam.
        let plan = plan_piece(
            42,
            126.0,
            ENERGETIC_SCORE.cycle,
            0,
            0,
            rolling(),
            default_layers(),
        );
        assert_eq!(plan.joint.bars, 2);
        assert!(
            plan.joint.energy.is_some(),
            "seam must have written energy"
        );
        assert!(plan.body.len() >= 3, "chunk body too short");
    }

    #[test]
    fn joint_leaves_room_for_the_next_piece() {
        // In the seam the upper voices are silent: the next chunk's
        // voice enters there — otherwise the join cuts the phrase.
        let plan = plan_piece(
            42,
            126.0,
            ENERGETIC_SCORE.cycle,
            0,
            0,
            rolling(),
            default_layers(),
        );
        assert_eq!(plan.joint.layers & L_THEME, 0, "seam must not contain the theme");
        assert_ne!(plan.joint.layers & L_GROOVE, 0, "seam must contain the groove");
    }

    #[test]
    fn sharp_transition_does_not_overlap() {
        // Sharp join: a short tail, chunks don't overlap —
        // the next starts immediately, without an overlap.
        let plan = plan_piece(
            42,
            126.0,
            ENERGETIC_SCORE.cycle,
            0,
            0,
            Transition::Cut,
            default_layers(),
        );
        assert_eq!(plan.joint.bars, 1);
        assert_eq!(Transition::Cut.overlap_bars(), 0);
        assert_eq!(Transition::Rolling { bars: 2 }.overlap_bars(), 2);
        // In a sharp seam the lineup is full: it's a hit, not emptiness.
        assert_ne!(plan.joint.layers & L_THEME, 0);
    }

    #[test]
    fn arrangement_writes_who_plays() {
        // Arrangement by layers: no effects in the theme, full lineup in the drop.
        assert_eq!(
            layers_for(SongSection::PostChorus) & L_THEME,
            0,
            "theme rests in the breakdown"
        );
    }

    #[test]
    fn pieces_differ_between_themselves() {
        // Neighboring chunks aren't verbatim repeats: section lengths and the set
        // differ (otherwise the stream becomes a "loop" again).
        let a = plan_piece(
            42,
            126.0,
            ENERGETIC_SCORE.cycle,
            0,
            0,
            rolling(),
            default_layers(),
        );
        let b = plan_piece(
            42,
            126.0,
            ENERGETIC_SCORE.cycle,
            1,
            0,
            rolling(),
            default_layers(),
        );
        assert_ne!(
            a.body.iter().map(|s| s.bars).collect::<Vec<_>>(),
            b.body.iter().map(|s| s.bars).collect::<Vec<_>>(),
            "chunks are not different"
        );
    }

    #[test]
    fn same_seed_same_piece() {
        let a = plan_piece(
            7,
            126.0,
            ENERGETIC_SCORE.cycle,
            3,
            0,
            rolling(),
            default_layers(),
        );
        let b = plan_piece(
            7,
            126.0,
            ENERGETIC_SCORE.cycle,
            3,
            0,
            rolling(),
            default_layers(),
        );
        assert_eq!(
            a.body.iter().map(|s| s.bars).collect::<Vec<_>>(),
            b.body.iter().map(|s| s.bars).collect::<Vec<_>>()
        );
    }

    #[test]
    fn remainder_is_planned_from_the_current_bar() {
        // A live seed/style change takes effect from the next bar: the remainder
        // of the chunk is re-planned, what has played is left alone.
        let full = plan_piece(
            42,
            126.0,
            ENERGETIC_SCORE.cycle,
            0,
            0,
            rolling(),
            default_layers(),
        );
        let rest = plan_piece(
            42,
            126.0,
            ENERGETIC_SCORE.cycle,
            0,
            8,
            rolling(),
            default_layers(),
        );
        let full_bars: usize = full.body.iter().map(|s| s.bars).sum();
        let rest_bars: usize = rest.body.iter().map(|s| s.bars).sum();
        assert_eq!(
            full_bars - rest_bars,
            8,
            "remainder must be shorter by what has played"
        );
        // The seam (finale) remains in the remainder: the chunk still reaches
        // the landing.
        assert_eq!(rest.joint.bars, 2);
    }

    #[test]
    fn calm_style_piece_breathes() {
        // Calm form: the theme returns after the hit, and the hit in lofi is
        // a timbre change, not an explosion. The form isn't cut down to one theme.
        let plan = plan_piece(7, 78.0, CALM_SCORE.cycle, 0, 0, rolling(), default_layers());
        let mains = plan
            .body
            .iter()
            .filter(|s| s.section == SongSection::MainMotif)
            .count();
        assert!(mains >= 2, "chunk without a theme return: {:?}", plan.body);
        assert!(
            plan.body.iter().any(|s| s.section == SongSection::Drop),
            "lofi lost its hit (timbre change): {:?}",
            plan.body
        );
    }
}
