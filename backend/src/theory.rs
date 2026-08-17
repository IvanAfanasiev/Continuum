use crate::instruments::Instrument;
use crate::markov::{Chord, LayerConfig};
use rand::prelude::*;
use crate::section::{HarmonicFunction, HarmonicMoment};


pub(crate) const JAZZ_DEGREES: &[u8] = &[0, 2, 3, 5, 7, 9, 10];

pub(crate) fn color_offset(chord: &Chord, preferred: u8) -> u8 {
    chord
        .notes
        .iter()
        .copied()
        .min_by_key(|&offset| pitch_class_distance(offset % 12, preferred % 12))
        .unwrap_or(preferred)
        % 12
}

pub(crate) fn alternate_offset(chord: &Chord, current: u8, step: usize) -> u8 {
    chord
        .notes
        .iter()
        .copied()
        .filter(|&offset| offset % 12 != current % 12)
        .nth(step % chord.notes.len().max(1))
        .unwrap_or((current + 7) % 12)
        % 12
}
pub(crate) fn nearest_note_for_pc(min: u8, max: u8, pc: u8, preferred: u8) -> Option<u8> {
    (min..=max)
        .filter(|note| note % 12 == pc % 12)
        .min_by_key(|&note| (note as i16 - preferred as i16).abs())
}

pub(crate) fn smooth_note_for_pc(
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

pub(crate) fn nearby_chord_note(
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

pub(crate) fn alternate_note(
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

pub(crate) fn pitch_class_distance(a: u8, b: u8) -> u8 {
    let diff = (a as i16 - b as i16).rem_euclid(12) as u8;
    diff.min(12 - diff)
}

pub(crate) fn close_step_gaps(steps: &mut Vec<usize>, phrase_len: usize, max_gap: usize) {
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

pub(crate) fn blend_transition_steps(current: &[usize], next: &[usize], phrase_len: usize) -> Vec<usize> {
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

pub(crate) fn blend_transition_degrees(current: &[u8], next: &[u8], count: usize) -> Vec<u8> {
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

pub(crate) fn apply_harmonic_melody_gravity(mut degrees: Vec<u8>, harmony: HarmonicMoment) -> Vec<u8> {
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

pub(crate) fn smooth_jazz_degrees(degrees: Vec<u8>) -> Vec<u8> {
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

pub(crate) fn move_jazz_degree(degree: u8, movement: isize) -> u8 {
    let index = nearest_jazz_degree_index(degree);

    JAZZ_DEGREES[move_jazz_degree_index(index, movement)]
}

pub(crate) fn nearest_jazz_degree_index(degree: u8) -> usize {
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

pub(crate) fn move_jazz_degree_index(index: usize, movement: isize) -> usize {
    (index as isize + movement).clamp(0, JAZZ_DEGREES.len().saturating_sub(1) as isize) as usize
}

pub(crate) fn is_far_from_marked(pattern: &[bool], step: usize, min_distance: usize) -> bool {
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

pub(crate) fn next_marked_distance(pattern: &[bool], step: usize) -> Option<usize> {
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