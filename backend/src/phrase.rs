use crate::markov::{Chord, MarkovPreset};
use rand::prelude::*;

use crate::composer::is_ambient_preset;
use crate::section::SectionState;
use crate::theory::{color_offset, alternate_offset, is_far_from_marked, next_marked_distance};

#[derive(Clone)]
pub(crate) struct PhrasePlan {
    pub offsets: Vec<u8>,
    pub piano_offsets: Vec<u8>,
    pub piano_steps: Vec<bool>,
    pub piano_chord_steps: Vec<bool>,
    pub accents: Vec<f32>,
    pub activity: Vec<f32>,
    pub bass_steps: Vec<bool>,
    pub bass_offsets: Vec<u8>,
    pub kick_steps: Vec<bool>,
    pub ride_steps: Vec<bool>,
    pub hihat_steps: Vec<bool>,
    pub ambient: bool,
}

pub(crate) struct JazzPhraseSketch {
    pub piano_steps: Vec<bool>,
    pub piano_offsets: Vec<u8>,
    pub piano_chord_steps: Vec<bool>,
    pub bass_steps: Vec<bool>,
    pub bass_offsets: Vec<u8>,
    pub kick_steps: Vec<bool>,
    pub hihat_steps: Vec<bool>,
    pub ride_steps: Vec<bool>,
}

impl PhrasePlan {
    pub(crate) fn new(
        preset: &MarkovPreset,
        chord: &Chord,
        section: &SectionState,
        rng: &mut impl Rng,
    ) -> Self {
        let len = preset.phrase_len.max(1);
        let ambient = is_ambient_preset(preset);
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
                let shifted = (step + section.phrase_index % 4) % len;
                let pulse = shifted.is_multiple_of(4);
                let breath = matches!(shifted, 2 | 6 | 10 | 14);
                let answer = matches!(shifted, 3 | 7 | 11 | 15);

                kick_steps[step] = pulse;
                bass_steps[step] =
                    pulse || (breath && section.presence > 0.32) || activity[step] > 0.86;
                bass_offsets[step] = if pulse {
                    0
                } else if breath {
                    color_offset(chord, 7)
                } else {
                    color_offset(chord, [9, 2, 11][step % 3])
                };
                piano_steps[step] = (breath && !pulse)
                    || (answer && section.variation > 0.42)
                    || (!pulse && activity[step] > 0.72);
                piano_offsets[step] = color_offset(chord, [2, 7, 9, 11][shifted % 4]);
                ride_steps[step] = breath && section.presence > 0.48;
                hihat_steps[step] = answer && section.presence > 0.62;
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

    pub(crate) fn offset(&self, step: usize) -> u8 {
        self.offsets[step % self.offsets.len()]
    }

    pub(crate) fn piano_offset(&self, step: usize) -> u8 {
        self.piano_offsets[step % self.piano_offsets.len()]
    }

    pub(crate) fn is_piano_step(&self, step: usize) -> bool {
        self.piano_steps[step % self.piano_steps.len()]
    }

    pub(crate) fn is_piano_chord_step(&self, step: usize) -> bool {
        self.piano_chord_steps[step % self.piano_chord_steps.len()]
    }

    pub(crate) fn bass_offset(&self, step: usize) -> u8 {
        self.bass_offsets[step % self.bass_offsets.len()]
    }

    pub(crate) fn accent(&self, step: usize) -> f32 {
        self.accents[step % self.accents.len()]
    }

    pub(crate) fn activity(&self, step: usize) -> f32 {
        self.activity[step % self.activity.len()]
    }

    pub(crate) fn is_echo_step(&self, step: usize) -> bool {
        self.ambient && self.activity(step) > 0.75
    }

    pub(crate) fn is_pad_shift(&self, step: usize) -> bool {
        let local = step % self.offsets.len();
        self.ambient && (local == self.offsets.len() / 2 || local == (self.offsets.len() * 3) / 4)
    }

    pub(crate) fn is_bass_step(&self, step: usize) -> bool {
        self.bass_steps[step % self.bass_steps.len()]
    }

    pub(crate)  fn is_kick_step(&self, step: usize) -> bool {
        self.kick_steps[step % self.kick_steps.len()]
    }

    pub(crate) fn is_ride_step(&self, step: usize) -> bool {
        self.ride_steps[step % self.ride_steps.len()]
    }

    pub(crate) fn is_hihat_step(&self, step: usize) -> bool {
        self.hihat_steps[step % self.hihat_steps.len()]
    }

    pub(crate) fn next_bass_distance(&self, step: usize) -> Option<usize> {
        next_marked_distance(&self.bass_steps, step)
    }

    pub(crate) fn next_hihat_distance(&self, step: usize) -> Option<usize> {
        next_marked_distance(&self.hihat_steps, step)
    }
}

pub(crate) fn generate_jazz_phrase_sketch(
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

pub(crate) fn generate_jazz_piano_steps(len: usize, section: &SectionState) -> Vec<usize> {
    let mut steps = section.jazz_piano_steps(len);
    steps.sort_unstable();
    steps.dedup();
    steps
}

pub(crate) fn generate_jazz_melodic_offsets(chord: &Chord, section: &SectionState, count: usize) -> Vec<u8> {
    section
        .jazz_melody_degrees(count)
        .into_iter()
        .map(|degree| color_offset(chord, degree))
        .collect()
}

pub(crate) fn choose_piano_chord_steps(
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

pub(crate) fn generate_jazz_kick_steps(len: usize, section: &SectionState) -> Vec<usize> {
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

pub(crate) fn add_jazz_bass_support(sketch: &mut JazzPhraseSketch, chord: &Chord, section: &SectionState) {
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

pub(crate) fn add_jazz_hihat_closures(sketch: &mut JazzPhraseSketch, section: &SectionState) {
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

pub(crate) fn add_jazz_ride_support(sketch: &mut JazzPhraseSketch, section: &SectionState) {
    let groove = section.current_groove();

    for &step in groove.ride_steps() {
        if step < sketch.ride_steps.len() {
            sketch.ride_steps[step] = true;
        }
    }
}