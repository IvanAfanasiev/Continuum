use crate::instruments::Instrument;
use crate::markov::{get_preset, MarkovGenerator};
use crate::NoteEvent;
use crossbeam_queue::ArrayQueue;
use rand::prelude::*;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

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

    loop {
        let is_phrase_start = global_step == 0;
        let step_ms = preset.base_step_ms;
        let sleep_ms = step_sleep_ms(step_ms, global_step, &mut rng);
        let mut bass_note_this_step = None;

        for (index, layer) in preset.layers.iter().enumerate() {
            let generator = &mut generators[index];
            generator.chord_idx = global_chord_idx;
            generator.phrase_pos = global_step;

            if !should_trigger(
                layer.instrument,
                is_phrase_start,
                global_step,
                step_ms,
                bass_note_this_step.is_some(),
                &mut rng,
            ) {
                continue;
            }

            let mut event = generator.next(layer);
            humanize_velocity(
                &mut event,
                preset.vel_min,
                preset.vel_max,
                layer.vel_scale,
                &mut rng,
            );
            humanize_duration(
                &mut event,
                layer.instrument,
                step_ms,
                preset.phrase_len,
                &mut rng,
            );
            if layer.instrument == Instrument::Piano {
                if let Some(bass_note) = bass_note_this_step {
                    complement_bass_with_piano(&mut event, bass_note, layer.note_max, &mut rng);
                }
            }

            push_event(&queue, event);

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
    rng: &mut impl Rng,
) -> bool {
    match instrument {
        Instrument::Pad => is_phrase_start,
        Instrument::Bass => match global_step % 12 {
            0 | 3 | 5 | 7 | 10 => true,
            2 | 4 | 8 | 11 => rng.random_range(0..100) < 65,
            _ => rng.random_range(0..100) < 35,
        },
        Instrument::Piano => {
            let density = if base_step_ms > 700.0 {
                22
            } else if bass_played_this_step {
                42
            } else {
                58
            };
            is_phrase_start || rng.random_range(0..100) < density
        }
        Instrument::Pluck | Instrument::Sine => is_phrase_start || rng.random_range(0..100) < 62,
        _ => true,
    }
}

fn humanize_velocity(
    event: &mut NoteEvent,
    preset_vel_min: f32,
    preset_vel_max: f32,
    layer_vel_scale: f32,
    rng: &mut impl Rng,
) {
    let layer_min = (preset_vel_min * layer_vel_scale).clamp(0.0, 1.0);
    let layer_max = (preset_vel_max * layer_vel_scale).clamp(layer_min, 1.0);
    let modifier = rng.random_range(0.90..1.10);

    event.velocity = (event.velocity * modifier).clamp(layer_min, layer_max);
}

fn humanize_duration(
    event: &mut NoteEvent,
    instrument: Instrument,
    step_ms: f32,
    phrase_len: usize,
    rng: &mut impl Rng,
) {
    match instrument {
        Instrument::Pad => {
            let phrase_tail = step_ms * (phrase_len as f32 - 0.4).max(1.0);
            event.duration = event.duration.max(phrase_tail);
        }
        Instrument::Bass => {
            let roll = rng.random_range(0..100);
            event.duration = match roll {
                0..=14 => step_ms * rng.random_range(2.8..4.4),
                15..=38 => step_ms * rng.random_range(1.35..2.3),
                39..=74 => step_ms * rng.random_range(0.72..1.18),
                _ => step_ms * rng.random_range(0.32..0.62),
            };

            if event.is_phrase_end {
                event.duration = event.duration.max(step_ms * rng.random_range(2.0..3.4));
            }
        }
        Instrument::Piano => {
            let modifier = if event.is_phrase_end {
                rng.random_range(1.25..1.75)
            } else {
                rng.random_range(0.82..1.16)
            };
            event.duration = (event.duration * modifier).clamp(step_ms * 0.45, step_ms * 2.8);
        }
        Instrument::Pluck | Instrument::Sine => {
            event.duration = (event.duration * rng.random_range(0.80..1.18))
                .clamp(step_ms * 0.35, step_ms * 2.0);
        }
        _ => {
            event.duration = event.duration.min(step_ms);
        }
    }
}

fn step_sleep_ms(base_step_ms: f32, global_step: usize, rng: &mut impl Rng) -> f32 {
    let swing = if global_step.is_multiple_of(2) {
        0.92
    } else {
        1.08
    };
    let jitter = rng.random_range(-6.0..6.0);

    (base_step_ms * swing + jitter).max(20.0)
}

fn complement_bass_with_piano(
    event: &mut NoteEvent,
    bass_note: u8,
    layer_note_max: u8,
    rng: &mut impl Rng,
) {
    if event.note < bass_note.saturating_add(12) {
        let raised = event.note.saturating_add(12);
        if raised <= layer_note_max {
            event.note = raised;
        }
    }

    event.velocity *= rng.random_range(0.70..0.90);
    event.duration *= rng.random_range(0.82..1.18);
}

fn maybe_push_bass_companion(
    queue: &ArrayQueue<NoteEvent>,
    event: NoteEvent,
    note_min: u8,
    note_max: u8,
    rng: &mut impl Rng,
) {
    let chance = if event.is_phrase_start || event.duration > 900.0 {
        24
    } else {
        10
    };

    if rng.random_range(0..100) >= chance {
        return;
    }

    let candidates = [
        event.note.saturating_add(7),
        event.note.saturating_add(12),
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
    companion.velocity *= rng.random_range(0.34..0.52);
    companion.duration *= rng.random_range(0.58..0.88);
    push_event(queue, companion);
}

fn push_event(queue: &ArrayQueue<NoteEvent>, event: NoteEvent) {
    if queue.push(event).is_err() {
        eprintln!("[composer] note queue is full; dropping one note");
    }
}
