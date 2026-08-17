use crate::controls::RuntimeControls;
use crate::instruments::Instrument;
use crate::markov::{Chord, LayerConfig};
use crate::NoteEvent;
use crossbeam_queue::ArrayQueue;
use rand::prelude::*;

use crate::composer::{push_event, GenerationContext, VelocityRange};
use crate::phrase::PhrasePlan;
use crate::section::{HarmonicFunction, HarmonicMoment, PhraseRole, SectionState};
use crate::theory::{
    alternate_note, color_offset, nearby_chord_note, nearest_note_for_pc,
    pitch_class_distance, smooth_note_for_pc,
};

pub(crate) struct PianoVoicing {
    pub notes: Vec<u8>,
    pub arpeggiate: bool,
    pub delay_ms: f32,
}

pub(crate) fn should_trigger(
    instrument: Instrument,
    is_phrase_start: bool,
    global_step: usize,
    _base_step_ms: f32,
    _bass_played_this_step: bool,
    context: &GenerationContext<'_>,
    rng: &mut impl Rng,
) -> bool {
    let phrase_plan = context.phrase_plan;
    let section = context.section;

    match instrument {
        Instrument::Pad => is_phrase_start || phrase_plan.is_pad_shift(global_step),
        Instrument::Bass => {
            if phrase_plan.ambient {
                phrase_plan.is_bass_step(global_step)
            } else {
                phrase_plan.is_bass_step(global_step)
            }
        }
        Instrument::Piano => {
            if phrase_plan.ambient {
                phrase_plan.is_piano_step(global_step) && rng.random_range(0.0..1.0) < 0.76
            } else {
                phrase_plan.is_piano_step(global_step)
            }
        }
        Instrument::Triangle => {
            if !phrase_plan.ambient {
                return false;
            }

            phrase_plan.is_kick_step(global_step)
                || (phrase_plan.is_echo_step(global_step)
                    && rng.random_range(0.0..1.0)
                        < section.layer_presence(instrument)
                            * (0.18 + phrase_plan.accent(global_step) * 0.24))
        }
        Instrument::Kick => !phrase_plan.ambient && phrase_plan.is_kick_step(global_step),
        Instrument::Ride => {
            if phrase_plan.ambient {
                return false;
            }

            phrase_plan.is_ride_step(global_step)
        }
        Instrument::Hihat => {
            if phrase_plan.ambient {
                return false;
            }

            phrase_plan.is_hihat_step(global_step)
        }
    }
}

pub(crate) fn align_to_phrase(
    event: &mut NoteEvent,
    layer: &LayerConfig,
    chord: &Chord,
    phrase_plan: &PhrasePlan,
    step: usize,
    last_note: Option<u8>,
    rng: &mut impl Rng,
) {
    let mut offset = phrase_plan.offset(step);

    match layer.instrument {
        Instrument::Kick => {
            event.note = 36;
            return;
        }
        Instrument::Ride => {
            event.note = 89;
            return;
        }
        Instrument::Hihat => {
            event.note = 92;
            return;
        }
        Instrument::Bass if !phrase_plan.ambient => {
            offset = phrase_plan.bass_offset(step);
        }
        Instrument::Piano if !phrase_plan.ambient => {
            offset = phrase_plan.piano_offset(step);
        }
        Instrument::Pad if offset == 0 => {
            offset = color_offset(chord, 9);
        }
        Instrument::Triangle => {
            offset = color_offset(chord, if step.is_multiple_of(3) { 11 } else { 9 });
        }
        _ => {}
    }

    let target_pc = (chord.root + offset) % 12;
    let preferred = preferred_center(layer, event.instrument, phrase_plan, step);
    let mut note = nearest_note_for_pc(layer.note_min, layer.note_max, target_pc, preferred)
        .unwrap_or(event.note);

    if let Some(last) = last_note {
        if !phrase_plan.ambient && matches!(layer.instrument, Instrument::Piano | Instrument::Bass)
        {
            if let Some(smoothed) = smooth_note_for_pc(
                layer.note_min,
                layer.note_max,
                target_pc,
                last,
                preferred,
                if layer.instrument == Instrument::Bass {
                    5
                } else {
                    7
                },
            ) {
                note = smoothed;
            }
        }

        if last == note && !phrase_plan.ambient && layer.instrument == Instrument::Piano {
            note = nearby_chord_note(layer, chord, note, last, preferred, rng).unwrap_or(note);
        } else if (last == note || (phrase_plan.ambient && last % 12 == note % 12))
            && layer.instrument != Instrument::Bass
        {
            note = alternate_note(layer, chord, note, preferred, step, rng).unwrap_or(note);
        }
    }

    event.note = note;
}

pub(crate) fn humanize_velocity(
    event: &mut NoteEvent,
    layer: &LayerConfig,
    velocity_range: VelocityRange,
    context: &GenerationContext<'_>,
    step: usize,
    rng: &mut impl Rng,
) {
    let phrase_plan = context.phrase_plan;
    let section = context.section;
    let layer_min = (velocity_range.min * layer.vel_scale * 0.70).clamp(0.0, 1.0);
    let layer_max = (velocity_range.max * layer.vel_scale).clamp(layer_min, 1.0);
    let accent = phrase_plan.accent(step);
    let base_velocity = ((velocity_range.min + velocity_range.max) * 0.5 * layer.vel_scale)
        .clamp(layer_min, layer_max);
    let role_gain = match layer.instrument {
        Instrument::Pad => {
            if phrase_plan.ambient {
                0.62
            } else {
                0.78
            }
        }
        Instrument::Piano => {
            if phrase_plan.ambient {
                0.40
            } else {
                0.92
            }
        }
        Instrument::Bass => {
            if phrase_plan.ambient {
                0.88
            } else {
                0.72
            }
        }
        Instrument::Triangle => 0.50,
        Instrument::Kick => 0.28,
        Instrument::Ride => 0.38,
        Instrument::Hihat => 0.22,
    };
    let section_gain = match layer.instrument {
        Instrument::Triangle => 0.20 + section.layer_presence(layer.instrument) * 0.80,
        Instrument::Kick | Instrument::Ride | Instrument::Hihat => {
            0.92 + section.layer_presence(layer.instrument) * 0.08
        }
        _ => 1.0,
    };
    let accent_gain = if phrase_plan.ambient {
        0.84 + accent * 0.16
    } else {
        match layer.instrument {
            Instrument::Bass if phrase_plan.is_kick_step(step) => 1.02,
            Instrument::Bass => 0.86,
            Instrument::Kick | Instrument::Ride | Instrument::Hihat => 0.92 + accent * 0.08,
            _ => 0.88 + accent * 0.12,
        }
    };
    let harmonic_gain = if phrase_plan.ambient {
        1.0
    } else {
        let harmony = section.current_harmony();
        match layer.instrument {
            Instrument::Piano => match harmony.function {
                HarmonicFunction::Release => 1.02,
                HarmonicFunction::Tension | HarmonicFunction::Pivot => {
                    0.96 + harmony.tension * 0.08
                }
                _ => 0.98,
            },
            Instrument::Bass if phrase_plan.is_kick_step(step) => 1.02,
            Instrument::Bass => match harmony.function {
                HarmonicFunction::Home | HarmonicFunction::Release => 1.00,
                HarmonicFunction::Tension | HarmonicFunction::Pivot => 0.94,
                _ => 0.97,
            },
            Instrument::Kick
                if matches!(
                    harmony.function,
                    HarmonicFunction::Tension | HarmonicFunction::Release | HarmonicFunction::Pivot
                ) =>
            {
                1.04
            }
            Instrument::Hihat | Instrument::Ride => 0.94 + harmony.tension * 0.08,
            _ => 1.0,
        }
    };
    let nuance = rng.random_range(0.992..1.008);

    event.velocity =
        (base_velocity * accent_gain * role_gain * section_gain * harmonic_gain * nuance)
            .clamp(layer_min, layer_max);
}

pub(crate) fn humanize_duration(
    event: &mut NoteEvent,
    instrument: Instrument,
    step_ms: f32,
    phrase_len: usize,
    context: &GenerationContext<'_>,
    step: usize,
    rng: &mut impl Rng,
) {
    let phrase_plan = context.phrase_plan;
    let section = context.section;
    let harmony = section.current_harmony();
    let phrase_role = section.current_variation().role;

    match instrument {
        Instrument::Pad => {
            let phrase_tail = if phrase_plan.ambient {
                step_ms * (phrase_len as f32 * 0.55)
            } else {
                step_ms * (phrase_len as f32 - 0.4).max(1.0)
            };
            event.duration = phrase_tail;
        }
        Instrument::Bass => {
            if phrase_plan.ambient {
                let accent = phrase_plan.accent(step);
                event.duration = if accent > 0.9 {
                    step_ms * rng.random_range(1.9..3.2)
                } else if accent > 0.75 {
                    step_ms * rng.random_range(1.0..1.75)
                } else if phrase_plan.activity(step) < 0.45 {
                    step_ms * rng.random_range(0.34..0.62)
                } else {
                    step_ms * rng.random_range(0.62..1.10)
                };
            } else {
                let steps_to_next = phrase_plan.next_bass_distance(step).unwrap_or(4) as f32;
                let steps_to_hat = phrase_plan
                    .next_hihat_distance(step)
                    .filter(|&distance| distance < steps_to_next as usize)
                    .map(|distance| distance as f32);
                let harmonic_hold = match harmony.function {
                    HarmonicFunction::Home | HarmonicFunction::Release => 1.06,
                    HarmonicFunction::Tension | HarmonicFunction::Pivot => 0.92,
                    HarmonicFunction::Color => 0.98,
                };
                let raw_hold = if let Some(steps_to_hat) = steps_to_hat {
                    steps_to_hat * rng.random_range(0.82..0.96)
                } else if phrase_plan.is_kick_step(step) {
                    steps_to_next * rng.random_range(0.88..0.98)
                } else {
                    steps_to_next * rng.random_range(0.62..0.84)
                } * harmonic_hold;
                let max_hold = (steps_to_next * 0.98).clamp(0.55, 5.8);
                let held = raw_hold.clamp(0.55, max_hold);
                event.duration = step_ms * held;
            }

            if event.is_phrase_end {
                event.duration = event.duration.max(step_ms * rng.random_range(1.8..2.6));
            }
        }
        Instrument::Piano => {
            if phrase_plan.ambient {
                event.duration = step_ms * rng.random_range(0.75..1.35);
            } else {
                let modifier = if phrase_plan.is_piano_chord_step(step) {
                    match harmony.function {
                        HarmonicFunction::Release => rng.random_range(1.28..1.72),
                        HarmonicFunction::Tension | HarmonicFunction::Pivot => {
                            rng.random_range(0.92..1.22)
                        }
                        _ => rng.random_range(1.02..1.34),
                    }
                } else if event.is_phrase_end || phrase_role == PhraseRole::Cadence {
                    rng.random_range(1.08..1.50)
                } else if phrase_role == PhraseRole::Development {
                    rng.random_range(0.54..0.84)
                } else if phrase_role == PhraseRole::Echo {
                    rng.random_range(0.64..0.94)
                } else if phrase_plan.accent(step) > 0.84 {
                    rng.random_range(0.82..1.12)
                } else if harmony.function == HarmonicFunction::Pivot {
                    rng.random_range(0.58..0.88)
                } else {
                    rng.random_range(0.66..1.02)
                };
                event.duration = (step_ms * modifier).clamp(step_ms * 0.36, step_ms * 2.1);
            }
        }
        Instrument::Triangle => {
            event.duration = step_ms * rng.random_range(0.65..1.8);
        }
        Instrument::Kick => {
            event.duration = step_ms * 0.18;
        }
        Instrument::Ride => {
            event.duration = step_ms * rng.random_range(0.36..0.72);
        }
        Instrument::Hihat => {
            event.duration = step_ms * rng.random_range(0.06..0.12);
        }
    }
}

pub(crate) fn complement_bass_with_piano(
    event: &mut NoteEvent,
    bass_note: u8,
    layer_note_max: u8,
    rng: &mut impl Rng,
) {
    if event.note < bass_note.saturating_add(14) {
        let raised = event.note.saturating_add(12);
        if raised <= layer_note_max {
            event.note = raised;
        }
    }

    event.velocity *= rng.random_range(0.62..0.82);
    event.duration *= rng.random_range(0.78..1.12);
}

pub(crate) fn support_piano_with_bass(
    event: &mut NoteEvent,
    chord: &Chord,
    layer: &LayerConfig,
    phrase_plan: &PhrasePlan,
    step: usize,
    piano_note: Option<u8>,
) {
    let offset = phrase_plan.bass_offset(step);
    let target_pc = (chord.root + offset) % 12;
    let center = preferred_center(layer, Instrument::Bass, phrase_plan, step);
    let preferred = piano_note
        .map(|note| {
            note.saturating_sub(if phrase_plan.is_kick_step(step) {
                24
            } else {
                19
            })
        })
        .unwrap_or(center)
        .clamp(layer.note_min, layer.note_max);

    if let Some(note) = nearest_note_for_pc(layer.note_min, layer.note_max, target_pc, preferred) {
        event.note = note;
    }
}

pub(crate) fn maybe_push_piano_chord(
    queue: &ArrayQueue<NoteEvent>,
    event: NoteEvent,
    chord: &Chord,
    layer: &LayerConfig,
    context: &GenerationContext<'_>,
    step: usize,
    rng: &mut impl Rng,
) {
    let phrase_plan = context.phrase_plan;
    let section = context.section;

    if !phrase_plan.is_piano_chord_step(step) {
        return;
    }

    let harmony = section.current_harmony();
    let mut voicing = piano_voicing(chord, layer, phrase_plan, section, step, event.note, rng);
    voicing
        .notes
        .retain(|&note| note != event.note && note.abs_diff(event.note) > 1);

    for (index, note) in voicing.notes.into_iter().enumerate() {
        let mut chord_event = event;
        chord_event.note = note;
        chord_event.velocity *= piano_voicing_gain(harmony, index, rng);
        chord_event.duration = (event.duration * rng.random_range(1.08..1.42)).clamp(220.0, 940.0);
        if voicing.arpeggiate {
            chord_event.start_delay_ms += voicing.delay_ms * (index + 1) as f32;
            chord_event.duration *= 0.86;
        }
        chord_event.is_phrase_start = false;
        push_event(queue, chord_event);
    }
}

pub(crate) fn preferred_center(
    layer: &LayerConfig,
    instrument: Instrument,
    phrase_plan: &PhrasePlan,
    step: usize,
) -> u8 {
    let span = layer.note_max.saturating_sub(layer.note_min).max(1) as f32;
    let low = layer.note_min as f32;
    let contour = step as f32 / phrase_plan.offsets.len().max(1) as f32;
    let position = match instrument {
        Instrument::Bass => 0.30 + 0.18 * contour,
        Instrument::Pad => 0.64,
        Instrument::Piano if phrase_plan.ambient => 0.78,
        Instrument::Piano => 0.50 + 0.18 * contour,
        Instrument::Triangle => 0.78,
        Instrument::Kick | Instrument::Ride | Instrument::Hihat => 0.50,
    };

    (low + span * position).round() as u8
}

pub(crate) fn phrase_step_lengths(
    base_step_ms: f32,
    phrase_len: usize,
    phrase_plan: &PhrasePlan,
    controls: &RuntimeControls,
    rng: &mut impl Rng,
) -> Vec<f32> {
    (0..phrase_len)
        .map(|step| step_sleep_ms(base_step_ms, step, phrase_plan, controls, rng))
        .collect()
}

pub(crate) fn step_sleep_ms(
    base_step_ms: f32,
    global_step: usize,
    phrase_plan: &PhrasePlan,
    controls: &RuntimeControls,
    rng: &mut impl Rng,
) -> f32 {
    let swing_amount = controls.swing().clamp(0.0, 1.0);
    let swing = if phrase_plan.ambient {
        if global_step.is_multiple_of(2) {
            1.0 + 0.08 * swing_amount
        } else {
            1.0 - 0.08 * swing_amount
        }
    } else if global_step.is_multiple_of(2) {
        1.0 + 0.28 * swing_amount
    } else {
        1.0 - 0.28 * swing_amount
    };
    let tempo = controls.tempo().clamp(0.25, 3.0);
    let jitter = if phrase_plan.ambient {
        rng.random_range(-7.0..9.0)
    } else {
        0.0
    };

    (base_step_ms * swing / tempo + jitter).max(20.0)
}

pub(crate) fn piano_voicing(
    chord: &Chord,
    layer: &LayerConfig,
    phrase_plan: &PhrasePlan,
    section: &SectionState,
    step: usize,
    lead_note: u8,
    rng: &mut impl Rng,
) -> PianoVoicing {
    let harmony = section.current_harmony();
    let lead_pc = (lead_note as i16 - chord.root as i16).rem_euclid(12) as u8;
    let count = harmony.chord_note_count.clamp(1, harmony.chord_tones.len());
    let mut offsets: Vec<u8> = harmony
        .chord_tones
        .iter()
        .copied()
        .filter(|&offset| offset % 12 != lead_pc)
        .take(count + 1)
        .collect();
    offsets.sort_by_key(|&offset| {
        let resolved = matches!(
            harmony.function,
            HarmonicFunction::Home | HarmonicFunction::Release
        );
        let stable = matches!(offset % 12, 3 | 4 | 7 | 10) || (resolved && offset % 12 == 0);
        (
            !stable,
            pitch_class_distance(offset % 12, phrase_plan.piano_offset(step)),
        )
    });
    offsets.truncate(count);

    let center = preferred_center(layer, Instrument::Piano, phrase_plan, step);
    let ceiling = lead_note.saturating_sub(2).min(layer.note_max);
    let anchors = [
        lead_note.saturating_sub(4),
        lead_note.saturating_sub(7),
        lead_note.saturating_sub(10),
    ];
    let mut notes = Vec::with_capacity(offsets.len());

    for (index, &offset) in offsets.iter().enumerate() {
        let preferred = anchors
            .get(index)
            .copied()
            .unwrap_or(center.saturating_sub(5));
        let pc = (chord.root + color_offset(chord, offset)) % 12;
        let note = nearest_note_for_pc(layer.note_min, ceiling, pc, preferred)
            .or_else(|| nearest_note_for_pc(layer.note_min, layer.note_max, pc, preferred));

        if let Some(note) = note {
            notes.push(note);
        }
    }

    if notes.len() < count {
        fill_piano_voicing(chord, layer, lead_pc, center, &mut notes);
    }

    notes.sort_unstable();
    notes.dedup();
    notes.truncate(count);

    let arpeggiate =
        rng.random_bool((harmony.arpeggio_bias + section.presence * 0.12).clamp(0.0, 0.72) as f64);
    let delay_ms = match harmony.function {
        HarmonicFunction::Tension | HarmonicFunction::Pivot => 56.0,
        HarmonicFunction::Release => 72.0,
        _ => 44.0,
    };

    PianoVoicing {
        notes,
        arpeggiate,
        delay_ms,
    }
}

pub(crate) fn fill_piano_voicing(
    chord: &Chord,
    layer: &LayerConfig,
    lead_pc: u8,
    center: u8,
    notes: &mut Vec<u8>,
) {
    let mut fallback_offsets: Vec<u8> = chord
        .notes
        .iter()
        .copied()
        .filter(|&offset| offset % 12 != lead_pc)
        .collect();
    fallback_offsets.sort_by_key(|&offset| {
        let stable = matches!(offset % 12, 3 | 4 | 7 | 10);
        (!stable, pitch_class_distance(offset % 12, lead_pc))
    });

    for offset in fallback_offsets {
        let pc = (chord.root + offset) % 12;
        let Some(note) =
            nearest_note_for_pc(layer.note_min, layer.note_max, pc, center.saturating_sub(6))
        else {
            continue;
        };

        if notes.iter().all(|&taken| taken.abs_diff(note) > 1) {
            notes.push(note);
        }
    }
}

pub(crate) fn piano_voicing_gain(harmony: HarmonicMoment, note_index: usize, rng: &mut impl Rng) -> f32 {
    let base = match harmony.function {
        HarmonicFunction::Home => 0.19,
        HarmonicFunction::Color => 0.21,
        HarmonicFunction::Tension => 0.24,
        HarmonicFunction::Release => 0.23,
        HarmonicFunction::Pivot => 0.20,
    };
    let depth = 1.0 - note_index as f32 * 0.08;

    (base * depth * rng.random_range(0.92..1.06)).clamp(0.14, 0.30)
}