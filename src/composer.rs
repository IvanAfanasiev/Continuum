use crate::instruments::Instrument;
use crate::markov::{get_preset, Chord, LayerConfig, MarkovGenerator, MarkovPreset};
use crate::NoteEvent;
use crossbeam_queue::ArrayQueue;
use rand::prelude::*;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

#[derive(Clone)]
struct PhrasePlan {
    offsets: Vec<u8>,
    accents: Vec<f32>,
    activity: Vec<f32>,
    ambient: bool,
}

impl PhrasePlan {
    fn new(preset: &MarkovPreset, chord: &Chord, rng: &mut impl Rng) -> Self {
        let len = preset.phrase_len.max(1);
        let ambient = preset.base_step_ms > 700.0;
        let motif = if ambient {
            [
                color_offset(chord, 2),
                color_offset(chord, 7),
                color_offset(chord, 11),
                color_offset(chord, 4),
            ]
        } else {
            [
                0,
                color_offset(chord, if rng.random_bool(0.55) { 2 } else { 3 }),
                7,
                color_offset(chord, if rng.random_bool(0.45) { 5 } else { 9 }),
            ]
        };

        let mut offsets = Vec::with_capacity(len);
        let mut accents = Vec::with_capacity(len);
        let mut activity = Vec::with_capacity(len);

        for step in 0..len {
            let mut offset = motif[step % motif.len()] % 12;

            if ambient && step.is_multiple_of(2) {
                offset = color_offset(chord, [2, 7, 9, 11][(step / 2) % 4]);
            } else if !ambient && step >= len.saturating_sub(2) {
                offset = if step == len - 1 {
                    0
                } else {
                    color_offset(chord, 2)
                };
            }

            if offsets.last().copied() == Some(offset) {
                offset = alternate_offset(chord, offset, step);
            }

            offsets.push(offset);

            let accent = if step == 0 {
                1.0
            } else if step.is_multiple_of(4) {
                0.86
            } else if step.is_multiple_of(2) {
                0.70
            } else {
                0.52
            };
            accents.push(accent);

            let activity_value = if ambient {
                match step {
                    2 | 7 | 10 => 0.90,
                    0 | 6 => 0.55,
                    _ => 0.18,
                }
            } else {
                match step % 12 {
                    0 | 3 | 5 | 7 | 10 => 0.95,
                    2 | 4 | 8 | 11 => 0.68,
                    _ => 0.34,
                }
            };
            activity.push(activity_value);
        }

        Self {
            offsets,
            accents,
            activity,
            ambient,
        }
    }

    fn offset(&self, step: usize) -> u8 {
        self.offsets[step % self.offsets.len()]
    }

    fn accent(&self, step: usize) -> f32 {
        self.accents[step % self.accents.len()]
    }

    fn activity(&self, step: usize) -> f32 {
        self.activity[step % self.activity.len()]
    }

    fn is_echo_step(&self, step: usize) -> bool {
        self.ambient && self.activity(step) > 0.75
    }

    fn is_pad_shift(&self, step: usize) -> bool {
        self.ambient && step == self.offsets.len() / 2
    }
}

pub fn start_composing(queue: Arc<ArrayQueue<NoteEvent>>, preset_name: &str) {
    thread::sleep(Duration::from_millis(200));

    let preset = get_preset(preset_name);
    println!("[composer] generating preset: {}", preset.name);

    let mut generators: Vec<MarkovGenerator> = preset
        .layers
        .iter()
        .enumerate()
        .map(|(index, layer)| MarkovGenerator::new(layer, preset, 1337 + index as u64))
        .collect();

    let mut global_step = 0usize;
    let mut global_chord_idx = 0usize;
    let mut rng = rand::rng();
    let mut phrase_plan = PhrasePlan::new(preset, preset.chords[global_chord_idx], &mut rng);
    let mut last_notes = vec![None; preset.layers.len()];

    loop {
        let is_phrase_start = global_step == 0;
        let current_chord = preset.chords[global_chord_idx];

        if is_phrase_start {
            phrase_plan = PhrasePlan::new(preset, current_chord, &mut rng);
        }

        let step_ms = preset.base_step_ms;
        let sleep_ms = step_sleep_ms(step_ms, global_step, &phrase_plan, &mut rng);
        let mut bass_note_this_step = None;

        for (index, layer) in preset.layers.iter().enumerate() {
            if !should_trigger(
                layer.instrument,
                is_phrase_start,
                global_step,
                step_ms,
                bass_note_this_step.is_some(),
                &phrase_plan,
                &mut rng,
            ) {
                continue;
            }

            let generator = &mut generators[index];
            generator.chord_idx = global_chord_idx;
            generator.phrase_pos = global_step;

            let mut event = generator.next(layer);
            align_to_phrase(
                &mut event,
                layer,
                current_chord,
                &phrase_plan,
                global_step,
                last_notes[index],
                &mut rng,
            );
            generator.revise_last_note(event.note);

            humanize_velocity(
                &mut event,
                layer,
                preset.vel_min,
                preset.vel_max,
                &phrase_plan,
                global_step,
                &mut rng,
            );
            humanize_duration(
                &mut event,
                layer.instrument,
                step_ms,
                preset.phrase_len,
                &phrase_plan,
                global_step,
                &mut rng,
            );

            if layer.instrument == Instrument::Piano {
                if let Some(bass_note) = bass_note_this_step {
                    complement_bass_with_piano(&mut event, bass_note, layer.note_max, &mut rng);
                }
            }

            push_event(&queue, event);
            last_notes[index] = Some(event.note);

            if layer.instrument == Instrument::Bass {
                bass_note_this_step = Some(event.note);
                maybe_push_bass_companion(&queue, event, layer.note_min, layer.note_max, &mut rng);
            }
        }

        global_step += 1;
        if global_step >= preset.phrase_len {
            global_step = 0;
            global_chord_idx = (global_chord_idx + 1) % preset.chords.len();
        }

        thread::sleep(Duration::from_millis(sleep_ms.round() as u64));
    }
}

fn should_trigger(
    instrument: Instrument,
    is_phrase_start: bool,
    global_step: usize,
    base_step_ms: f32,
    bass_played_this_step: bool,
    phrase_plan: &PhrasePlan,
    rng: &mut impl Rng,
) -> bool {
    match instrument {
        Instrument::Pad => is_phrase_start || phrase_plan.is_pad_shift(global_step),
        Instrument::Bass => {
            let activity = phrase_plan.activity(global_step);
            activity > 0.90 || rng.random_range(0.0..1.0) < activity * 0.72
        }
        Instrument::Piano => {
            if base_step_ms > 700.0 {
                phrase_plan.is_echo_step(global_step) && rng.random_range(0..100) < 42
            } else {
                let activity = phrase_plan.activity(global_step);
                let call_response = if bass_played_this_step { 0.34 } else { 0.56 };
                rng.random_range(0.0..1.0) < activity * call_response
            }
        }
        Instrument::Pluck | Instrument::Sine => is_phrase_start || rng.random_range(0..100) < 48,
        _ => true,
    }
}

fn align_to_phrase(
    event: &mut NoteEvent,
    layer: &LayerConfig,
    chord: &Chord,
    phrase_plan: &PhrasePlan,
    step: usize,
    last_note: Option<u8>,
    rng: &mut impl Rng,
) {
    let mut offset = phrase_plan.offset(step);

    if layer.instrument == Instrument::Bass && (event.is_phrase_start || step.is_multiple_of(4)) {
        offset = 0;
    } else if layer.instrument == Instrument::Pad && offset == 0 {
        offset = color_offset(chord, 9);
    }

    let target_pc = (chord.root + offset) % 12;
    let preferred = preferred_center(layer, event.instrument, phrase_plan, step);
    let mut note = nearest_note_for_pc(layer.note_min, layer.note_max, target_pc, preferred)
        .unwrap_or(event.note);

    if let Some(last) = last_note {
        if last == note || (phrase_plan.ambient && last % 12 == note % 12) {
            note = alternate_note(layer, chord, note, preferred, step, rng).unwrap_or(note);
        }
    }

    event.note = note;
}

fn humanize_velocity(
    event: &mut NoteEvent,
    layer: &LayerConfig,
    preset_vel_min: f32,
    preset_vel_max: f32,
    phrase_plan: &PhrasePlan,
    step: usize,
    rng: &mut impl Rng,
) {
    let layer_min = (preset_vel_min * layer.vel_scale * 0.70).clamp(0.0, 1.0);
    let layer_max = (preset_vel_max * layer.vel_scale).clamp(layer_min, 1.0);
    let accent = phrase_plan.accent(step);
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
                0.46
            } else {
                0.78
            }
        }
        Instrument::Bass => 0.96,
        _ => 0.85,
    };
    let nuance = rng.random_range(0.985..1.015);

    event.velocity =
        (event.velocity * (0.72 + accent * 0.28) * role_gain * nuance).clamp(layer_min, layer_max);
}

fn humanize_duration(
    event: &mut NoteEvent,
    instrument: Instrument,
    step_ms: f32,
    phrase_len: usize,
    phrase_plan: &PhrasePlan,
    step: usize,
    rng: &mut impl Rng,
) {
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

            if event.is_phrase_end {
                event.duration = event.duration.max(step_ms * rng.random_range(1.8..2.6));
            }
        }
        Instrument::Piano => {
            if phrase_plan.ambient {
                event.duration = step_ms * rng.random_range(0.75..1.35);
            } else {
                let modifier = if event.is_phrase_end {
                    rng.random_range(1.15..1.55)
                } else {
                    rng.random_range(0.78..1.12)
                };
                event.duration = (event.duration * modifier).clamp(step_ms * 0.42, step_ms * 2.2);
            }
        }
        Instrument::Pluck | Instrument::Sine => {
            event.duration = (event.duration * rng.random_range(0.86..1.10))
                .clamp(step_ms * 0.35, step_ms * 1.8);
        }
        _ => {
            event.duration = event.duration.min(step_ms);
        }
    }
}

fn step_sleep_ms(
    base_step_ms: f32,
    global_step: usize,
    phrase_plan: &PhrasePlan,
    rng: &mut impl Rng,
) -> f32 {
    let swing = if phrase_plan.ambient {
        1.0
    } else if global_step.is_multiple_of(2) {
        0.92
    } else {
        1.08
    };
    let jitter = if phrase_plan.ambient {
        rng.random_range(-14.0..18.0)
    } else {
        rng.random_range(-5.0..7.0)
    };

    (base_step_ms * swing + jitter).max(20.0)
}

fn complement_bass_with_piano(
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

fn maybe_push_bass_companion(
    queue: &ArrayQueue<NoteEvent>,
    event: NoteEvent,
    note_min: u8,
    note_max: u8,
    rng: &mut impl Rng,
) {
    let chance = if event.is_phrase_start || event.duration > 900.0 {
        12
    } else {
        4
    };

    if rng.random_range(0..100) >= chance {
        return;
    }

    let candidates = [
        event.note.saturating_add(12),
        event.note.saturating_add(7),
        event.note.saturating_sub(5),
    ];
    let Some(note) = candidates
        .into_iter()
        .find(|&note| note >= note_min && note <= note_max && note != event.note)
    else {
        return;
    };

    let mut companion = event;
    companion.note = note;
    companion.velocity *= rng.random_range(0.20..0.32);
    companion.duration *= rng.random_range(0.45..0.72);
    push_event(queue, companion);
}

fn push_event(queue: &ArrayQueue<NoteEvent>, event: NoteEvent) {
    if queue.push(event).is_err() {
        eprintln!("[composer] note queue is full; dropping one note");
    }
}

fn color_offset(chord: &Chord, preferred: u8) -> u8 {
    chord
        .notes
        .iter()
        .copied()
        .min_by_key(|&offset| pitch_class_distance(offset % 12, preferred % 12))
        .unwrap_or(preferred)
        % 12
}

fn alternate_offset(chord: &Chord, current: u8, step: usize) -> u8 {
    chord
        .notes
        .iter()
        .copied()
        .filter(|&offset| offset % 12 != current % 12)
        .nth(step % chord.notes.len().max(1))
        .unwrap_or((current + 7) % 12)
        % 12
}

fn preferred_center(
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
        Instrument::Piano => 0.58 + 0.16 * contour,
        _ => 0.50,
    };

    (low + span * position).round() as u8
}

fn nearest_note_for_pc(min: u8, max: u8, pc: u8, preferred: u8) -> Option<u8> {
    (min..=max)
        .filter(|note| note % 12 == pc % 12)
        .min_by_key(|&note| (note as i16 - preferred as i16).abs())
}

fn alternate_note(
    layer: &LayerConfig,
    chord: &Chord,
    current: u8,
    preferred: u8,
    step: usize,
    rng: &mut impl Rng,
) -> Option<u8> {
    let mut offsets: Vec<u8> = chord
        .notes
        .iter()
        .copied()
        .filter(|offset| (chord.root + offset) % 12 != current % 12)
        .collect();

    offsets.shuffle(rng);
    offsets
        .into_iter()
        .cycle()
        .take(chord.notes.len().max(1))
        .skip(step % chord.notes.len().max(1))
        .find_map(|offset| {
            nearest_note_for_pc(
                layer.note_min,
                layer.note_max,
                (chord.root + offset) % 12,
                preferred,
            )
        })
}

fn pitch_class_distance(a: u8, b: u8) -> u8 {
    let diff = (a as i16 - b as i16).rem_euclid(12) as u8;
    diff.min(12 - diff)
}
