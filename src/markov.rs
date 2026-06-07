use crate::instruments::{EnvelopeConfig, Instrument};
use crate::NoteEvent;
use rand::distr::{weighted::WeightedIndex, Distribution};
use rand::prelude::*;
use std::collections::VecDeque;
use std::f32::consts::TAU;

pub struct Scale {
    pub root: u8,
    pub intervals: &'static [u8],
}

impl Scale {
    pub fn tones_in_range(&self, min: u8, max: u8) -> Vec<u8> {
        if min > max {
            return Vec::new();
        }

        let root_pc = self.root % 12;
        let mut out = Vec::new();
        let start = (min / 12).saturating_sub(1) * 12;

        for octave in (start..=max).step_by(12) {
            for &interval in self.intervals {
                let note = octave + ((root_pc + interval) % 12);
                if note >= min && note <= max {
                    out.push(note);
                }
            }
        }

        out.sort_unstable();
        out.dedup();
        out
    }
}

pub struct Chord {
    pub root: u8,
    pub notes: &'static [u8],
}

impl Chord {
    pub fn contains(&self, note: u8) -> bool {
        let pc = (note as i16 - self.root as i16).rem_euclid(12) as u8;
        self.notes.iter().any(|&chord_pc| chord_pc % 12 == pc)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RhythmRole {
    Melody,
    Bass,
}

pub struct LayerConfig {
    pub instrument: Instrument,
    pub role: RhythmRole,
    pub note_min: u8,
    pub note_max: u8,
    pub vel_scale: f32,
    pub envelope: Option<EnvelopeConfig>,
}

pub struct MarkovPreset {
    pub name: &'static str,
    pub scale: Scale,
    pub chords: &'static [&'static Chord],
    pub phrase_len: usize,
    pub base_step_ms: f32,
    pub vel_min: f32,
    pub vel_max: f32,
    pub layers: &'static [LayerConfig],
}

pub struct MarkovGenerator {
    history: VecDeque<u8>,
    tones: Vec<u8>,
    pub phrase_pos: usize,
    pub chord_idx: usize,
    tempo_phase: f32,
    rng: StdRng,
    preset: &'static MarkovPreset,
}

impl MarkovGenerator {
    pub fn new(layer: &LayerConfig, preset: &'static MarkovPreset, seed: u64) -> Self {
        let mut rng = StdRng::seed_from_u64(seed);
        let mut tones = preset.scale.tones_in_range(layer.note_min, layer.note_max);

        if tones.is_empty() {
            tones.push(layer.note_min.min(layer.note_max));
        }

        let start_note = tones[tones.len() / 2];

        Self {
            history: VecDeque::from(vec![start_note]),
            tones,
            phrase_pos: 0,
            chord_idx: 0,
            tempo_phase: rng.random_range(0.0..TAU),
            rng,
            preset,
        }
    }

    pub fn next(&mut self, layer: &LayerConfig) -> NoteEvent {
        let phrase_len = self.preset.phrase_len.max(1);
        let is_phrase_start = self.phrase_pos == 0;
        let is_phrase_end = self.phrase_pos >= phrase_len.saturating_sub(1);
        let is_penultimate = self.phrase_pos == phrase_len.saturating_sub(2);
        let current_chord = self.preset.chords[self.chord_idx];

        let raw_note = if layer.role == RhythmRole::Bass {
            self.pick_bass_note(current_chord, is_phrase_start || is_phrase_end)
        } else {
            self.pick_melody_note(is_phrase_end, is_penultimate)
        };

        let final_note = self.fit_to_chord(raw_note, current_chord, layer, is_phrase_start);
        self.remember(final_note);

        let duration = self.dynamic_duration(is_phrase_end);
        let base_vel = self
            .rng
            .random_range(self.preset.vel_min..=self.preset.vel_max);

        NoteEvent {
            note: final_note,
            velocity: (base_vel * layer.vel_scale).clamp(0.0, 1.0),
            duration,
            instrument: layer.instrument,
            envelope: layer
                .envelope
                .unwrap_or_else(|| layer.instrument.default_envelope()),
            is_phrase_start,
            is_phrase_end,
        }
    }

    fn fit_to_chord(
        &self,
        raw_note: u8,
        current_chord: &Chord,
        layer: &LayerConfig,
        is_phrase_start: bool,
    ) -> u8 {
        let should_anchor = is_phrase_start
            || layer.instrument == Instrument::Pad
            || layer.role == RhythmRole::Bass;

        if !should_anchor || current_chord.contains(raw_note) {
            return raw_note;
        }

        self.tones
            .iter()
            .copied()
            .filter(|&note| current_chord.contains(note))
            .min_by_key(|&note| (note as i16 - raw_note as i16).abs())
            .unwrap_or(raw_note)
    }

    fn dynamic_duration(&mut self, is_phrase_end: bool) -> f32 {
        self.tempo_phase = (self.tempo_phase + 0.12) % TAU;
        let rubato = 1.0 + self.tempo_phase.sin() * 0.12;
        let base_ms = self.preset.base_step_ms * rubato;

        if is_phrase_end {
            return base_ms * self.rng.random_range(1.8..2.8);
        }

        match self.rng.random_range(0..100) {
            0..65 => base_ms,
            65..85 => base_ms * 0.55,
            _ => base_ms * 1.35,
        }
    }

    fn pick_melody_note(&mut self, is_last: bool, is_penultimate: bool) -> u8 {
        let prev = *self.history.back().unwrap_or(&self.tones[0]);
        let prev_prev = if self.history.len() >= 2 {
            self.history[self.history.len() - 2]
        } else {
            prev
        };

        let chord = self.preset.chords[self.chord_idx];
        let tonic_root = self.preset.chords[0].root % 12;
        let phrase_len = self.preset.phrase_len.max(1) as f32;
        let phrase_progress = self.phrase_pos as f32 / phrase_len;
        let mut candidates = Vec::with_capacity(self.tones.len());

        for &note in &self.tones {
            let mut score = 1.0f32;
            let interval = (note as i16 - prev as i16).abs();

            score *= match interval {
                0 => 0.18,
                1 | 2 => 5.0,
                3 | 4 => 2.4,
                5 | 7 => 1.0,
                6 => 0.12,
                _ => 0.05,
            };

            let last_motion = prev as i16 - prev_prev as i16;
            if last_motion.abs() > 4 {
                let current_motion = note as i16 - prev as i16;
                if (last_motion > 0 && current_motion < 0)
                    || (last_motion < 0 && current_motion > 0)
                {
                    score *= 4.0;
                } else {
                    score *= 0.08;
                }
            }

            let is_chord_tone = chord.contains(note);
            if self.phrase_pos.is_multiple_of(2) {
                score *= if is_chord_tone { 3.5 } else { 0.35 };
            } else if !is_chord_tone {
                score *= 1.7;
            }

            let target_peak = if phrase_progress < 0.65 {
                phrase_progress / 0.65
            } else {
                1.0 - ((phrase_progress - 0.65) / 0.35)
            };
            let low = *self.tones.first().unwrap_or(&note) as f32;
            let high = *self.tones.last().unwrap_or(&note) as f32;
            let ideal = low + (high - low) * (0.22 + 0.48 * target_peak);
            let contour_distance = (note as f32 - ideal).abs();
            score *= (-contour_distance * contour_distance / 36.0).exp();

            if is_last {
                if note % 12 == tonic_root {
                    score *= 12.0;
                } else if is_chord_tone {
                    score *= 2.5;
                } else {
                    score *= 0.02;
                }
            } else if is_penultimate {
                let dist_to_tonic = pitch_class_distance(note % 12, tonic_root);
                if dist_to_tonic == 1 || dist_to_tonic == 2 {
                    score *= 2.2;
                }
            }

            if self.history.contains(&note) {
                score *= 0.45;
            }

            candidates.push((note, score.max(0.001)));
        }

        self.sample_weighted(&candidates)
    }

    fn pick_bass_note(&mut self, chord: &Chord, prefer_root: bool) -> u8 {
        let prev = *self.history.back().unwrap_or(&self.tones[0]);
        let root_pc = chord.root % 12;
        let strong_beat = prefer_root || self.phrase_pos.is_multiple_of(4);
        let mut candidates = Vec::new();

        for &note in &self.tones {
            let interval = (note as i16 - prev as i16).abs();
            let rel_pc = (note as i16 - chord.root as i16).rem_euclid(12) as u8;
            let is_chord_tone = chord.contains(note);

            let mut score = match (strong_beat, rel_pc, is_chord_tone) {
                (true, 0, _) => 9.0,
                (true, 7, _) => 3.2,
                (true, 3 | 4, _) => 1.8,
                (true, _, true) => 1.2,
                (true, _, false) => 0.18,
                (false, 0, _) => 2.6,
                (false, 7, _) => 2.2,
                (false, 3 | 4, _) => 1.7,
                (false, _, true) => 1.4,
                (false, _, false) => 1.15,
            };

            score *= match interval {
                0 => 0.32,
                1 | 2 => 2.8,
                3..=5 => 1.6,
                6 | 7 => {
                    if strong_beat {
                        0.9
                    } else {
                        0.55
                    }
                }
                8..=12 => 0.35,
                _ => 0.12,
            };

            if !is_chord_tone {
                let nearest_chord_tone = chord
                    .notes
                    .iter()
                    .map(|&pc| pitch_class_distance(note % 12, (chord.root + pc) % 12))
                    .min()
                    .unwrap_or(6);
                if nearest_chord_tone <= 2 {
                    score *= 1.35;
                }
            }

            if prefer_root {
                score *= if note % 12 == root_pc { 6.0 } else { 0.6 };
            }

            if self.history.contains(&note) {
                score *= 0.55;
            }

            candidates.push((note, score));
        }

        if candidates.is_empty() {
            return self.pick_melody_note(false, false);
        }

        self.sample_weighted(&candidates)
    }

    fn sample_weighted(&mut self, candidates: &[(u8, f32)]) -> u8 {
        if candidates.is_empty() {
            return self.tones[0];
        }

        let weights: Vec<f32> = candidates
            .iter()
            .map(|&(_, weight)| weight.max(0.001))
            .collect();
        let Ok(dist) = WeightedIndex::new(&weights) else {
            return candidates[0].0;
        };

        candidates[dist.sample(&mut self.rng)].0
    }

    fn remember(&mut self, note: u8) {
        self.history.push_back(note);
        if self.history.len() > 5 {
            self.history.pop_front();
        }
    }

    pub fn revise_last_note(&mut self, note: u8) {
        if let Some(last) = self.history.back_mut() {
            *last = note;
        }
    }
}

fn pitch_class_distance(a: u8, b: u8) -> u8 {
    let diff = (a as i16 - b as i16).rem_euclid(12) as u8;
    diff.min(12 - diff)
}

static C_MAJ9: Chord = Chord {
    root: 60,
    notes: &[0, 4, 7, 11, 2],
};

static G_9: Chord = Chord {
    root: 55,
    notes: &[0, 4, 7, 10, 2],
};

static A_MIN7_9: Chord = Chord {
    root: 45,
    notes: &[0, 3, 7, 10, 2],
};

static F_MAJ9: Chord = Chord {
    root: 65,
    notes: &[0, 4, 7, 11, 2],
};

static D_MIN9: Chord = Chord {
    root: 62,
    notes: &[0, 3, 7, 10, 2],
};

static G_13: Chord = Chord {
    root: 67,
    notes: &[0, 4, 7, 10, 2, 9],
};

static C_6_9: Chord = Chord {
    root: 60,
    notes: &[0, 4, 7, 9, 2],
};

static A_MIN11: Chord = Chord {
    root: 57,
    notes: &[0, 3, 7, 10, 2, 5],
};

pub static PRESET_NAMES: &[&str] = &["Ambient", "Jazz"];

pub fn get_preset(name: &str) -> &'static MarkovPreset {
    match name.trim().to_ascii_lowercase().as_str() {
        "jazz" | "night coffee jazz" => &JAZZ_PRESET,
        _ => &AMBIENT_PRESET,
    }
}

static AMBIENT_PRESET: MarkovPreset = MarkovPreset {
    name: "Ambient",
    scale: Scale {
        root: 60,
        intervals: &[0, 2, 4, 5, 7, 9, 11],
    },
    chords: &[&C_MAJ9, &G_9, &A_MIN7_9, &F_MAJ9],
    phrase_len: 12,
    base_step_ms: 1040.0,
    vel_min: 0.10,
    vel_max: 0.30,
    layers: &[
        LayerConfig {
            instrument: Instrument::Pad,
            role: RhythmRole::Melody,
            note_min: 62,
            note_max: 81,
            vel_scale: 0.14,
            envelope: Some(EnvelopeConfig::new(3400.0, 1200.0, 0.34, 2200.0)),
        },
        LayerConfig {
            instrument: Instrument::Piano,
            role: RhythmRole::Melody,
            note_min: 72,
            note_max: 91,
            vel_scale: 0.10,
            envelope: Some(EnvelopeConfig::new(120.0, 1300.0, 0.08, 4200.0)),
        },
        LayerConfig {
            instrument: Instrument::Triangle,
            role: RhythmRole::Melody,
            note_min: 88,
            note_max: 103,
            vel_scale: 0.075,
            envelope: Some(EnvelopeConfig::new(1.0, 1800.0, 0.0, 3000.0)),
        },
    ],
};

static JAZZ_PRESET: MarkovPreset = MarkovPreset {
    name: "Jazz",
    scale: Scale {
        root: 50,
        intervals: &[0, 2, 3, 5, 7, 9, 10],
    },
    chords: &[&D_MIN9, &G_13, &C_6_9, &A_MIN11],
    phrase_len: 12,
    base_step_ms: 380.0,
    vel_min: 0.28,
    vel_max: 0.38,
    layers: &[
        LayerConfig {
            instrument: Instrument::Piano,
            role: RhythmRole::Melody,
            note_min: 57,
            note_max: 74,
            vel_scale: 0.46,
            envelope: Some(EnvelopeConfig::new(10.0, 820.0, 0.14, 1250.0)),
        },
        LayerConfig {
            instrument: Instrument::Bass,
            role: RhythmRole::Bass,
            note_min: 33,
            note_max: 50,
            vel_scale: 0.44,
            envelope: Some(EnvelopeConfig::new(6.0, 980.0, 0.20, 620.0)),
        },
        LayerConfig {
            instrument: Instrument::Kick,
            role: RhythmRole::Bass,
            note_min: 36,
            note_max: 36,
            vel_scale: 1.0,
            envelope: Some(EnvelopeConfig::new(20.0, 250.0, 0.0, 50.0)),
        },
        LayerConfig {
            instrument: Instrument::Hihat,
            role: RhythmRole::Melody,
            note_min: 92,
            note_max: 92,
            vel_scale: 0.044,
            envelope: Some(EnvelopeConfig::new(1.0, 28.0, 0.0, 16.0)),
        },
    ],
};
