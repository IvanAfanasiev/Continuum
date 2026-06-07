use crate::instruments::Instrument;
use crate::markov::{get_preset, Chord, LayerConfig, MarkovGenerator, MarkovPreset};
use crate::NoteEvent;
use crossbeam_queue::ArrayQueue;
use rand::prelude::*;
use std::f32::consts::PI;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

#[derive(Clone)]
struct PhrasePlan {
    offsets: Vec<u8>,
    accents: Vec<f32>,
    activity: Vec<f32>,
    bass_steps: Vec<bool>,
    kick_steps: Vec<bool>,
    ride_steps: Vec<bool>,
    hihat_steps: Vec<bool>,
    ambient: bool,
}

struct JazzGroovePattern {
    kick_anchors: &'static [usize],
    bass_fills: &'static [usize],
    hihat_closures: &'static [usize],
}

impl PhrasePlan {
    fn new(
        preset: &MarkovPreset,
        chord: &Chord,
        section: &SectionState,
        rng: &mut impl Rng,
    ) -> Self {
        let len = preset.phrase_len.max(1);
        let ambient = preset.base_step_ms > 700.0;
        let motif = if ambient {
            let colors = if section.variation > 0.58 {
                [2, 9, 11, 7]
            } else {
                [2, 7, 11, 4]
            };
            [
                color_offset(chord, colors[0]),
                color_offset(chord, colors[1]),
                color_offset(chord, colors[2]),
                color_offset(chord, colors[3]),
            ]
        } else {
            let leading_color = if section.variation > 0.55 { 3 } else { 2 };
            let answer_color = if section.variation > 0.72 {
                9
            } else if section.variation > 0.36 {
                5
            } else {
                7
            };
            [
                0,
                color_offset(
                    chord,
                    if rng.random_bool(0.55) {
                        2
                    } else {
                        leading_color
                    },
                ),
                7,
                color_offset(chord, answer_color),
            ]
        };

        let mut offsets = Vec::with_capacity(len);
        let mut accents = Vec::with_capacity(len);
        let mut activity = Vec::with_capacity(len);
        let mut bass_steps = vec![false; len];
        let mut kick_steps = vec![false; len];
        let mut ride_steps = vec![false; len];
        let mut hihat_steps = vec![false; len];

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
                let drifted_step = (step + section.phrase_index % 4) % len;
                let base = match drifted_step {
                    2 | 7 | 10 => 0.90,
                    0 | 6 => 0.55,
                    _ => 0.18,
                };
                (base + section.presence * 0.10).clamp(0.08, 0.96)
            } else {
                let base = match step % 12 {
                    0 | 7 => 0.88,
                    3 | 10 => 0.78,
                    2 | 5 | 8 => 0.58,
                    11 => 0.50,
                    _ => 0.20,
                };
                (base + section.presence * 0.04).clamp(0.14, 0.92)
            };
            activity.push(activity_value);
        }

        if !ambient {
            let groove = jazz_groove_pattern(section.variation);

            // Jazz dependency order: kick anchors -> bass line -> hihat closures.
            for &step in groove.kick_anchors {
                if step < len {
                    kick_steps[step] = true;
                    bass_steps[step] = true;
                }
            }

            for &step in groove.bass_fills {
                if step < len {
                    bass_steps[step] = true;
                }
            }

            for &step in groove.hihat_closures {
                if step < len && !bass_steps[step] {
                    hihat_steps[step] = true;
                }
            }

            for step in 0..len {
                let previous_bass = previous_marked_distance(&bass_steps, step);
                ride_steps[step] = previous_bass
                    .is_some_and(|distance| (1..=3).contains(&distance))
                    && !bass_steps[step]
                    && !hihat_steps[step]
                    && step.is_multiple_of(2);
            }
        }

        Self {
            offsets,
            accents,
            activity,
            bass_steps,
            kick_steps,
            ride_steps,
            hihat_steps,
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

    fn is_bass_step(&self, step: usize) -> bool {
        self.bass_steps[step % self.bass_steps.len()]
    }

    fn is_kick_step(&self, step: usize) -> bool {
        self.kick_steps[step % self.kick_steps.len()]
    }

    fn is_ride_step(&self, step: usize) -> bool {
        self.ride_steps[step % self.ride_steps.len()]
    }

    fn is_hihat_step(&self, step: usize) -> bool {
        self.hihat_steps[step % self.hihat_steps.len()]
    }

    fn next_bass_distance(&self, step: usize) -> Option<usize> {
        next_marked_distance(&self.bass_steps, step)
    }

    fn next_hihat_distance(&self, step: usize) -> Option<usize> {
        next_marked_distance(&self.hihat_steps, step)
    }
}

#[derive(Clone)]
struct SectionState {
    phrase_index: usize,
    section_len: usize,
    section_pos: usize,
    presence: f32,
    variation: f32,
    ambient: bool,
}

impl SectionState {
    fn new(preset: &MarkovPreset, rng: &mut impl Rng) -> Self {
        let ambient = preset.base_step_ms > 700.0;
        let section_len = if ambient {
            rng.random_range(5..=9)
        } else {
            rng.random_range(8..=14)
        };
        let mut state = Self {
            phrase_index: 0,
            section_len,
            section_pos: 0,
            presence: 0.0,
            variation: rng.random_range(0.0..1.0),
            ambient,
        };
        state.update_shape();
        state
    }

    fn advance(&mut self, rng: &mut impl Rng) {
        self.phrase_index += 1;
        self.section_pos += 1;

        if self.section_pos >= self.section_len {
            self.section_pos = 0;
            self.section_len = if self.ambient {
                rng.random_range(5..=10)
            } else {
                rng.random_range(8..=15)
            };
            self.variation = rng.random_range(0.0..1.0);
        } else {
            self.variation =
                (self.variation * 0.88 + rng.random_range(0.0..1.0) * 0.12).clamp(0.0, 1.0);
        }

        self.update_shape();
    }

    fn layer_presence(&self, instrument: Instrument) -> f32 {
        match instrument {
            Instrument::Sax if !self.ambient => self.presence,
            Instrument::Kick | Instrument::Ride | Instrument::Hihat if !self.ambient => {
                (0.18 + self.presence * 0.72).clamp(0.0, 0.92)
            }
            Instrument::Triangle if self.ambient => self.presence,
            _ => 1.0,
        }
    }

    fn update_shape(&mut self) {
        let denom = self.section_len.saturating_sub(1).max(1) as f32;
        let x = self.section_pos as f32 / denom;
        self.presence = (x * PI)
            .sin()
            .max(0.0)
            .powf(if self.ambient { 1.65 } else { 1.35 });
    }
}

struct GenerationContext<'a> {
    phrase_plan: &'a PhrasePlan,
    section: &'a SectionState,
}

#[derive(Clone, Copy)]
struct VelocityRange {
    min: f32,
    max: f32,
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
    let mut section = SectionState::new(preset, &mut rng);
    let mut phrase_plan =
        PhrasePlan::new(preset, preset.chords[global_chord_idx], &section, &mut rng);
    let mut last_notes = vec![None; preset.layers.len()];
    let velocity_range = VelocityRange {
        min: preset.vel_min,
        max: preset.vel_max,
    };

    loop {
        let is_phrase_start = global_step == 0;
        let current_chord = preset.chords[global_chord_idx];

        if is_phrase_start {
            phrase_plan = PhrasePlan::new(preset, current_chord, &section, &mut rng);
        }

        let step_ms = preset.base_step_ms;
        let sleep_ms = step_sleep_ms(step_ms, global_step, &phrase_plan, &mut rng);
        let mut bass_note_this_step = None;
        let mut piano_note_this_step = None;
        let context = GenerationContext {
            phrase_plan: &phrase_plan,
            section: &section,
        };

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

            if layer.instrument == Instrument::Piano && !phrase_plan.ambient {
                piano_note_this_step = Some(event.note);
                maybe_push_piano_chord(
                    &queue,
                    event,
                    current_chord,
                    layer,
                    &phrase_plan,
                    global_step,
                    &mut rng,
                );
            }

            if layer.instrument == Instrument::Bass {
                bass_note_this_step = Some(event.note);
            }
        }

        global_step += 1;
        if global_step >= preset.phrase_len {
            global_step = 0;
            global_chord_idx = (global_chord_idx + 1) % preset.chords.len();
            section.advance(&mut rng);
        }

        thread::sleep(Duration::from_millis(sleep_ms.round() as u64));
    }
}

fn should_trigger(
    instrument: Instrument,
    is_phrase_start: bool,
    global_step: usize,
    base_step_ms: f32,
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
                let activity = phrase_plan.activity(global_step);
                activity > 0.92 || rng.random_range(0.0..1.0) < activity * 0.66
            } else {
                phrase_plan.is_bass_step(global_step)
            }
        }
        Instrument::Piano => {
            if base_step_ms > 700.0 {
                phrase_plan.is_echo_step(global_step) && rng.random_range(0.0..1.0) < 0.34
            } else {
                matches!(global_step % 12, 0 | 3 | 7 | 10)
                    || (matches!(global_step % 12, 2 | 5 | 8) && rng.random_range(0.0..1.0) < 0.52)
                    || (matches!(global_step % 12, 11) && rng.random_range(0.0..1.0) < 0.62)
            }
        }
        Instrument::Sax => {
            if phrase_plan.ambient {
                return false;
            }

            let response_step = matches!(global_step % 12, 1 | 4 | 8 | 11);
            response_step
                && rng.random_range(0.0..1.0)
                    < section.layer_presence(instrument) * phrase_plan.activity(global_step) * 0.58
        }
        Instrument::Triangle => {
            if !phrase_plan.ambient || !phrase_plan.is_echo_step(global_step) {
                return false;
            }

            rng.random_range(0.0..1.0)
                < section.layer_presence(instrument)
                    * (0.14 + phrase_plan.accent(global_step) * 0.20)
        }
        Instrument::Kick => !phrase_plan.ambient && phrase_plan.is_kick_step(global_step),
        Instrument::Ride => {
            if phrase_plan.ambient {
                return false;
            }

            phrase_plan.is_ride_step(global_step)
                && rng.random_range(0.0..1.0) < section.layer_presence(instrument) * 0.74
        }
        Instrument::Hihat => {
            if phrase_plan.ambient {
                return false;
            }

            phrase_plan.is_hihat_step(global_step)
        }
        Instrument::Pluck | Instrument::Sine => is_phrase_start || rng.random_range(0..100) < 48,
        Instrument::Organ | Instrument::Snare => true,
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
        Instrument::Bass if phrase_plan.is_kick_step(step) => {
            offset = 0;
        }
        Instrument::Piano if !phrase_plan.ambient => {
            offset = match step % 12 {
                0 => 0,
                2 => color_offset(chord, 2),
                3 => color_offset(chord, 2),
                5 => color_offset(chord, 5),
                7 => color_offset(chord, 7),
                8 => color_offset(chord, 9),
                10 => color_offset(chord, 9),
                11 => color_offset(chord, 2),
                _ => offset,
            };
        }
        Instrument::Pad if offset == 0 => {
            offset = color_offset(chord, 9);
        }
        Instrument::Sax => {
            offset = match step % 12 {
                1 | 8 => color_offset(chord, 9),
                4 => color_offset(chord, 3),
                11 => color_offset(chord, 2),
                _ => offset,
            };
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
        if last == note || (phrase_plan.ambient && last % 12 == note % 12) {
            note = alternate_note(layer, chord, note, preferred, step, rng).unwrap_or(note);
        }
    }

    event.note = note;
}

fn humanize_velocity(
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
        Instrument::Sax => 0.62,
        Instrument::Triangle => 0.50,
        Instrument::Kick => 0.26,
        Instrument::Ride => 0.21,
        Instrument::Hihat => 0.19,
        Instrument::Snare => 0.48,
        _ => 0.85,
    };
    let section_gain = match layer.instrument {
        Instrument::Sax | Instrument::Triangle => {
            0.20 + section.layer_presence(layer.instrument) * 0.80
        }
        Instrument::Kick | Instrument::Ride | Instrument::Hihat | Instrument::Snare => {
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
            Instrument::Kick | Instrument::Ride | Instrument::Hihat | Instrument::Snare => {
                0.92 + accent * 0.08
            }
            _ => 0.88 + accent * 0.12,
        }
    };
    let nuance = rng.random_range(0.992..1.008);

    event.velocity = (base_velocity * accent_gain * role_gain * section_gain * nuance)
        .clamp(layer_min, layer_max);
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
                let held = if let Some(steps_to_hat) = steps_to_hat {
                    steps_to_hat * rng.random_range(0.82..0.96)
                } else if phrase_plan.is_kick_step(step) {
                    steps_to_next * rng.random_range(0.78..0.92)
                } else {
                    steps_to_next * rng.random_range(0.46..0.68)
                }
                .clamp(1.05, 4.2);
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
                let modifier = match step % 12 {
                    0 => rng.random_range(1.00..1.36),
                    3 | 7 | 10 => rng.random_range(0.66..1.02),
                    2 | 5 | 8 | 11 => rng.random_range(0.36..0.70),
                    _ if event.is_phrase_end => rng.random_range(1.15..1.55),
                    _ => rng.random_range(0.70..1.05),
                };
                event.duration = (event.duration * modifier).clamp(step_ms * 0.36, step_ms * 2.1);
            }
        }
        Instrument::Sax => {
            let modifier = if event.is_phrase_end {
                rng.random_range(1.6..2.5)
            } else if phrase_plan.accent(step) > 0.85 {
                rng.random_range(1.05..1.85)
            } else {
                rng.random_range(0.72..1.28)
            };
            event.duration = (step_ms * modifier).clamp(step_ms * 0.55, step_ms * 2.7);
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
        Instrument::Snare => {
            event.duration = step_ms * 0.16;
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

fn support_piano_with_bass(
    event: &mut NoteEvent,
    chord: &Chord,
    layer: &LayerConfig,
    phrase_plan: &PhrasePlan,
    step: usize,
    piano_note: Option<u8>,
) {
    let offset = if phrase_plan.is_kick_step(step) {
        0
    } else {
        match step % 12 {
            0 => 0,
            3 => color_offset(chord, 3),
            4 | 5 => color_offset(chord, 5),
            7 | 8 => color_offset(chord, 7),
            10 => color_offset(chord, 10),
            11 => color_offset(chord, 2),
            _ => phrase_plan.offset(step),
        }
    };
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

fn maybe_push_piano_chord(
    queue: &ArrayQueue<NoteEvent>,
    event: NoteEvent,
    chord: &Chord,
    layer: &LayerConfig,
    phrase_plan: &PhrasePlan,
    step: usize,
    rng: &mut impl Rng,
) {
    let chance = match step % 12 {
        3 | 7 => 0.24,
        5 => 0.16,
        10 => 0.22,
        11 => 0.52,
        _ => 0.0,
    };

    if chance <= 0.0 || rng.random_range(0.0..1.0) >= chance {
        return;
    }

    let max_notes = if event.is_phrase_end || step % 12 == 11 {
        3
    } else {
        2
    };
    let mut voicing = piano_chord_voicing(chord, layer, phrase_plan, step, event.note);
    voicing.retain(|&note| note != event.note && note.abs_diff(event.note) > 1);

    for note in voicing.into_iter().take(max_notes) {
        let mut chord_event = event;
        chord_event.note = note;
        chord_event.velocity *= rng.random_range(0.24..0.38);
        chord_event.duration = (event.duration * rng.random_range(1.15..1.65)).clamp(220.0, 980.0);
        chord_event.is_phrase_start = false;
        push_event(queue, chord_event);
    }
}

fn piano_chord_voicing(
    chord: &Chord,
    layer: &LayerConfig,
    phrase_plan: &PhrasePlan,
    step: usize,
    lead_note: u8,
) -> Vec<u8> {
    let offsets: &[u8] = match step % 12 {
        3 => &[3, 10, 2],
        5 => &[3, 7],
        7 => &[7, 10, 2],
        10 | 11 => &[3, 7, 10],
        _ => &[3, 7],
    };
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

    notes.sort_unstable();
    notes.dedup();
    notes
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
        Instrument::Piano => 0.50 + 0.18 * contour,
        Instrument::Sax => 0.46 + 0.22 * contour,
        Instrument::Triangle => 0.78,
        Instrument::Kick | Instrument::Ride | Instrument::Hihat | Instrument::Snare => 0.50,
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

fn jazz_groove_pattern(variation: f32) -> JazzGroovePattern {
    if variation < 0.34 {
        JazzGroovePattern {
            kick_anchors: &[0, 7],
            bass_fills: &[3, 10],
            hihat_closures: &[5],
        }
    } else if variation < 0.68 {
        JazzGroovePattern {
            kick_anchors: &[0, 7],
            bass_fills: &[4, 10, 11],
            hihat_closures: &[5, 9],
        }
    } else {
        JazzGroovePattern {
            kick_anchors: &[0, 7],
            bass_fills: &[2, 4, 10, 11],
            hihat_closures: &[6, 9],
        }
    }
}

fn previous_marked_distance(pattern: &[bool], step: usize) -> Option<usize> {
    if pattern.is_empty() {
        return None;
    }

    let len = pattern.len();
    for distance in 1..=len {
        let index = (step + len - distance % len) % len;
        if pattern[index] {
            return Some(distance);
        }
    }

    None
}

fn next_marked_distance(pattern: &[bool], step: usize) -> Option<usize> {
    if pattern.is_empty() {
        return None;
    }

    let len = pattern.len();
    for distance in 1..=len {
        let index = (step + distance) % len;
        if pattern[index] {
            return Some(distance);
        }
    }

    None
}
