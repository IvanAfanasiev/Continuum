use super::{RoleGuide, Style, Voicing};
use crate::instruments::Slot;

/// Drum'n'bass: брейк на скорости. Синкопированная бочка, снейр
/// ту-степ, быстрые хэты, глубокий катящийся саб и темные стабы.
/// Нервно, упруго, вперед.
const SCALE: [u8; 7] = [48, 50, 51, 53, 55, 56, 58];

const KICK_W: &[f32] = &[
    1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.8, 0.0, 0.0, 0.0, 0.0, 0.0,
];
const KICK_P1: &[f32] = &[
    1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.7, 0.0, 0.0, 0.8, 0.0, 0.0, 0.0, 0.0, 0.0,
];
const KICK_P2: &[f32] = &[
    1.0, 0.0, 0.0, 0.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.8, 0.0, 0.0, 0.5, 0.0, 0.0,
];
const KICK_POLES: &[&[f32]] = &[KICK_W, KICK_P1, KICK_P2];

const SNARE_W: &[f32] = &[
    0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0,
];
const SNARE_P1: &[f32] = &[
    0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.4, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.4, 0.0,
];
const SNARE_POLES: &[&[f32]] = &[SNARE_W, SNARE_P1];

const HAT_W: &[f32] = &[
    0.5, 0.4, 0.5, 0.4, 0.5, 0.4, 0.5, 0.4, 0.5, 0.4, 0.5, 0.4, 0.5, 0.4, 0.5, 0.4,
];
const HAT_P1: &[f32] = &[
    0.6, 0.0, 0.4, 0.0, 0.6, 0.4, 0.4, 0.0, 0.6, 0.0, 0.4, 0.0, 0.6, 0.4, 0.8, 0.0,
];
const HAT_POLES: &[&[f32]] = &[HAT_W, HAT_P1];

const BASS_W: &[f32] = &[
    1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
];
const BASS_P1: &[f32] = &[
    1.0, 0.0, 0.0, 0.6, 0.0, 0.0, 0.0, 0.0, 0.0, 0.6, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0,
];
const BASS_POLES: &[&[f32]] = &[BASS_W, BASS_P1];

const STAB_W: &[f32] = &[
    0.0, 0.0, 0.6, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.6, 0.0, 0.0, 0.0, 0.0, 0.0,
];
const DARK_W: &[f32] = &[
    1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
];
const CHOP_W: &[f32] = &[
    0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.5, 0.0,
];
const CRASH_W: &[f32] = &[
    1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
];
const BED_W: &[f32] = &[
    0.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
];

pub struct DnbStyle {}

impl DnbStyle {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for DnbStyle {
    fn default() -> Self {
        Self::new()
    }
}

impl Style for DnbStyle {
    fn name(&self) -> &str {
        "dnb"
    }

    fn bpm(&self) -> f32 {
        174.0
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
        0.15
    }

    fn reverb_size(&self) -> f32 {
        0.5
    }

    fn treble_cutoff(&self) -> f32 {
        18000.0
    }

    /// Форма энергии: ровное давление, без провалов и взрывов.
    fn shape_energy(&self, energy: f32) -> f32 {
        0.5 + 0.5 * energy
    }

    fn ensemble(&self) -> &'static [Slot] {
        // Брейк: синкопа бочки, ту-степ снейра, саб, стабы, чопы.
        // Без 808, лазера и фестиваля.
        &[
            Slot::Bass,
            Slot::Kick,
            Slot::Snare,
            Slot::Hat,
            Slot::Pad,
            Slot::Keys,
            Slot::Vocal,
            Slot::Crash,
        ]
    }

    fn slot_guides(&self) -> [RoleGuide; Slot::COUNT] {
        // Катящийся саб: длинные ноты за брейком.
        let bass = RoleGuide {
            energy_response: 1.5,
            duration_mult: 2.0,
            vel_base: 0.75,
            vel_energy: 0.2,
            vel_jitter: 0.05,
            voice: Voicing::Scale {
                degree: 0,
                octave: -1,
                jitter: 0,
            },
            top_pitch: 52,
            vel_high_cut: 0.6,
            poles: Some(BASS_POLES),
            ..RoleGuide::pitched(BASS_W, 0, -1, 0, 2.0, 0.75)
        };
        let kick = RoleGuide {
            climax_boost: 0.6,
            vel_base: 0.9,
            vel_energy: 0.05,
            poles: Some(KICK_POLES),
            ..RoleGuide::kit(KICK_W, 36, 0.9)
        };
        let pad = RoleGuide {
            duration_mult: 12.0,
            vel_base: 0.35,
            vel_jitter: 0.0,
            ..RoleGuide::pitched(DARK_W, 0, 0, 0, 12.0, 0.35)
        };
        let snare = RoleGuide {
            climax_boost: 0.7,
            vel_base: 0.85,
            poles: Some(SNARE_POLES),
            ..RoleGuide::kit(SNARE_W, 38, 0.85)
        };
        let hat = RoleGuide {
            energy_response: 1.1,
            duration_mult: 0.25,
            vel_base: 0.35,
            vel_energy: 0.2,
            vel_jitter: 0.08,
            poles: Some(HAT_POLES),
            ..RoleGuide::kit(HAT_W, 42, 0.35)
        };
        let pluck = RoleGuide {
            energy_response: 1.2,
            duration_mult: 0.6,
            vel_base: 0.35,
            voice: Voicing::Scale {
                degree: 0,
                octave: 1,
                jitter: 2,
            },
            ..RoleGuide::pitched(CHOP_W, 0, 1, 2, 0.6, 0.35)
        };
        // Темные стабы между ударами.
        let keys = RoleGuide {
            energy_response: 1.8,
            duration_mult: 1.2,
            vel_base: 0.5,
            vel_jitter: 0.06,
            voice: Voicing::Scale {
                degree: 4,
                octave: 1,
                jitter: 0,
            },
            ..RoleGuide::pitched(STAB_W, 4, 1, 0, 1.2, 0.5)
        };
        // Чужие слоты: сдержанные направляющие для ручек фронта.
        let kick808 = RoleGuide {
            vel_base: 0.75,
            ..RoleGuide::kit(KICK_W, 36, 0.75)
        };
        let laser = RoleGuide {
            energy_response: 1.0,
            duration_mult: 0.8,
            vel_base: 0.35,
            voice: Voicing::Scale {
                degree: 0,
                octave: 2,
                jitter: 2,
            },
            ..RoleGuide::pitched(CHOP_W, 0, 2, 2, 0.8, 0.35)
        };
        let vinyl = RoleGuide {
            duration_mult: 200.0,
            vel_base: 0.3,
            ..RoleGuide::pitched(BED_W, 0, 0, 0, 200.0, 0.3)
        };
        let vocal = RoleGuide {
            energy_response: 1.4,
            duration_mult: 1.2,
            vel_base: 0.45,
            vel_energy: 0.15,
            vel_jitter: 0.08,
            voice: Voicing::Scale {
                degree: 2,
                octave: 1,
                jitter: 2,
            },
            ..RoleGuide::pitched(CHOP_W, 2, 1, 2, 1.2, 0.45)
        };
        let flute = RoleGuide {
            energy_response: 1.0,
            duration_mult: 4.0,
            vel_base: 0.3,
            voice: Voicing::Scale {
                degree: 0,
                octave: 1,
                jitter: 2,
            },
            ..RoleGuide::pitched(CHOP_W, 0, 1, 2, 4.0, 0.3)
        };
        let festival = RoleGuide {
            energy_response: 1.2,
            duration_mult: 0.8,
            vel_base: 0.35,
            vel_energy: 0.2,
            vel_jitter: 0.06,
            voice: Voicing::Scale {
                degree: 0,
                octave: 2,
                jitter: 2,
            },
            ..RoleGuide::pitched(CHOP_W, 0, 2, 2, 0.8, 0.35)
        };
        let crash = RoleGuide {
            climax_only: true,
            duration_mult: 10.0,
            vel_base: 0.7,
            vel_jitter: 0.05,
            ..RoleGuide::kit(CRASH_W, 49, 0.7)
        };
        let guitar = RoleGuide {
            energy_response: 1.0,
            duration_mult: 1.2,
            vel_base: 0.3,
            voice: Voicing::Scale {
                degree: 0,
                octave: 1,
                jitter: 2,
            },
            ..RoleGuide::pitched(CHOP_W, 0, 1, 2, 1.2, 0.3)
        };
        let rain = RoleGuide {
            duration_mult: 150.0,
            vel_base: 0.25,
            ..RoleGuide::pitched(BED_W, 0, 0, 0, 150.0, 0.25)
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
