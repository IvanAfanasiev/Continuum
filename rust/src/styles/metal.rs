use super::{RoleGuide, Style, StyleGroup, Voicing};
use crate::instruments::Slot;

/// Metal: скорость и вес. Дабл-бочка очередями, галоп-бас, пулеметные
/// чуг-гитары и крэши как удары молота. Никакой нежности.
const SCALE: [u8; 5] = [48, 51, 53, 55, 58];

const KICK_W: &[f32] = &[
    1.0, 0.0, 0.8, 0.0, 1.0, 0.0, 0.8, 0.0, 1.0, 0.0, 0.8, 0.0, 1.0, 0.0, 0.8, 0.5,
];
const KICK_P1: &[f32] = &[
    1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.9, 0.9, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.9, 0.9,
];
const KICK_P2: &[f32] = &[
    1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.8, 0.8, 0.8, 0.0, 0.0, 0.0, 0.0,
];
const KICK_POLES: &[&[f32]] = &[KICK_W, KICK_P1, KICK_P2];

const SNARE_W: &[f32] = &[
    0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0,
];
const SNARE_P1: &[f32] = &[
    0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.5, 0.5, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.5, 0.5,
];
const SNARE_POLES: &[&[f32]] = &[SNARE_W, SNARE_P1];

const HAT_W: &[f32] = &[
    0.6, 0.4, 0.6, 0.4, 0.6, 0.4, 0.6, 0.4, 0.6, 0.4, 0.6, 0.4, 0.6, 0.4, 0.6, 0.4,
];
const HAT_P1: &[f32] = &[
    0.7, 0.0, 0.5, 0.0, 0.7, 0.0, 0.5, 0.0, 0.7, 0.0, 0.5, 0.3, 0.7, 0.0, 0.5, 0.0,
];
const HAT_POLES: &[&[f32]] = &[HAT_W, HAT_P1];

// Галоп: восьмая + две шестнадцатые (якоря держат интро).
const BASS_W: &[f32] = &[
    1.0, 0.0, 0.0, 0.7, 0.0, 0.7, 1.0, 0.0, 0.0, 0.7, 0.0, 0.7, 1.0, 0.0, 0.0, 0.7,
];
const BASS_P1: &[f32] = &[
    1.0, 0.0, 0.6, 0.0, 1.0, 0.0, 0.6, 0.0, 1.0, 0.0, 0.6, 0.0, 1.0, 0.0, 0.6, 0.6,
];
const BASS_POLES: &[&[f32]] = &[BASS_W, BASS_P1];

// Пулемет: глухие шестнадцатые с открытыми акцентами.
const GUN_W: &[f32] = &[
    0.7, 0.5, 0.7, 0.5, 0.7, 0.5, 0.7, 0.5, 0.7, 0.5, 0.7, 0.5, 0.7, 0.5, 0.9, 0.5,
];
const GUN_P1: &[f32] = &[
    0.8, 0.0, 0.5, 0.0, 0.8, 0.0, 0.5, 0.5, 0.8, 0.0, 0.5, 0.0, 0.8, 0.0, 0.9, 0.0,
];
const GUN_POLES: &[&[f32]] = &[GUN_W, GUN_P1];

const CRASH_W: &[f32] = &[
    1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
];
const BED_W: &[f32] = &[
    0.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
];

pub struct MetalStyle {}

impl MetalStyle {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for MetalStyle {
    fn default() -> Self {
        Self::new()
    }
}

impl Style for MetalStyle {
    fn name(&self) -> &str {
        "metal"
    }

    fn group(&self) -> StyleGroup {
        StyleGroup::Rock
    }

    fn bpm(&self) -> f32 {
        150.0
    }

    fn steps_per_measure(&self) -> usize {
        16
    }

    fn swing_ratio(&self) -> f32 {
        1.0
    }

    fn scale(&self) -> &'static [u8] {
        &SCALE
    }

    fn reverb_wet(&self) -> f32 {
        0.08
    }

    fn reverb_size(&self) -> f32 {
        0.3
    }

    fn treble_cutoff(&self) -> f32 {
        19000.0
    }

    /// Сайдчейн давит сильно: натиск качает под дабл-бочку.
    fn sidechain_depth(&self) -> f32 {
        1.25
    }

    /// Форма энергии: натиск без передышек, дроп — лишь пик стены.
    fn shape_energy(&self, energy: f32) -> f32 {
        0.65 + 0.35 * energy
    }

    fn ensemble(&self) -> &'static [Slot] {
        // Натиск: дабл-бочка, галоп-бас, пулемет, снейр, хэт и крэш.
        &[
            Slot::Bass,
            Slot::Kick,
            Slot::Snare,
            Slot::Hat,
            Slot::Guitar,
            Slot::Crash,
        ]
    }

    fn slot_guides(&self) -> [RoleGuide; Slot::COUNT] {
        let bass = RoleGuide {
            energy_response: 1.2,
            duration_mult: 0.4,
            vel_base: 0.8,
            vel_energy: 0.15,
            vel_jitter: 0.04,
            voice: Voicing::Scale {
                degree: 0,
                octave: -1,
                jitter: 0,
            },
            top_pitch: 52,
            vel_high_cut: 0.6,
            poles: Some(BASS_POLES),
            bass_fifth: true,
            ..RoleGuide::pitched(BASS_W, 0, -1, 0, 0.4, 0.8)
        };
        let kick = RoleGuide {
            climax_boost: 0.5,
            vel_base: 0.95,
            vel_energy: 0.05,
            poles: Some(KICK_POLES),
            ..RoleGuide::kit(KICK_W, 36, 0.95)
        };
        let pad = RoleGuide {
            duration_mult: 8.0,
            vel_base: 0.2,
            ..RoleGuide::pitched(BED_W, 0, 0, 0, 8.0, 0.2)
        };
        let snare = RoleGuide {
            climax_boost: 0.8,
            vel_base: 0.9,
            poles: Some(SNARE_POLES),
            ..RoleGuide::kit(SNARE_W, 38, 0.9)
        };
        let hat = RoleGuide {
            energy_response: 1.0,
            duration_mult: 0.3,
            vel_base: 0.4,
            vel_energy: 0.15,
            vel_jitter: 0.06,
            poles: Some(HAT_POLES),
            ..RoleGuide::kit(HAT_W, 42, 0.4)
        };
        let pluck = RoleGuide {
            energy_response: 1.0,
            duration_mult: 0.6,
            vel_base: 0.25,
            voice: Voicing::Scale {
                degree: 0,
                octave: 1,
                jitter: 2,
            },
            ..RoleGuide::pitched(GUN_W, 0, 1, 2, 0.6, 0.25)
        };
        let keys = RoleGuide {
            energy_response: 1.0,
            duration_mult: 3.0,
            vel_base: 0.25,
            voice: Voicing::Scale {
                degree: 0,
                octave: 1,
                jitter: 0,
            },
            ..RoleGuide::pitched(BED_W, 0, 1, 0, 3.0, 0.25)
        };
        // Чужие слоты: сдержанные направляющие для ручек фронта.
        let kick808 = RoleGuide {
            vel_base: 0.7,
            ..RoleGuide::kit(KICK_W, 36, 0.7)
        };
        let laser = RoleGuide {
            energy_response: 1.0,
            duration_mult: 0.6,
            vel_base: 0.25,
            voice: Voicing::Scale {
                degree: 0,
                octave: 2,
                jitter: 2,
            },
            ..RoleGuide::pitched(GUN_W, 0, 2, 2, 0.6, 0.25)
        };
        let vinyl = RoleGuide {
            duration_mult: 200.0,
            vel_base: 0.2,
            ..RoleGuide::pitched(BED_W, 0, 0, 0, 200.0, 0.2)
        };
        let vocal = RoleGuide {
            energy_response: 1.0,
            duration_mult: 1.2,
            vel_base: 0.28,
            voice: Voicing::Scale {
                degree: 0,
                octave: 1,
                jitter: 2,
            },
            ..RoleGuide::pitched(GUN_W, 0, 1, 2, 1.2, 0.28)
        };
        let flute = RoleGuide {
            energy_response: 1.0,
            duration_mult: 4.0,
            vel_base: 0.22,
            voice: Voicing::Scale {
                degree: 0,
                octave: 1,
                jitter: 2,
            },
            ..RoleGuide::pitched(GUN_W, 0, 1, 2, 4.0, 0.22)
        };
        let festival = RoleGuide {
            energy_response: 1.2,
            duration_mult: 0.6,
            vel_base: 0.28,
            vel_jitter: 0.06,
            voice: Voicing::Scale {
                degree: 0,
                octave: 1,
                jitter: 0,
            },
            follow_motif: true,
            ..RoleGuide::pitched(GUN_W, 0, 1, 0, 0.6, 0.28)
        };
        let crash = RoleGuide {
            climax_only: true,
            duration_mult: 10.0,
            vel_base: 0.85,
            vel_jitter: 0.05,
            ..RoleGuide::kit(CRASH_W, 49, 0.85)
        };
        // Пулемет: глухие шестнадцатые, акценты открываются.
        let guitar = RoleGuide {
            energy_response: 1.4,
            duration_mult: 0.3,
            vel_base: 0.48,
            vel_energy: 0.2,
            vel_jitter: 0.09,
            voice: Voicing::Scale {
                degree: 0,
                octave: 0,
                jitter: 1,
            },
            poles: Some(GUN_POLES),
            ..RoleGuide::pitched(GUN_W, 0, 0, 1, 0.3, 0.48)
        };
        let rain = RoleGuide {
            duration_mult: 150.0,
            vel_base: 0.2,
            ..RoleGuide::pitched(BED_W, 0, 0, 0, 150.0, 0.2)
        };
        // Фазз вне рок-полки: тихий редкий рифф для ручки фронта.
        let fuzz = RoleGuide {
            energy_response: 1.0,
            duration_mult: 1.5,
            vel_base: 0.35,
            voice: Voicing::Scale {
                degree: 0,
                octave: 1,
                jitter: 2,
            },
            ..RoleGuide::pitched(BED_W, 0, 1, 2, 1.5, 0.35)
        };
        // Кислота вне club: тихий сквелч для ручки фронта.
        let acid = RoleGuide {
            energy_response: 1.0,
            duration_mult: 1.0,
            vel_base: 0.3,
            voice: Voicing::Scale {
                degree: 0,
                octave: 0,
                jitter: 1,
            },
            ..RoleGuide::pitched(BED_W, 0, 0, 1, 1.0, 0.3)
        };
        [
            bass, kick, pad, snare, hat, pluck, keys, kick808, laser, vinyl, vocal, flute,
            festival, crash, guitar, rain, fuzz, acid,
        ]
    }
}
