//! Переходы стилей — короткий жест, а не правило формы.
//!
//! Переход срабатывает в последнем такте `BuildUp` и оставляет
//! затухающий хвост на первых шагах следующей секции. Жест владеет
//! своим слотом на этих шагах: обычная направляющая не дублирует удар.
//! Словарь жестов у каждого стиля свой (шейкерный вихрь, ролл, филл,
//! слайд 808, стаб-пикап, дыхание), поэтому стык звучит как стиль,
//! а не как общий метроном.
//!
//! Это не `piece::Transition`: тот отвечает за склейку кусков, этот —
//! за музыкальный вход из разгона в удар.

use crate::arranger::{SongContext, SongSection};
use crate::instruments::{NoteEvent, Slot};

/// Словарь перехода. Вид задает рисунок и инструмент, а не громкость:
/// громкость — дело секции и стадии.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Deep: шейкерный вихрь, плотность и сила растут к дропу.
    ShakerSpiral,
    /// Club: снейр-ролл 16-ми с хвостом после удара.
    SnareRoll,
    /// Rock: филл бочка/снейр, как томы, с мягким хвостом.
    TomFill,
    /// Phonk: слайд 808 через квинту в корень дропа.
    Slide808,
    /// Proto: короткий стаб-пикап щипка в последней четверти.
    StabPickup,
    /// Lofi: тихое дыхание хэта, без удара.
    BreathPickup,
}

/// Описание перехода стиля.
#[derive(Clone, Copy, Debug)]
pub struct Spec {
    pub kind: Kind,
    /// Слоты, которые жест занимает в последнем такте разгона.
    pub fill_slots: &'static [Slot],
    /// Слоты, которые жест занимает на первых шагах следующей секции.
    /// Хвост может быть уже филла: бочка дропа остается на месте.
    pub tail_slots: &'static [Slot],
    /// Длина хвоста в шагах (считая с даунбита новой секции).
    pub tail_steps: usize,
    /// Питч для ударных жестов; питчевые виды считают высоту сами.
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

/// Что переход делает на одном шаге.
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

    /// Занимает ли переход этот слот на данном шаге. Обычная
    /// направляющая слота в этом случае молчит: один удар, не два.
    pub fn owns(&self, slot: Slot) -> bool {
        self.owned[slot.index()]
    }
}

/// Отрисовать шаг перехода. `None` — стиль без перехода или шаг вне
/// зоны жеста. Слоты, выключенные пользователем или отсутствующие в
/// аранже секции, не занимаются: переход не обходит микшер.
pub fn render(
    spec: Option<Spec>,
    song: &SongContext,
    step_in_measure: usize,
    steps_per_measure: usize,
    base_step_secs: f32,
    scale: &[u8],
    active_tracks: &[usize],
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
            // Филл чередует низ (бочка) и верх (снейр): томы в этом
            // движке живут питчем снейра/бочки, а не отдельным слотом.
            // Хвост — только снейр: даунбит дропа остается за бочкой.
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
            let root = bass_root(scale, song);
            match phase {
                Phase::Fill => {
                    // Квинта в середине и корень к трем четвертям:
                    // моно-808 сам тянет глисс между нотами.
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
                let pitch = scale_pitch(scale, degree, song, 2);
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

/// Где и с какой силой звучит жест. Заполнение — последний такт
/// `BuildUp`; хвост — первые шаги раздела, вошедшего после разгона.
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

/// С какого шага последнего такта начинается жест. Поздние виды
/// (стаб, дыхание) оставляют больше места обычному груву.
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

/// Корень текущей гармонии в суб-регистре: 808 не уезжает в лид.
fn bass_root(scale: &[u8], song: &SongContext) -> u8 {
    if scale.is_empty() {
        return 36;
    }
    let prog = song.prog[(song.measure_index.saturating_sub(1)
        / super::prog_bar_len(song.seed_hash))
        % song.prog.len()] as usize;
    let mut pitch =
        scale[prog % scale.len()] as i32 + super::tonic_shift_for(song.seed_hash) as i32;
    while pitch > 41 {
        pitch -= 12;
    }
    while pitch < 24 {
        pitch += 12;
    }
    pitch.clamp(0, 127) as u8
}

/// Ступень лада в верхнем регистре для стаб-пикапа.
fn scale_pitch(scale: &[u8], degree: usize, song: &SongContext, octave_shift: i32) -> u8 {
    if scale.is_empty() {
        return 60;
    }
    let idx = degree % scale.len();
    let pitch =
        scale[idx] as i32 + super::tonic_shift_for(song.seed_hash) as i32 + 12 * octave_shift;
    pitch.clamp(0, 127) as u8
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arranger::{SongContext, SongSection};
    use crate::piece::layers_for;

    const SCALE: [u8; 7] = [48, 50, 51, 53, 55, 56, 58];

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
        // Ролл занимает последний такт разгона целиком и оставляет
        // короткий хвост после удара, а не обрывается на даунбите.
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
            render(spec, &build, 7, 16, 0.1, &SCALE, &active)
                .notes
                .is_empty(),
            "roll starts too early"
        );
        let fill: Vec<_> = (8..16)
            .map(|step| render(spec, &build, step, 16, 0.1, &SCALE, &active))
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
        let tail = render(spec, &drop, 0, 16, 0.1, &SCALE, &active);
        assert_eq!(tail.notes.len(), 1, "tail missing after the drop");
        assert!(tail.owns(Slot::Snare));
        assert!(
            render(spec, &drop, 3, 16, 0.1, &SCALE, &active)
                .notes
                .is_empty(),
            "tail outlived its length"
        );
    }

    #[test]
    fn rock_fill_takes_both_but_tail_leaves_kick() {
        // Филл владеет и бочкой, и снейром, но хвост — только снейр:
        // даунбит дропа остается за бочкой.
        let spec = Some(Spec::new(
            Kind::TomFill,
            &[Slot::Kick, Slot::Snare],
            &[Slot::Snare],
            3,
            36,
        ));
        let active = [Slot::Kick.id(), Slot::Snare.id()];
        let build = song(SongSection::BuildUp, true, 8, SongSection::MainMotif);
        let even = render(spec, &build, 8, 16, 0.1, &SCALE, &active);
        assert!(even.owns(Slot::Kick) && even.owns(Slot::Snare));
        assert_eq!(even.notes.len(), 1);
        assert_eq!(even.notes[0].instrument_id, Slot::Kick.id());
        let odd = render(spec, &build, 9, 16, 0.1, &SCALE, &active);
        assert_eq!(odd.notes[0].instrument_id, Slot::Snare.id());

        let drop = song(SongSection::Drop, false, 1, SongSection::BuildUp);
        let tail = render(spec, &drop, 0, 16, 0.1, &SCALE, &active);
        assert_eq!(tail.notes.len(), 1);
        assert_eq!(tail.notes[0].instrument_id, Slot::Snare.id());
        assert!(
            !tail.owns(Slot::Kick),
            "tail must leave the drop downbeat kick alone"
        );
    }

    #[test]
    fn transition_does_not_fire_inside_a_section() {
        // Жест живет только на шве: последний такт разгона и первые
        // шаги следующего раздела. Внутри секции обычный грув.
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
            let out = render(spec, &main, step, 16, 0.1, &SCALE, &active);
            assert!(out.notes.is_empty(), "transition leaked at step {step}");
            assert!(!out.owns(Slot::Shaker));
        }
        // Секция после BuildUp, но не первая — хвоста уже нет.
        let later = song(SongSection::Drop, false, 2, SongSection::BuildUp);
        assert!(
            render(spec, &later, 0, 16, 0.1, &SCALE, &active)
                .notes
                .is_empty(),
            "tail repeated in every measure"
        );
    }
}
