//! Style transitions are a short gesture, not a rule of form.
//!
//! The transition fires in the last measure of `BuildUp` and leaves
//! a fading tail over the first steps of the next section. The gesture owns
//! its slot on those steps: the regular guide does not duplicate the hit.
//! Each style has its own vocabulary of gestures (shaker spiral, roll, fill,
//! 808 slide, stab pickup, breath), so the seam sounds like its style,
//! and not as a general metronome.
//!
//! This is not `piece::Transition`: that one is responsible for gluing the pieces together, this one is
//! for the musical entry from build-up into the drop.

use super::Harmony;
use crate::arranger::{SongContext, SongSection};
use crate::instruments::{NoteEvent, Slot};

/// Transition vocabulary. A kind defines its pattern and instrument, not its volume:
/// volume is a matter of section and stage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Deep: shaker spiral; density and force grow towards the drop.
    ShakerSpiral,
    /// Club: 16th-note snare roll with a tail after the hit.
    SnareRoll,
    /// Rock: kick/snare fill like toms, with a soft tail.
    TomFill,
    /// Phonk: 808 slide through the fifth into the drop root.
    Slide808,
    /// Proto: short stab-pickup pluck in the last quarter.
    StabPickup,
    /// Lofi: quiet hi-hat breath, no hit.
    BreathPickup,
}

/// Description of the style transition.
#[derive(Clone, Copy, Debug)]
pub struct Spec {
    pub kind: Kind,
    /// Slots that the gesture occupies in the last build-up measure.
    pub fill_slots: &'static [Slot],
    /// The slots that the gesture occupies in the first steps of the next section.
    /// The tail may be narrower: the drop kick remains in place.
    pub tail_slots: &'static [Slot],
    /// Tail length in steps (counting from the downbeat of the new section).
    pub tail_steps: usize,
    /// Pitch for percussion gestures; pitched kinds determine their own pitch.
    pub pitch: u8,
}

impl Spec {
    pub const fn new(
        kind: Kind,
        fill_slots: &'static [Slot],
        tail_slots: &'static [Slot],
        tail_steps: usize,
        pitch: u8,
    ) -> Self {
        Self {
            kind,
            fill_slots,
            tail_slots,
            tail_steps,
            pitch,
        }
    }
}

/// What the transition does in one step.
pub struct Step {
    owned: [bool; Slot::COUNT],
    pub notes: Vec<NoteEvent>,
}

impl Step {
    fn none() -> Self {
        Self {
            owned: [false; Slot::COUNT],
            notes: Vec::new(),
        }
    }

    /// Whether the transition owns this slot at this step. The slot's regular
    /// guide is silent in this case: one hit, not two.
    pub fn owns(&self, slot: Slot) -> bool {
        self.owned[slot.index()]
    }
}

/// Render a transition step. `None` — style without a transition, or a step outside
/// the gesture zone. Slots disabled by the user or missing from the
/// section arrangement are not owned: the transition does not bypass the mixer.
pub(crate) fn render(
    spec: Option<Spec>,
    song: &SongContext,
    step_in_measure: usize,
    steps_per_measure: usize,
    base_step_secs: f32,
    active_tracks: &[usize],
    harmony: &Harmony,
) -> Step {
    let Some(spec) = spec else {
        return Step::none();
    };
    let Some((phase, progress)) = phase(spec, song, step_in_measure, steps_per_measure) else {
        return Step::none();
    };
    let slots = match phase {
        Phase::Fill => spec.fill_slots,
        Phase::Tail => spec.tail_slots,
    };
    let mut out = Step::none();
    for slot in slots {
        if available(*slot, song, active_tracks) {
            out.owned[slot.index()] = true;
        }
    }
    if !out.owned.iter().any(|&o| o) {
        return out;
    }

    match spec.kind {
        Kind::ShakerSpiral => {
            let v = match phase {
                Phase::Fill => 0.20 + 0.62 * progress,
                Phase::Tail => 0.42 * progress,
            };
            push(
                &mut out,
                Slot::Shaker,
                spec.pitch,
                v,
                0.4,
                base_step_secs,
                song,
            );
        }
        Kind::SnareRoll => {
            let v = match phase {
                Phase::Fill => 0.35 + 0.60 * progress,
                Phase::Tail => 0.32 * progress,
            };
            push(
                &mut out,
                Slot::Snare,
                spec.pitch,
                v,
                0.5,
                base_step_secs,
                song,
            );
        }
        Kind::TomFill => {
            // The fill alternates between the bottom (kick) and the top (snare): the toms in this
            // in the engine they live by the pitch of the snare/kick, and not by a separate slot.
            // The tail is only a snare: the downbeat drop remains behind the kick drum.
            if phase == Phase::Fill {
                let (slot, pitch) = if step_in_measure.is_multiple_of(2) {
                    (Slot::Kick, spec.pitch)
                } else {
                    (Slot::Snare, 38)
                };
                push(
                    &mut out,
                    slot,
                    pitch,
                    0.45 + 0.50 * progress,
                    0.5,
                    base_step_secs,
                    song,
                );
            } else {
                push(
                    &mut out,
                    Slot::Snare,
                    38,
                    0.28 * progress,
                    0.4,
                    base_step_secs,
                    song,
                );
            }
        }
        Kind::Slide808 => {
            let root = bass_root(harmony.scale, song, harmony);
            match phase {
                Phase::Fill => {
                    // Fifth in the middle and root to three quarters:
                    // Mono-808 itself pulls gliss between notes.
                    if step_in_measure == steps_per_measure / 2 {
                        push(
                            &mut out,
                            Slot::Kick808,
                            root.saturating_add(7),
                            0.72,
                            6.0,
                            base_step_secs,
                            song,
                        );
                    } else if step_in_measure == steps_per_measure * 3 / 4 {
                        push(
                            &mut out,
                            Slot::Kick808,
                            root,
                            0.85,
                            8.0,
                            base_step_secs,
                            song,
                        );
                    }
                }
                Phase::Tail => {
                    if step_in_measure == 0 {
                        push(
                            &mut out,
                            Slot::Kick808,
                            root,
                            0.62 * progress,
                            8.0,
                            base_step_secs,
                            song,
                        );
                    }
                }
            }
        }
        Kind::StabPickup => {
            let start = pick_start(steps_per_measure);
            let (play, degree, v) = match phase {
                Phase::Fill => {
                    let v = 0.35 + 0.25 * progress;
                    match step_in_measure.saturating_sub(start) {
                        0 => (true, 0, v),
                        2 => (true, 2, v),
                        3 => (true, 4, v),
                        _ => (false, 0, 0.0),
                    }
                }
                Phase::Tail => {
                    let v = 0.35 * progress;
                    match step_in_measure {
                        0 => (true, 0, v),
                        2 => (true, 2, v),
                        _ => (false, 0, 0.0),
                    }
                }
            };
            if play {
                let pitch = scale_pitch(harmony.scale, degree, song, harmony, 2);
                push(&mut out, Slot::Pluck, pitch, v, 1.0, base_step_secs, song);
            }
        }
        Kind::BreathPickup => {
            let v = match phase {
                Phase::Fill => 0.12 + 0.18 * progress,
                Phase::Tail => 0.12 * progress,
            };
            push(
                &mut out,
                Slot::Hat,
                spec.pitch,
                v,
                0.35,
                base_step_secs,
                song,
            );
        }
    }
    out
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Fill,
    Tail,
}

/// Where and with what force the gesture sounds. Fill - last measure
/// `BuildUp`; tail - the first steps of the section that entered after build-up.
fn phase(
    spec: Spec,
    song: &SongContext,
    step_in_measure: usize,
    steps_per_measure: usize,
) -> Option<(Phase, f32)> {
    let start = fill_start(spec.kind, steps_per_measure);
    if song.section == SongSection::BuildUp && song.is_last_measure && step_in_measure >= start {
        let span = steps_per_measure.saturating_sub(start).max(1);
        let p = (step_in_measure - start) as f32 / span.saturating_sub(1).max(1) as f32;
        return Some((Phase::Fill, p.min(1.0)));
    }
    if song.section_measure_index == 1
        && song.previous_section == SongSection::BuildUp
        && step_in_measure < spec.tail_steps
    {
        let p = 1.0 - (step_in_measure + 1) as f32 / (spec.tail_steps.max(1) + 1) as f32;
        return Some((Phase::Tail, p.clamp(0.0, 1.0)));
    }
    None
}

/// From what step of the last measure does the gesture begin? Late species
/// (stab, breath) leave more room for the regular groove.
fn fill_start(kind: Kind, steps_per_measure: usize) -> usize {
    match kind {
        Kind::ShakerSpiral | Kind::SnareRoll | Kind::TomFill | Kind::Slide808 => {
            steps_per_measure / 2
        }
        Kind::StabPickup | Kind::BreathPickup => pick_start(steps_per_measure),
    }
}

fn pick_start(steps_per_measure: usize) -> usize {
    steps_per_measure * 3 / 4
}

fn available(slot: Slot, song: &SongContext, active_tracks: &[usize]) -> bool {
    active_tracks.contains(&slot.id()) && song.layers & (1 << slot.group().index()) != 0
}

fn push(
    out: &mut Step,
    slot: Slot,
    pitch: u8,
    velocity: f32,
    duration_steps: f32,
    base_step_secs: f32,
    song: &SongContext,
) {
    if !out.owned[slot.index()] || velocity < 0.02 {
        return;
    }
    let v = (velocity * song.stage_gain(slot.role())).clamp(0.02, 1.0);
    out.notes.push(NoteEvent::new(
        slot.id(),
        pitch,
        v,
        base_step_secs * duration_steps,
    ));
}

/// The root of the current harmony is in the sub-register: 808 does not go into the lead.
/// Progression, harmony tempo and tonality are style, not general.
fn bass_root(scale: &[u8], song: &SongContext, harmony: &Harmony) -> u8 {
    if scale.is_empty() {
        return 36;
    }
    let prog = harmony.prog
        [(song.measure_index.saturating_sub(1) / harmony.bar_len) % harmony.prog.len()]
        as usize;
    let mut pitch = scale[prog % scale.len()] as i32
        + super::tonic_shift_for(song.seed_hash) as i32
        + harmony.key_shift as i32;
    while pitch > 41 {
        pitch -= 12;
    }
    while pitch < 24 {
        pitch += 12;
    }
    pitch.clamp(0, 127) as u8
}

/// Upper register scale level for stab pickup.
fn scale_pitch(
    scale: &[u8],
    degree: usize,
    song: &SongContext,
    harmony: &Harmony,
    octave_shift: i32,
) -> u8 {
    if scale.is_empty() {
        return 60;
    }
    let idx = degree % scale.len();
    let pitch = scale[idx] as i32
        + super::tonic_shift_for(song.seed_hash) as i32
        + harmony.key_shift as i32
        + 12 * octave_shift;
    pitch.clamp(0, 127) as u8
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arranger::{SongContext, SongSection};
    use crate::piece::layers_for;

    const SCALE: [u8; 7] = [48, 50, 51, 53, 55, 56, 58];

    fn h(song: &SongContext) -> Harmony {
        Harmony {
            scale: &SCALE,
            ..Harmony::neutral(song)
        }
    }

    fn song(
        section: SongSection,
        last: bool,
        section_measure: usize,
        previous: SongSection,
    ) -> SongContext {
        SongContext {
            section,
            measure_index: 9,
            phrase_index: 4,
            energy: 1.0,
            progress: 0.5,
            is_last_measure: last,
            section_measure_index: section_measure,
            previous_section: previous,
            stage: 1,
            cycle: 0,
            stage_gains: [1.0; 8],
            layers: layers_for(section),
            seed_hash: 11,
            density: 1.0,
            motif: [0; 16],
            motif_b: [0; 16],
            use_alt_motif: false,
            prog: [0, 1, 2, 3],
            tonic_walk: 0,
        }
    }

    #[test]
    fn club_roll_fills_last_bar_and_tails_after() {
        // The roll takes up the entire last build-up measure and leaves
        // short tail after hit, rather than breaking off on the downbeat.
        let spec = Some(Spec::new(
            Kind::SnareRoll,
            &[Slot::Snare],
            &[Slot::Snare],
            3,
            60,
        ));
        let active = [Slot::Snare.id()];
        let build = song(SongSection::BuildUp, true, 8, SongSection::MainMotif);
        assert!(
            render(spec, &build, 7, 16, 0.1, &active, &h(&build))
                .notes
                .is_empty(),
            "roll starts too early"
        );
        let fill: Vec<_> = (8..16)
            .map(|step| render(spec, &build, step, 16, 0.1, &active, &h(&build)))
            .collect();
        for (i, step) in fill.iter().enumerate() {
            assert_eq!(step.notes.len(), 1, "roll missed step {}", i + 8);
            assert!(step.owns(Slot::Snare));
        }
        assert!(
            fill[7].notes[0].velocity > fill[0].notes[0].velocity,
            "roll must rise into the drop"
        );

        let drop = song(SongSection::Drop, false, 1, SongSection::BuildUp);
        let tail = render(spec, &drop, 0, 16, 0.1, &active, &h(&build));
        assert_eq!(tail.notes.len(), 1, "tail missing after the drop");
        assert!(tail.owns(Slot::Snare));
        assert!(
            render(spec, &drop, 3, 16, 0.1, &active, &h(&build))
                .notes
                .is_empty(),
            "tail outlived its length"
        );
    }

    #[test]
    fn rock_fill_takes_both_but_tail_leaves_kick() {
        // Phill has both a kick drum and a snare, but the tail is only a snare:
        // the downbeat drop remains behind the kick drum.
        let spec = Some(Spec::new(
            Kind::TomFill,
            &[Slot::Kick, Slot::Snare],
            &[Slot::Snare],
            3,
            36,
        ));
        let active = [Slot::Kick.id(), Slot::Snare.id()];
        let build = song(SongSection::BuildUp, true, 8, SongSection::MainMotif);
        let even = render(spec, &build, 8, 16, 0.1, &active, &h(&build));
        assert!(even.owns(Slot::Kick) && even.owns(Slot::Snare));
        assert_eq!(even.notes.len(), 1);
        assert_eq!(even.notes[0].instrument_id, Slot::Kick.id());
        let odd = render(spec, &build, 9, 16, 0.1, &active, &h(&build));
        assert_eq!(odd.notes[0].instrument_id, Slot::Snare.id());

        let drop = song(SongSection::Drop, false, 1, SongSection::BuildUp);
        let tail = render(spec, &drop, 0, 16, 0.1, &active, &h(&build));
        assert_eq!(tail.notes.len(), 1);
        assert_eq!(tail.notes[0].instrument_id, Slot::Snare.id());
        assert!(
            !tail.owns(Slot::Kick),
            "tail must leave the drop downbeat kick alone"
        );
    }

    #[test]
    fn transition_does_not_fire_inside_a_section() {
        // The gesture lives only at the seam: the last stroke of build-up and the first
        // steps in the next section. Inside the section there is a regular groove.
        let spec = Some(Spec::new(
            Kind::ShakerSpiral,
            &[Slot::Shaker],
            &[Slot::Shaker],
            4,
            70,
        ));
        let active = [Slot::Shaker.id()];
        let main = song(SongSection::MainMotif, true, 8, SongSection::MainMotif);
        for step in 0..16 {
            let out = render(spec, &main, step, 16, 0.1, &active, &h(&main));
            assert!(out.notes.is_empty(), "transition leaked at step {step}");
            assert!(!out.owns(Slot::Shaker));
        }
        // The section after BuildUp, but not the first - the tail is no longer there.
        let later = song(SongSection::Drop, false, 2, SongSection::BuildUp);
        assert!(
            render(spec, &later, 0, 16, 0.1, &active, &h(&later))
                .notes
                .is_empty(),
            "tail repeated in every measure"
        );
    }
}
