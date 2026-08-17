use crate::instruments::Instrument;
use crate::markov::MarkovPreset;
use rand::prelude::*;
use std::f32::consts::PI;

use crate::composer::is_ambient_preset;
use crate::theory::{
    JAZZ_DEGREES, apply_harmonic_melody_gravity, blend_transition_degrees, blend_transition_steps, close_step_gaps, move_jazz_degree, move_jazz_degree_index, smooth_jazz_degrees,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum PhraseRole {
    Statement, Echo, Answer, Development, Cadence, Transition,
}

#[derive(Clone, Copy)]
pub(crate) struct PhraseVariation {
    pub role: PhraseRole,
    pub step_shift: isize,
    pub degree_motion: isize,
    pub density: f32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum HarmonicFunction {
    Home, Color, Tension, Release, Pivot,
}

#[derive(Clone, Copy)]
pub(crate) struct HarmonicMoment {
    pub function: HarmonicFunction,
    pub tension: f32,
    pub chord_tones: [u8; 4],
    pub chord_note_count: usize,
    pub bass_anchor: u8,
    pub approach_degree: Option<u8>,
    pub arpeggio_bias: f32,
}

#[derive(Clone)]
pub(crate) struct HarmonicPlan {
    pub moments: Vec<HarmonicMoment>,
}

impl HarmonicPlan {
    pub(crate) fn new(variations: &[PhraseVariation], rng: &mut impl Rng) -> Self {
        let moments = variations
            .iter()
            .map(|variation| harmonic_moment_for(*variation, rng))
            .collect();

        Self { moments }
    }

    pub(crate) fn moment(&self, phrase_pos: usize) -> HarmonicMoment {
        self.moments
            .get(phrase_pos % self.moments.len().max(1))
            .copied()
            .unwrap_or_else(default_harmonic_moment)
    }
}

#[derive(Clone)]
pub(crate) struct GroovePlan {
    pub beat_strength: Vec<f32>,
    pub beat_steps: Vec<usize>,
    pub kick_steps: Vec<usize>,
    pub ride_steps: Vec<usize>,
    pub hihat_closures: Vec<usize>,
    pub melody_windows: Vec<usize>,
}

impl GroovePlan {
    pub(crate) fn new(phrase_len: usize) -> Self {
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

    pub(crate) fn beat_strength(&self, step: usize) -> f32 {
        self.beat_strength[step % self.beat_strength.len()]
    }

    pub(crate) fn kick_steps(&self) -> &[usize] {
        &self.kick_steps
    }

    pub(crate) fn beat_steps(&self) -> &[usize] {
        &self.beat_steps
    }

    pub(crate) fn ride_steps(&self) -> &[usize] {
        &self.ride_steps
    }

    pub(crate) fn hihat_closures(&self) -> &[usize] {
        &self.hihat_closures
    }

    pub(crate) fn melody_windows(&self) -> &[usize] {
        &self.melody_windows
    }

    pub(crate) fn is_kick_step(&self, step: usize) -> bool {
        self.kick_steps.contains(&(step % self.beat_strength.len()))
    }

    pub(crate) fn nearest_melody_step(&self, step: usize) -> usize {
        self.melody_windows
            .iter()
            .copied()
            .min_by_key(|&candidate| candidate.abs_diff(step))
            .unwrap_or(step)
    }
}

#[derive(Clone)]
pub(crate) struct SectionPlan {
    pub phrase_count: usize,
    pub motif_steps: Vec<usize>,
    pub motif_degrees: Vec<u8>,
    pub answer_degrees: Vec<u8>,
    pub bass_steps: Vec<usize>,
    pub bass_degrees: Vec<u8>,
    pub chord_accent_index: usize,
    pub variations: Vec<PhraseVariation>,
    pub harmony: HarmonicPlan,
    pub groove: GroovePlan,
}

impl SectionPlan {
    pub(crate) fn new(phrase_count: usize, phrase_len: usize, rng: &mut impl Rng) -> Self {
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

    pub(crate) fn variation(&self, phrase_pos: usize) -> PhraseVariation {
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

    pub(crate) fn shifted_steps(&self, phrase_len: usize, variation: PhraseVariation) -> Vec<usize> {
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

    pub(crate) fn melody_degrees(&self, count: usize, variation: PhraseVariation) -> Vec<u8> {
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

    pub(crate) fn bass_line(&self, variation: PhraseVariation) -> (Vec<usize>, Vec<u8>) {
        let mut degrees = self.bass_degrees.clone();

        if matches!(variation.role, PhraseRole::Cadence | PhraseRole::Transition) {
            if let Some(last) = degrees.last_mut() {
                *last = 0;
            }
        }

        (self.bass_steps.clone(), degrees)
    }

    pub(crate) fn harmony(&self, phrase_pos: usize) -> HarmonicMoment {
        self.harmony.moment(phrase_pos)
    }

    pub(crate) fn groove(&self) -> &GroovePlan {
        &self.groove
    }
}

#[derive(Clone)]
pub(crate) struct SectionState {
    pub phrase_index: usize,
    pub section_len: usize,
    pub section_pos: usize,
    pub presence: f32,
    pub variation: f32,
    pub ambient: bool,
    pub current_plan: SectionPlan,
    pub next_plan: SectionPlan,
}

impl SectionState {
    pub(crate) fn new(preset: &MarkovPreset, rng: &mut impl Rng) -> Self {
        let ambient = is_ambient_preset(preset);
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

    pub(crate) fn advance(&mut self, preset: &MarkovPreset, rng: &mut impl Rng) {
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

    pub(crate) fn layer_presence(&self, instrument: Instrument) -> f32 {
        match instrument {
            Instrument::Kick | Instrument::Ride | Instrument::Hihat if !self.ambient => {
                (0.18 + self.presence * 0.72).clamp(0.0, 0.92)
            }
            Instrument::Triangle if self.ambient => self.presence,
            _ => 1.0,
        }
    }

    pub(crate) fn update_shape(&mut self) {
        let denom = self.section_len.saturating_sub(1).max(1) as f32;
        let x = self.section_pos as f32 / denom;
        self.presence = (x * PI)
            .sin()
            .max(0.0)
            .powf(if self.ambient { 1.65 } else { 1.35 });
    }

    pub(crate) fn current_variation(&self) -> PhraseVariation {
        self.current_plan.variation(self.section_pos)
    }

    pub(crate) fn current_harmony(&self) -> HarmonicMoment {
        self.current_plan.harmony(self.section_pos)
    }

    pub(crate) fn current_groove(&self) -> &GroovePlan {
        self.current_plan.groove()
    }

    pub(crate) fn jazz_piano_steps(&self, phrase_len: usize) -> Vec<usize> {
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

    pub(crate) fn jazz_melody_degrees(&self, count: usize) -> Vec<u8> {
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

    pub(crate) fn jazz_chord_accent_index(&self) -> usize {
        if self.current_variation().role == PhraseRole::Transition {
            1
        } else {
            self.current_plan.chord_accent_index
        }
    }

    pub(crate) fn jazz_bass_line(&self) -> (Vec<usize>, Vec<u8>) {
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

pub(crate) fn generate_section_motif_steps(groove: &GroovePlan, rng: &mut impl Rng) -> Vec<usize> {
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

pub(crate) fn generate_section_motif_degrees(count: usize, rng: &mut impl Rng) -> Vec<u8> {
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

pub(crate) fn generate_answer_degrees(motif_degrees: &[u8], rng: &mut impl Rng) -> Vec<u8> {
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

pub(crate) fn generate_section_bass_line(groove: &GroovePlan, rng: &mut impl Rng) -> (Vec<usize>, Vec<u8>) {
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

pub(crate) fn generate_phrase_variations(phrase_count: usize, rng: &mut impl Rng) -> Vec<PhraseVariation> {
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

pub(crate) fn harmonic_moment_for(variation: PhraseVariation, rng: &mut impl Rng) -> HarmonicMoment {
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

pub(crate) fn default_harmonic_moment() -> HarmonicMoment {
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
