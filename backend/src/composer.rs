use crate::controls::RuntimeControls;
use crate::instruments::Instrument;
use crate::markov::{get_preset, LayerConfig, MarkovGenerator, MarkovPreset};
use crate::NoteEvent;
use crossbeam_queue::ArrayQueue;
use rand::prelude::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use crate::phrase::PhrasePlan;
use crate::section::SectionState;
use crate::step::{
    align_to_phrase, complement_bass_with_piano, humanize_duration, humanize_velocity,
    maybe_push_piano_chord, phrase_step_lengths, should_trigger, support_piano_with_bass,
};

const COMPOSER_LOOKAHEAD_MS: u64 = 180;

pub(crate) fn is_ambient_preset(preset: &MarkovPreset) -> bool {
    preset.name.eq_ignore_ascii_case("ambient")
}

#[derive(Clone, Copy)]
pub(crate) struct VelocityRange {
    pub min: f32,
    pub max: f32,
}

pub(crate) struct GenerationContext<'a> {
    pub phrase_plan: &'a PhrasePlan,
    pub section: &'a SectionState,
}

pub fn start_composing(
    queue: Arc<ArrayQueue<NoteEvent>>,
    preset_name: &str,
    controls: Arc<RuntimeControls>,
    stop: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
) {
    thread::sleep(Duration::from_millis(200));

    let preset = get_preset(preset_name);
    println!("[composer] generating preset: {}", preset.name);

    let mut rng = rand::rng();
    let mut generators: Vec<MarkovGenerator> = preset
        .layers
        .iter()
        .map(|layer| MarkovGenerator::new(layer, preset, rng.random::<u64>()))
        .collect();

    let mut global_chord_idx = if is_ambient_preset(preset) {
        0
    } else {
        rng.random_range(0..preset.chords.len().max(1))
    };
    let mut section = SectionState::new(preset, &mut rng);
    let mut last_notes = vec![None; preset.layers.len()];
    let velocity_range = VelocityRange {
        min: preset.vel_min,
        max: preset.vel_max,
    };
    let lookahead = Duration::from_millis(COMPOSER_LOOKAHEAD_MS);
    let mut phrase_start_at = Instant::now() + lookahead;

    while !stop.load(Ordering::Relaxed) {
        wait_while_paused(&mut phrase_start_at, paused.as_ref(), stop.as_ref());
        if stop.load(Ordering::Relaxed) {
            break;
        }

        let current_chord = preset.chords[global_chord_idx];
        let step_ms = preset.base_step_ms;
        let phrase_plan = PhrasePlan::new(preset, current_chord, &section, &mut rng);
        let step_lengths = phrase_step_lengths(
            step_ms,
            preset.phrase_len,
            &phrase_plan,
            controls.as_ref(),
            &mut rng,
        );
        let context = GenerationContext {
            phrase_plan: &phrase_plan,
            section: &section,
        };
        let phrase_enqueued_at = Instant::now();
        let phrase_delay_ms = delay_until_ms(phrase_start_at, phrase_enqueued_at);
        let mut step_start_ms = 0.0f32;

        for global_step in 0..preset.phrase_len {
            if stop.load(Ordering::Relaxed) {
                break;
            }

            let is_phrase_start = global_step == 0;
            let event_delay_ms = phrase_delay_ms + step_start_ms;
            let mut bass_note_this_step = None;
            let mut piano_note_this_step = None;

            for (index, layer) in preset.layers.iter().enumerate() {
                if !should_trigger(
                    layer.instrument,
                    is_phrase_start,
                    global_step,
                    step_ms,
                    bass_note_this_step.is_some(),
                    &context,
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
                if layer.instrument == Instrument::Bass && !phrase_plan.ambient {
                    support_piano_with_bass(
                        &mut event,
                        current_chord,
                        layer,
                        &phrase_plan,
                        global_step,
                        piano_note_this_step,
                    );
                }
                generator.revise_last_note(event.note);

                humanize_velocity(
                    &mut event,
                    layer,
                    velocity_range,
                    &context,
                    global_step,
                    &mut rng,
                );
                humanize_duration(
                    &mut event,
                    layer.instrument,
                    step_ms,
                    preset.phrase_len,
                    &context,
                    global_step,
                    &mut rng,
                );

                if layer.instrument == Instrument::Piano {
                    if let Some(bass_note) = bass_note_this_step {
                        complement_bass_with_piano(&mut event, bass_note, layer.note_max, &mut rng);
                    }
                }

                event.start_delay_ms = event_delay_ms;
                push_event(&queue, event);
                last_notes[index] = Some(event.note);

                if layer.instrument == Instrument::Piano && !phrase_plan.ambient {
                    piano_note_this_step = Some(event.note);
                    maybe_push_piano_chord(
                        &queue,
                        event,
                        current_chord,
                        layer,
                        &context,
                        global_step,
                        &mut rng,
                    );
                }

                if layer.instrument == Instrument::Bass {
                    bass_note_this_step = Some(event.note);
                }
            }

            if let Some(length) = step_lengths.get(global_step) {
                step_start_ms += *length;
            }
        }

        phrase_start_at += Duration::from_secs_f32(step_start_ms / 1000.0);
        global_chord_idx = (global_chord_idx + 1) % preset.chords.len();
        section.advance(preset, &mut rng);
        sleep_until_enqueue_window(
            &mut phrase_start_at,
            lookahead,
            stop.as_ref(),
            paused.as_ref(),
        );
    }
}

pub(crate) fn push_event(queue: &ArrayQueue<NoteEvent>, event: NoteEvent) {
    if queue.push(event).is_err() {
        eprintln!("[composer] note queue is full; dropping one note");
    }
}
fn delay_until_ms(deadline: Instant, now: Instant) -> f32 {
    deadline
        .checked_duration_since(now)
        .map(|duration| duration.as_secs_f32() * 1000.0)
        .unwrap_or(0.0)
}

fn wait_while_paused(timeline_anchor: &mut Instant, paused: &AtomicBool, stop: &AtomicBool) {
    if !paused.load(Ordering::Relaxed) {
        return;
    }

    let pause_started = Instant::now();
    while paused.load(Ordering::Relaxed) && !stop.load(Ordering::Relaxed) {
        thread::sleep(Duration::from_millis(20));
    }
    *timeline_anchor += pause_started.elapsed();
}

fn sleep_until_enqueue_window(
    next_phrase_start: &mut Instant,
    lookahead: Duration,
    stop: &AtomicBool,
    paused: &AtomicBool,
) {
    loop {
        if stop.load(Ordering::Relaxed) {
            break;
        }

        wait_while_paused(next_phrase_start, paused, stop);

        let enqueue_deadline = next_phrase_start
            .checked_sub(lookahead)
            .unwrap_or_else(Instant::now);
        let Some(remaining) = enqueue_deadline.checked_duration_since(Instant::now()) else {
            break;
        };

        if remaining <= Duration::from_millis(2) {
            break;
        }

        thread::sleep(remaining.min(Duration::from_millis(25)));
    }
}