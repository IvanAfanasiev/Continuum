use crate::instruments::Instrument;
use crate::markov::{get_preset, Chord, LayerConfig, MarkovGenerator, MarkovPreset};
use crate::NoteEvent;
use crossbeam_queue::ArrayQueue;
use rand::prelude::*;
use std::f32::consts::PI;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

const COMPOSER_LOOKAHEAD_MS: u64 = 180;

#[derive(Clone)]
struct PhrasePlan {
    offsets: Vec<u8>,
    piano_offsets: Vec<u8>,
    piano_steps: Vec<bool>,
    piano_chord_steps: Vec<bool>,
    accents: Vec<f32>,
    activity: Vec<f32>,
    bass_steps: Vec<bool>,
    bass_offsets: Vec<u8>,
    kick_steps: Vec<bool>,
    ride_steps: Vec<bool>,
    hihat_steps: Vec<bool>,
    ambient: bool,
}

struct JazzPhraseSketch {
    piano_steps: Vec<bool>,
    piano_offsets: Vec<u8>,
    piano_chord_steps: Vec<bool>,
    bass_steps: Vec<bool>,
    bass_offsets: Vec<u8>,
    kick_steps: Vec<bool>,
    hihat_steps: Vec<bool>,
    ride_steps: Vec<bool>,
}

const JAZZ_DEGREES: &[u8] = &[0, 2, 3, 5, 7, 9, 10];

#[derive(Clone, Copy, PartialEq, Eq)]
enum PhraseRole {
    Statement,
    Echo,
    Answer,
    Development,
    Cadence,
    Transition,
}

#[derive(Clone, Copy)]
struct PhraseVariation {
    role: PhraseRole,
    step_shift: isize,
    degree_motion: isize,
    density: f32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum HarmonicFunction {
    Home,
    Color,
    Tension,
    Release,
    Pivot,
}

#[derive(Clone, Copy)]
struct HarmonicMoment {
    function: HarmonicFunction,
    tension: f32,
    chord_tones: [u8; 4],
    chord_note_count: usize,
    bass_anchor: u8,
    approach_degree: Option<u8>,
    arpeggio_bias: f32,
}

#[derive(Clone)]
struct HarmonicPlan {
    moments: Vec<HarmonicMoment>,
}

struct PianoVoicing {
    notes: Vec<u8>,
    arpeggiate: bool,
    delay_ms: f32,
}

#[derive(Clone)]
struct GroovePlan {
    beat_strength: Vec<f32>,
    beat_steps: Vec<usize>,
    kick_steps: Vec<usize>,
    ride_steps: Vec<usize>,
    hihat_closures: Vec<usize>,
    melody_windows: Vec<usize>,
}

impl GroovePlan {
    fn new(phrase_len: usize) -> Self {
        let len = phrase_len.max(1);
        let bar_steps = if len >= 8 { 8 } else { len };
        let beat_interval = (bar_steps / 4).max(1);
        let mut beat_strength = vec![0.30; len];
        let mut beat_steps = Vec::new();

        for (step, strength) in beat_strength.iter_mut().enumerate() {
            let bar_pos = step % bar_steps;
            let is_beat = bar_pos.is_multiple_of(beat_interval);
            if is_beat {
                beat_steps.push(step);
            }

            *strength = if bar_pos == 0 {
                1.0
            } else if bar_pos == beat_interval * 2 {
                0.84
            } else if is_beat {
                0.64
            } else if bar_pos + 1 == bar_steps {
                0.50
            } else {
                0.34
            };
        }

        let kick_steps: Vec<usize> = (0..len).filter(|step| step % bar_steps == 0).collect();

        let mut ride_steps = Vec::new();
        for bar_start in (0..len).step_by(bar_steps) {
            for offset in [
                0,
                beat_interval,
                beat_interval + 1,
                beat_interval * 2,
                beat_interval * 3,
                beat_interval * 3 + 1,
            ] {
                let step = bar_start + offset;
                if step < len {
                    ride_steps.push(step);
                }
            }
        }
        ride_steps.sort_unstable();
        ride_steps.dedup();

        let mut hihat_closures = Vec::new();
        for bar_start in (0..len).step_by(bar_steps) {
            for offset in [beat_interval, beat_interval * 3] {
                let step = bar_start + offset;
                if step < len {
                    hihat_closures.push(step);
                }
            }
        }
        hihat_closures.sort_unstable();
        hihat_closures.dedup();

        let melody_windows: Vec<usize> =
            (0..len).filter(|step| !kick_steps.contains(step)).collect();

        Self {
            beat_strength,
            beat_steps,
            kick_steps,
            ride_steps,
            hihat_closures,
            melody_windows,
        }
    }

    fn beat_strength(&self, step: usize) -> f32 {
        self.beat_strength[step % self.beat_strength.len()]
    }

    fn kick_steps(&self) -> &[usize] {
        &self.kick_steps
    }

    fn beat_steps(&self) -> &[usize] {
        &self.beat_steps
    }

    fn ride_steps(&self) -> &[usize] {
        &self.ride_steps
    }

    fn hihat_closures(&self) -> &[usize] {
        &self.hihat_closures
    }

    fn melody_windows(&self) -> &[usize] {
        &self.melody_windows
    }

    fn is_kick_step(&self, step: usize) -> bool {
        self.kick_steps.contains(&(step % self.beat_strength.len()))
    }

    fn nearest_melody_step(&self, step: usize) -> usize {
        self.melody_windows
            .iter()
            .copied()
            .min_by_key(|&candidate| candidate.abs_diff(step))
            .unwrap_or(step)
    }
}

impl HarmonicPlan {
    fn new(variations: &[PhraseVariation], rng: &mut impl Rng) -> Self {
        let moments = variations
            .iter()
            .map(|variation| harmonic_moment_for(*variation, rng))
            .collect();

        Self { moments }
    }

    fn moment(&self, phrase_pos: usize) -> HarmonicMoment {
        self.moments
            .get(phrase_pos % self.moments.len().max(1))
            .copied()
            .unwrap_or_else(default_harmonic_moment)
    }
}

#[derive(Clone)]
struct SectionPlan {
    phrase_count: usize,
    motif_steps: Vec<usize>,
    motif_degrees: Vec<u8>,
    answer_degrees: Vec<u8>,
    bass_steps: Vec<usize>,
    bass_degrees: Vec<u8>,
    chord_accent_index: usize,
    variations: Vec<PhraseVariation>,
    harmony: HarmonicPlan,
    groove: GroovePlan,
}

impl SectionPlan {
    fn new(phrase_count: usize, phrase_len: usize, rng: &mut impl Rng) -> Self {
        let groove = GroovePlan::new(phrase_len);
        let motif_steps = generate_section_motif_steps(&groove, rng);
        let motif_degrees = generate_section_motif_degrees(motif_steps.len(), rng);
        let answer_degrees = generate_answer_degrees(&motif_degrees, rng);
        let (bass_steps, bass_degrees) = generate_section_bass_line(&groove, rng);
        let chord_accent_index = if motif_steps.len() > 1 {
            let start = (motif_steps.len() / 2).max(1).min(motif_steps.len() - 1);
            rng.random_range(start..motif_steps.len())
        } else {
            0
        };
        let variations = generate_phrase_variations(phrase_count, rng);
        let harmony = HarmonicPlan::new(&variations, rng);

        Self {
            phrase_count,
            motif_steps,
            motif_degrees,
            answer_degrees,
            bass_steps,
            bass_degrees,
            chord_accent_index,
            variations,
            harmony,
            groove,
        }
    }

    fn variation(&self, phrase_pos: usize) -> PhraseVariation {
        self.variations
            .get(phrase_pos % self.variations.len().max(1))
            .copied()
            .unwrap_or(PhraseVariation {
                role: PhraseRole::Statement,
                step_shift: 0,
                degree_motion: 0,
                density: 1.0,
            })
    }

    fn shifted_steps(&self, phrase_len: usize, variation: PhraseVariation) -> Vec<usize> {
        let mut steps: Vec<usize> = self
            .motif_steps
            .iter()
            .enumerate()
            .filter_map(|(index, &step)| {
                if variation.density < 0.78 && index > 0 && index + 1 < self.motif_steps.len() {
                    return None;
                }

                let shift = if step == 0 { 0 } else { variation.step_shift };
                let shifted = (step as isize + shift)
                    .clamp(0, phrase_len.saturating_sub(1) as isize)
                    as usize;
                Some(shifted)
            })
            .collect();

        if matches!(variation.role, PhraseRole::Cadence | PhraseRole::Transition)
            && phrase_len > 2
            && !steps.contains(&(phrase_len - 1))
        {
            steps.push(phrase_len - 1);
        }

        steps.sort_unstable();
        steps.dedup();
        let max_gap = if phrase_len >= 12 { 2 } else { 3 };
        close_step_gaps(&mut steps, phrase_len, max_gap);
        for step in &mut steps {
            if self.groove.is_kick_step(*step) {
                *step = self.groove.nearest_melody_step(*step);
            }
        }
        steps.sort_unstable();
        steps.dedup();
        steps
    }

    fn melody_degrees(&self, count: usize, variation: PhraseVariation) -> Vec<u8> {
        let source = match variation.role {
            PhraseRole::Answer | PhraseRole::Cadence => &self.answer_degrees,
            _ => &self.motif_degrees,
        };

        let mut degrees: Vec<u8> = (0..count)
            .map(|index| {
                let degree = source
                    .get(index % source.len().max(1))
                    .copied()
                    .unwrap_or(0);
                move_jazz_degree(degree, variation.degree_motion)
            })
            .collect();

        if matches!(variation.role, PhraseRole::Cadence | PhraseRole::Transition) {
            if let Some(last) = degrees.last_mut() {
                *last = 0;
            }
        }

        degrees
    }

    fn bass_line(&self, variation: PhraseVariation) -> (Vec<usize>, Vec<u8>) {
        let mut degrees = self.bass_degrees.clone();

        if matches!(variation.role, PhraseRole::Cadence | PhraseRole::Transition) {
            if let Some(last) = degrees.last_mut() {
                *last = 0;
            }
        }

        (self.bass_steps.clone(), degrees)
    }

    fn harmony(&self, phrase_pos: usize) -> HarmonicMoment {
        self.harmony.moment(phrase_pos)
    }

    fn groove(&self) -> &GroovePlan {
        &self.groove
    }
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
        let mut piano_offsets = vec![0; len];
        let mut piano_steps = vec![false; len];
        let mut piano_chord_steps = vec![false; len];
        let mut accents = Vec::with_capacity(len);
        let mut activity = Vec::with_capacity(len);
        let mut bass_steps = vec![false; len];
        let mut bass_offsets = vec![0; len];
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
            piano_offsets[step] = offset;

            let accent = if ambient {
                if step == 0 {
                    1.0
                } else if step.is_multiple_of(4) {
                    0.86
                } else if step.is_multiple_of(2) {
                    0.70
                } else {
                    0.52
                }
            } else {
                section.current_groove().beat_strength(step)
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
                let groove = section.current_groove();
                let base = if groove.is_kick_step(step) {
                    0.82
                } else if groove.melody_windows().contains(&step) {
                    0.56 + groove.beat_strength(step) * 0.18
                } else {
                    0.20
                };
                (base + section.presence * 0.04).clamp(0.14, 0.92)
            };
            activity.push(activity_value);
        }

        if !ambient {
            let sketch = generate_jazz_phrase_sketch(len, chord, section, rng);
            piano_steps = sketch.piano_steps;
            piano_offsets = sketch.piano_offsets;
            piano_chord_steps = sketch.piano_chord_steps;
            bass_steps = sketch.bass_steps;
            bass_offsets = sketch.bass_offsets;
            kick_steps = sketch.kick_steps;
            hihat_steps = sketch.hihat_steps;
            ride_steps = sketch.ride_steps;

            for step in 0..len {
                let planned = piano_steps[step] || bass_steps[step] || kick_steps[step];
                let texture = if planned {
                    0.74
                } else if hihat_steps[step] || ride_steps[step] {
                    0.48
                } else {
                    0.18
                };
                activity[step] =
                    (texture + accents[step] * 0.12 + section.presence * 0.06).clamp(0.12, 0.92);
            }
        } else {
            for step in 0..len {
                piano_steps[step] = activity[step] > 0.74;
            }
        }

        Self {
            offsets,
            piano_offsets,
            piano_steps,
            piano_chord_steps,
            accents,
            activity,
            bass_steps,
            bass_offsets,
            kick_steps,
            ride_steps,
            hihat_steps,
            ambient,
        }
    }

    fn offset(&self, step: usize) -> u8 {
        self.offsets[step % self.offsets.len()]
    }

    fn piano_offset(&self, step: usize) -> u8 {
        self.piano_offsets[step % self.piano_offsets.len()]
    }

    fn is_piano_step(&self, step: usize) -> bool {
        self.piano_steps[step % self.piano_steps.len()]
    }

    fn is_piano_chord_step(&self, step: usize) -> bool {
        self.piano_chord_steps[step % self.piano_chord_steps.len()]
    }

    fn bass_offset(&self, step: usize) -> u8 {
        self.bass_offsets[step % self.bass_offsets.len()]
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
    current_plan: SectionPlan,
    next_plan: SectionPlan,
}

impl SectionState {
    fn new(preset: &MarkovPreset, rng: &mut impl Rng) -> Self {
        let ambient = preset.base_step_ms > 700.0;
        let section_len = if ambient {
            rng.random_range(5..=9)
        } else {
            rng.random_range(8..=12)
        };
        let current_plan = SectionPlan::new(section_len, preset.phrase_len, rng);
        let next_plan = SectionPlan::new(rng.random_range(8..=12), preset.phrase_len, rng);
        let mut state = Self {
            phrase_index: 0,
            section_len,
            section_pos: 0,
            presence: 0.0,
            variation: rng.random_range(0.0..1.0),
            ambient,
            current_plan,
            next_plan,
        };
        state.update_shape();
        state
    }

    fn advance(&mut self, preset: &MarkovPreset, rng: &mut impl Rng) {
        self.phrase_index += 1;
        self.section_pos += 1;

        if self.section_pos >= self.section_len {
            self.section_pos = 0;
            self.section_len = if self.ambient {
                rng.random_range(5..=10)
            } else {
                self.current_plan = self.next_plan.clone();
                self.next_plan = SectionPlan::new(rng.random_range(8..=12), preset.phrase_len, rng);
                self.current_plan.phrase_count
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

    fn current_variation(&self) -> PhraseVariation {
        self.current_plan.variation(self.section_pos)
    }

    fn current_harmony(&self) -> HarmonicMoment {
        self.current_plan.harmony(self.section_pos)
    }

    fn current_groove(&self) -> &GroovePlan {
        self.current_plan.groove()
    }

    fn jazz_piano_steps(&self, phrase_len: usize) -> Vec<usize> {
        let variation = self.current_variation();

        if variation.role == PhraseRole::Transition {
            return blend_transition_steps(
                &self.current_plan.shifted_steps(phrase_len, variation),
                &self.next_plan.shifted_steps(
                    phrase_len,
                    PhraseVariation {
                        role: PhraseRole::Statement,
                        step_shift: 0,
                        degree_motion: 0,
                        density: 1.0,
                    },
                ),
                phrase_len,
            );
        }

        self.current_plan.shifted_steps(phrase_len, variation)
    }

    fn jazz_melody_degrees(&self, count: usize) -> Vec<u8> {
        let variation = self.current_variation();
        let harmony = self.current_harmony();

        if variation.role == PhraseRole::Transition {
            let current = apply_harmonic_melody_gravity(
                self.current_plan.melody_degrees(count, variation),
                harmony,
            );
            let next = self.next_plan.melody_degrees(
                count,
                PhraseVariation {
                    role: PhraseRole::Statement,
                    step_shift: 0,
                    degree_motion: 0,
                    density: 1.0,
                },
            );
            return blend_transition_degrees(&current, &next, count);
        }

        apply_harmonic_melody_gravity(self.current_plan.melody_degrees(count, variation), harmony)
    }

    fn jazz_chord_accent_index(&self) -> usize {
        if self.current_variation().role == PhraseRole::Transition {
            1
        } else {
            self.current_plan.chord_accent_index
        }
    }

    fn jazz_bass_line(&self) -> (Vec<usize>, Vec<u8>) {
        let variation = self.current_variation();
        let harmony = self.current_harmony();

        if variation.role == PhraseRole::Transition {
            let (mut steps, mut degrees) = self.current_plan.bass_line(variation);
            if let Some(first) = degrees.first_mut() {
                *first = harmony.bass_anchor;
            }
            let (next_steps, next_degrees) = self.next_plan.bass_line(PhraseVariation {
                role: PhraseRole::Statement,
                step_shift: 0,
                degree_motion: 0,
                density: 1.0,
            });

            if let (Some(&next_step), Some(&next_degree)) =
                (next_steps.first(), next_degrees.first())
            {
                let transition_step = next_step.max(steps.last().copied().unwrap_or(0));
                steps.push(transition_step);
                degrees.push(next_degree);
            }

            return (steps, degrees);
        }

        let (steps, mut degrees) = self.current_plan.bass_line(variation);
        if let Some(first) = degrees.first_mut() {
            *first = harmony.bass_anchor;
        }
        if let Some(approach) = harmony.approach_degree {
            if matches!(
                harmony.function,
                HarmonicFunction::Tension | HarmonicFunction::Pivot
            ) {
                if let Some(last) = degrees.last_mut() {
                    *last = approach;
                }
            }
        }

        (steps, degrees)
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

    let mut rng = rand::rng();
    let mut generators: Vec<MarkovGenerator> = preset
        .layers
        .iter()
        .map(|layer| MarkovGenerator::new(layer, preset, rng.random::<u64>()))
        .collect();

    let mut global_chord_idx = if preset.base_step_ms > 700.0 {
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

    loop {
        let current_chord = preset.chords[global_chord_idx];
        let step_ms = preset.base_step_ms;
        let phrase_plan = PhrasePlan::new(preset, current_chord, &section, &mut rng);
        let step_lengths = phrase_step_lengths(step_ms, preset.phrase_len, &phrase_plan, &mut rng);
        let context = GenerationContext {
            phrase_plan: &phrase_plan,
            section: &section,
        };
        let phrase_enqueued_at = Instant::now();
        let phrase_delay_ms = delay_until_ms(phrase_start_at, phrase_enqueued_at);
        let mut step_start_ms = 0.0f32;

        for global_step in 0..preset.phrase_len {
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
        sleep_until_enqueue_window(phrase_start_at, lookahead);
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
                phrase_plan.is_piano_step(global_step)
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
        Instrument::Bass if !phrase_plan.ambient => {
            offset = phrase_plan.bass_offset(step);
        }
        Instrument::Piano if !phrase_plan.ambient => {
            offset = phrase_plan.piano_offset(step);
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
        Instrument::Kick => 0.28,
        Instrument::Ride => 0.38,
        Instrument::Hihat => 0.22,
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

fn humanize_duration(
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

fn phrase_step_lengths(
    base_step_ms: f32,
    phrase_len: usize,
    phrase_plan: &PhrasePlan,
    rng: &mut impl Rng,
) -> Vec<f32> {
    (0..phrase_len)
        .map(|step| step_sleep_ms(base_step_ms, step, phrase_plan, rng))
        .collect()
}

fn delay_until_ms(deadline: Instant, now: Instant) -> f32 {
    deadline
        .checked_duration_since(now)
        .map(|duration| duration.as_secs_f32() * 1000.0)
        .unwrap_or(0.0)
}

fn sleep_until_enqueue_window(next_phrase_start: Instant, lookahead: Duration) {
    let enqueue_deadline = next_phrase_start
        .checked_sub(lookahead)
        .unwrap_or_else(Instant::now);

    while let Some(remaining) = enqueue_deadline.checked_duration_since(Instant::now()) {
        if remaining <= Duration::from_millis(2) {
            break;
        }

        thread::sleep(remaining.min(Duration::from_millis(25)));
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
        1.18
    } else {
        0.82
    };
    let jitter = if phrase_plan.ambient {
        rng.random_range(-14.0..18.0)
    } else {
        0.0
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

fn maybe_push_piano_chord(
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

fn piano_voicing(
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

fn fill_piano_voicing(
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

fn piano_voicing_gain(harmony: HarmonicMoment, note_index: usize, rng: &mut impl Rng) -> f32 {
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

fn smooth_note_for_pc(
    min: u8,
    max: u8,
    pc: u8,
    last_note: u8,
    preferred: u8,
    max_jump: u8,
) -> Option<u8> {
    (min..=max)
        .filter(|note| note % 12 == pc % 12)
        .min_by_key(|&note| {
            let jump = note.abs_diff(last_note);
            let jump_penalty = if jump > max_jump { 80 } else { 0 };
            jump as u16 * 8 + preferred.abs_diff(note) as u16 + jump_penalty
        })
}

fn nearby_chord_note(
    layer: &LayerConfig,
    chord: &Chord,
    current: u8,
    last_note: u8,
    preferred: u8,
    rng: &mut impl Rng,
) -> Option<u8> {
    let max_jump = if layer.instrument == Instrument::Bass {
        5
    } else {
        7
    };
    let mut candidates: Vec<u8> = (layer.note_min..=layer.note_max)
        .filter(|&note| {
            note != current && chord.contains(note) && note.abs_diff(last_note) <= max_jump
        })
        .collect();

    candidates.shuffle(rng);
    candidates
        .into_iter()
        .min_by_key(|&note| note.abs_diff(last_note) as u16 * 4 + note.abs_diff(preferred) as u16)
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

fn generate_section_motif_steps(groove: &GroovePlan, rng: &mut impl Rng) -> Vec<usize> {
    let windows = groove.melody_windows();
    if windows.is_empty() {
        return vec![0];
    }

    let phrase_len = groove.beat_strength.len().max(1);
    let min_count = if phrase_len >= 12 {
        ((windows.len() * 55) / 100).clamp(6, 9).min(windows.len())
    } else {
        (windows.len() / 2).clamp(2, 4).min(windows.len())
    };
    let max_count = if phrase_len >= 12 {
        ((windows.len() * 75) / 100)
            .clamp(min_count, 12)
            .min(windows.len())
    } else {
        ((windows.len() * 2) / 3)
            .clamp(min_count, 5)
            .min(windows.len())
    };
    let target = rng.random_range(min_count..=max_count);
    let mut steps = Vec::with_capacity(target);
    let mut window_index = rng.random_range(0..windows.len().min(2));

    while steps.len() < target {
        let step = windows[window_index.min(windows.len() - 1)];

        if steps.contains(&step) {
            break;
        }

        steps.push(step);

        let gap = if phrase_len >= 12 {
            if rng.random_range(0..100) < 78 {
                1
            } else {
                2
            }
        } else if rng.random_range(0..100) < 62 {
            1
        } else {
            2
        };
        window_index += gap;
        if window_index >= windows.len() {
            break;
        }
    }

    while steps.len() < target {
        let candidate = windows[((steps.len() * windows.len()) / target).min(windows.len() - 1)];
        if !steps.contains(&candidate) {
            steps.push(candidate);
        } else {
            break;
        }
    }

    steps.sort_unstable();
    steps.dedup();
    let max_gap = if phrase_len >= 12 { 2 } else { 3 };
    close_step_gaps(&mut steps, phrase_len, max_gap);
    for step in &mut steps {
        if groove.is_kick_step(*step) {
            *step = groove.nearest_melody_step(*step);
        }
    }
    steps.sort_unstable();
    steps.dedup();
    steps
}

fn generate_section_motif_degrees(count: usize, rng: &mut impl Rng) -> Vec<u8> {
    if count == 0 {
        return Vec::new();
    }

    let mut index = rng.random_range(0..JAZZ_DEGREES.len());
    let mut direction = if rng.random_bool(0.5) {
        1isize
    } else {
        -1isize
    };
    let mut degrees = Vec::with_capacity(count);

    for note_index in 0..count {
        degrees.push(JAZZ_DEGREES[index]);

        if note_index > 0 && rng.random_bool(0.28) {
            direction *= -1;
        }

        let movement = if rng.random_range(0..100) < 18 {
            0
        } else {
            direction
        };
        index = move_jazz_degree_index(index, movement);
    }

    if let Some(first) = degrees.first_mut() {
        if rng.random_bool(0.42) {
            *first = move_jazz_degree(*first, -1);
        }
    }

    smooth_jazz_degrees(degrees)
}

fn generate_answer_degrees(motif_degrees: &[u8], rng: &mut impl Rng) -> Vec<u8> {
    let mut answer: Vec<u8> = motif_degrees
        .iter()
        .rev()
        .map(|&degree| move_jazz_degree(degree, if rng.random_bool(0.5) { -1 } else { 1 }))
        .collect();

    if let Some(last) = answer.last_mut() {
        *last = 0;
    }

    smooth_jazz_degrees(answer)
}

fn generate_section_bass_line(groove: &GroovePlan, rng: &mut impl Rng) -> (Vec<usize>, Vec<u8>) {
    let phrase_len = groove.beat_strength.len().max(1);
    let mut steps = if phrase_len >= 12 {
        groove.beat_steps().to_vec()
    } else {
        groove.kick_steps().to_vec()
    };

    if phrase_len > 6 && phrase_len < 12 && rng.random_bool(0.44) {
        let fill = phrase_len - 2;
        if !groove.is_kick_step(fill) && !steps.contains(&fill) {
            steps.push(fill);
        }
    }

    steps.sort_unstable();
    steps.dedup();

    let mut degrees = Vec::with_capacity(steps.len());
    for (index, &step) in steps.iter().enumerate() {
        let degree = if phrase_len >= 12 {
            match index % 4 {
                0 => 0,
                1 => {
                    if rng.random_bool(0.58) {
                        3
                    } else {
                        5
                    }
                }
                2 => 7,
                _ => {
                    if rng.random_bool(0.62) {
                        10
                    } else {
                        5
                    }
                }
            }
        } else if groove.is_kick_step(step) {
            0
        } else if rng.random_bool(0.70) {
            7
        } else {
            5
        };
        degrees.push(degree);
    }

    if let Some(last) = degrees.last_mut() {
        if phrase_len < 12 && !groove.is_kick_step(*steps.last().unwrap_or(&0)) {
            *last = match rng.random_range(0..100) {
                0..=54 => 7,
                55..=84 => 5,
                _ => 3,
            };
        }
    }

    (steps, degrees)
}

fn generate_phrase_variations(phrase_count: usize, rng: &mut impl Rng) -> Vec<PhraseVariation> {
    (0..phrase_count.max(1))
        .map(|index| {
            let role = if index == 0 {
                PhraseRole::Statement
            } else if index + 1 == phrase_count {
                PhraseRole::Transition
            } else if index + 2 == phrase_count {
                PhraseRole::Cadence
            } else if index % 4 == 3 {
                PhraseRole::Answer
            } else if index % 2 == 1 {
                PhraseRole::Echo
            } else {
                PhraseRole::Development
            };

            let direction = if rng.random_bool(0.5) { 1 } else { -1 };
            let step_shift = match role {
                PhraseRole::Statement | PhraseRole::Cadence | PhraseRole::Transition => 0,
                PhraseRole::Echo => {
                    if rng.random_bool(0.30) {
                        direction
                    } else {
                        0
                    }
                }
                PhraseRole::Answer | PhraseRole::Development => direction,
            };
            let degree_motion = match role {
                PhraseRole::Development => direction,
                PhraseRole::Echo if rng.random_bool(0.28) => direction,
                PhraseRole::Transition => -direction,
                _ => 0,
            };
            let density = match role {
                PhraseRole::Statement | PhraseRole::Cadence => 1.0,
                PhraseRole::Echo => 0.96,
                PhraseRole::Answer => 1.0,
                PhraseRole::Development => 1.08,
                PhraseRole::Transition => 0.90,
            };

            PhraseVariation {
                role,
                step_shift,
                degree_motion,
                density,
            }
        })
        .collect()
}

fn harmonic_moment_for(variation: PhraseVariation, rng: &mut impl Rng) -> HarmonicMoment {
    match variation.role {
        PhraseRole::Statement => HarmonicMoment {
            function: HarmonicFunction::Home,
            tension: 0.22,
            chord_tones: [3, 7, 2, 0],
            chord_note_count: 2,
            bass_anchor: 0,
            approach_degree: None,
            arpeggio_bias: 0.10,
        },
        PhraseRole::Echo => HarmonicMoment {
            function: HarmonicFunction::Color,
            tension: 0.30,
            chord_tones: if rng.random_bool(0.5) {
                [3, 7, 9, 2]
            } else {
                [7, 2, 3, 5]
            },
            chord_note_count: 2,
            bass_anchor: if rng.random_bool(0.72) { 0 } else { 7 },
            approach_degree: None,
            arpeggio_bias: 0.16,
        },
        PhraseRole::Development => HarmonicMoment {
            function: HarmonicFunction::Tension,
            tension: 0.58,
            chord_tones: [7, 10, 2, 9],
            chord_note_count: 3,
            bass_anchor: if rng.random_bool(0.68) { 7 } else { 0 },
            approach_degree: Some(if variation.degree_motion >= 0 { 2 } else { 10 }),
            arpeggio_bias: 0.38,
        },
        PhraseRole::Answer => HarmonicMoment {
            function: HarmonicFunction::Color,
            tension: 0.40,
            chord_tones: [3, 7, 5, 2],
            chord_note_count: 2,
            bass_anchor: 0,
            approach_degree: Some(7),
            arpeggio_bias: 0.22,
        },
        PhraseRole::Cadence => HarmonicMoment {
            function: HarmonicFunction::Release,
            tension: 0.14,
            chord_tones: [3, 7, 2, 0],
            chord_note_count: 3,
            bass_anchor: 0,
            approach_degree: Some(7),
            arpeggio_bias: 0.30,
        },
        PhraseRole::Transition => HarmonicMoment {
            function: HarmonicFunction::Pivot,
            tension: 0.66,
            chord_tones: [7, 10, 9, 2],
            chord_note_count: 2,
            bass_anchor: 7,
            approach_degree: Some(if rng.random_bool(0.5) { 10 } else { 2 }),
            arpeggio_bias: 0.46,
        },
    }
}

fn default_harmonic_moment() -> HarmonicMoment {
    HarmonicMoment {
        function: HarmonicFunction::Home,
        tension: 0.22,
        chord_tones: [3, 7, 2, 0],
        chord_note_count: 2,
        bass_anchor: 0,
        approach_degree: None,
        arpeggio_bias: 0.10,
    }
}

fn blend_transition_steps(current: &[usize], next: &[usize], phrase_len: usize) -> Vec<usize> {
    let split = phrase_len / 2;
    let mut steps: Vec<usize> = current
        .iter()
        .copied()
        .filter(|&step| step < split)
        .collect();
    steps.extend(next.iter().copied().filter(|&step| step >= split));

    if steps.is_empty() {
        steps.extend(current.iter().copied());
    }

    steps.sort_unstable();
    steps.dedup();
    close_step_gaps(&mut steps, phrase_len, 3);
    steps
}

fn close_step_gaps(steps: &mut Vec<usize>, phrase_len: usize, max_gap: usize) {
    if steps.is_empty() || phrase_len == 0 {
        return;
    }

    steps.sort_unstable();
    steps.dedup();

    loop {
        let mut inserted = false;
        let snapshot = steps.clone();

        for pair in snapshot.windows(2) {
            let gap = pair[1].saturating_sub(pair[0]);
            if gap > max_gap {
                let fill = pair[0] + gap / 2;
                if fill < phrase_len && !steps.contains(&fill) {
                    steps.push(fill);
                    inserted = true;
                    break;
                }
            }
        }

        if !inserted {
            break;
        }

        steps.sort_unstable();
        steps.dedup();
    }
}

fn blend_transition_degrees(current: &[u8], next: &[u8], count: usize) -> Vec<u8> {
    (0..count)
        .map(|index| {
            if index < count / 2 {
                current
                    .get(index % current.len().max(1))
                    .copied()
                    .unwrap_or(0)
            } else {
                next.get(index % next.len().max(1)).copied().unwrap_or(0)
            }
        })
        .collect()
}

fn apply_harmonic_melody_gravity(mut degrees: Vec<u8>, harmony: HarmonicMoment) -> Vec<u8> {
    if degrees.is_empty() {
        return degrees;
    }

    match harmony.function {
        HarmonicFunction::Tension | HarmonicFunction::Pivot => {
            let color_index = (degrees.len() / 2).min(degrees.len() - 1);
            degrees[color_index] = move_jazz_degree(harmony.chord_tones[0], 0);
        }
        HarmonicFunction::Release => {
            if let Some(last) = degrees.last_mut() {
                *last = 0;
            }
        }
        HarmonicFunction::Home => {
            if degrees.len() > 2 {
                degrees[0] = move_jazz_degree(degrees[0], 0);
            }
        }
        HarmonicFunction::Color => {}
    }

    smooth_jazz_degrees(degrees)
}

fn smooth_jazz_degrees(degrees: Vec<u8>) -> Vec<u8> {
    let Some((&first, rest)) = degrees.split_first() else {
        return degrees;
    };
    let mut out = Vec::with_capacity(degrees.len());
    out.push(first);

    for &degree in rest {
        let previous = *out.last().unwrap_or(&degree);
        let previous_index = nearest_jazz_degree_index(previous);
        let target_index = nearest_jazz_degree_index(degree);
        let smoothed_index = if target_index > previous_index + 1 {
            previous_index + 1
        } else if previous_index > target_index + 1 {
            previous_index - 1
        } else {
            target_index
        };
        out.push(JAZZ_DEGREES[smoothed_index]);
    }

    out
}

fn move_jazz_degree(degree: u8, movement: isize) -> u8 {
    let index = nearest_jazz_degree_index(degree);

    JAZZ_DEGREES[move_jazz_degree_index(index, movement)]
}

fn nearest_jazz_degree_index(degree: u8) -> usize {
    JAZZ_DEGREES
        .iter()
        .position(|&candidate| candidate == degree)
        .unwrap_or_else(|| {
            JAZZ_DEGREES
                .iter()
                .enumerate()
                .min_by_key(|&(_, &candidate)| pitch_class_distance(candidate, degree))
                .map(|(index, _)| index)
                .unwrap_or(0)
        })
}

fn move_jazz_degree_index(index: usize, movement: isize) -> usize {
    (index as isize + movement).clamp(0, JAZZ_DEGREES.len().saturating_sub(1) as isize) as usize
}

fn generate_jazz_phrase_sketch(
    len: usize,
    chord: &Chord,
    section: &SectionState,
    rng: &mut impl Rng,
) -> JazzPhraseSketch {
    let mut sketch = JazzPhraseSketch {
        piano_steps: vec![false; len],
        piano_offsets: vec![0; len],
        piano_chord_steps: vec![false; len],
        bass_steps: vec![false; len],
        bass_offsets: vec![0; len],
        kick_steps: vec![false; len],
        hihat_steps: vec![false; len],
        ride_steps: vec![false; len],
    };

    if len == 0 {
        return sketch;
    }

    let piano_steps = generate_jazz_piano_steps(len, section);
    let piano_offsets = generate_jazz_melodic_offsets(chord, section, piano_steps.len());

    for (&step, &offset) in piano_steps.iter().zip(piano_offsets.iter()) {
        sketch.piano_steps[step] = true;
        sketch.piano_offsets[step] = offset;
    }

    for step in choose_piano_chord_steps(&piano_steps, len, section, rng) {
        sketch.piano_chord_steps[step] = true;
    }

    let kick_steps = generate_jazz_kick_steps(len, section);
    for &step in &kick_steps {
        sketch.kick_steps[step] = true;
    }

    add_jazz_bass_support(&mut sketch, chord, section);
    add_jazz_hihat_closures(&mut sketch, section);
    add_jazz_ride_support(&mut sketch, section);

    sketch
}

fn generate_jazz_piano_steps(len: usize, section: &SectionState) -> Vec<usize> {
    let mut steps = section.jazz_piano_steps(len);
    steps.sort_unstable();
    steps.dedup();
    steps
}

fn generate_jazz_melodic_offsets(chord: &Chord, section: &SectionState, count: usize) -> Vec<u8> {
    section
        .jazz_melody_degrees(count)
        .into_iter()
        .map(|degree| color_offset(chord, degree))
        .collect()
}

fn choose_piano_chord_steps(
    piano_steps: &[usize],
    _len: usize,
    section: &SectionState,
    _rng: &mut impl Rng,
) -> Vec<usize> {
    if piano_steps.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::with_capacity(2);
    let accent_index = section
        .jazz_chord_accent_index()
        .min(piano_steps.len().saturating_sub(1));
    out.push(piano_steps[accent_index]);

    if section.presence > 0.62 && piano_steps.len() > 4 {
        let later = piano_steps[piano_steps.len() - 1];
        if out.iter().all(|&step| step.abs_diff(later) > 2) {
            out.push(later);
        }
    }

    out
}

fn generate_jazz_kick_steps(len: usize, section: &SectionState) -> Vec<usize> {
    if len == 0 {
        return Vec::new();
    }

    let mut kicks: Vec<usize> = section
        .current_groove()
        .kick_steps()
        .iter()
        .copied()
        .filter(|&step| step < len)
        .collect();

    kicks.sort_unstable();
    kicks.dedup();
    kicks
}

fn add_jazz_bass_support(sketch: &mut JazzPhraseSketch, chord: &Chord, section: &SectionState) {
    let len = sketch.bass_steps.len();
    if len == 0 {
        return;
    }

    let (bass_steps, bass_degrees) = section.jazz_bass_line();

    for (&step, &degree) in bass_steps.iter().zip(bass_degrees.iter()) {
        if step >= len {
            continue;
        }

        sketch.bass_steps[step] = true;
        sketch.bass_offsets[step] = color_offset(chord, degree);
    }

    if !sketch.bass_steps.iter().any(|&active| active) {
        sketch.bass_steps[0] = true;
        sketch.bass_offsets[0] = 0;
    }

    for step in 0..len {
        if sketch.kick_steps[step] {
            sketch.bass_steps[step] = true;
            sketch.bass_offsets[step] = 0;
        }
    }

    let harmony = section.current_harmony();
    let groove = section.current_groove();
    for step in 0..len {
        if sketch.piano_chord_steps[step] && !groove.hihat_closures().contains(&step) {
            sketch.bass_steps[step] = true;
            sketch.bass_offsets[step] = color_offset(chord, harmony.bass_anchor);
        }
    }
}

fn add_jazz_hihat_closures(sketch: &mut JazzPhraseSketch, section: &SectionState) {
    let len = sketch.hihat_steps.len();
    let groove = section.current_groove();
    let mut added = 0usize;

    for &closure in groove.hihat_closures() {
        if closure >= len {
            continue;
        };
        if !sketch.kick_steps[closure] && is_far_from_marked(&sketch.hihat_steps, closure, 2) {
            sketch.hihat_steps[closure] = true;
            added += 1;
        }
    }

    if added == 0 {
        for step in (0..len).rev() {
            if !sketch.kick_steps[step] {
                sketch.hihat_steps[step] = true;
                break;
            }
        }
    }
}

fn add_jazz_ride_support(sketch: &mut JazzPhraseSketch, section: &SectionState) {
    let groove = section.current_groove();

    for &step in groove.ride_steps() {
        if step < sketch.ride_steps.len() {
            sketch.ride_steps[step] = true;
        }
    }
}

fn is_far_from_marked(pattern: &[bool], step: usize, min_distance: usize) -> bool {
    if pattern.is_empty() {
        return true;
    }

    let len = pattern.len();
    for distance in 0..=min_distance {
        let left = (step + len - distance % len) % len;
        let right = (step + distance) % len;
        if pattern[left] || pattern[right] {
            return false;
        }
    }

    true
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
