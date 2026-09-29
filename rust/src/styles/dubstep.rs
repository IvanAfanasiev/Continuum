use super::{RoleGuide, Style, Voicing};
use crate::instruments::Slot;

/// Dubstep: халфтайм-тяжесть. Разреженная бочка, снейр на третьей доле,
/// вобл-бас восьмыми и саб-дропы на 808. Темно, широко, медленно качает.
const SCALE: [u8; 7] = [48, 50, 51, 53, 55, 56, 58];

const KICK_W: &[f32] = &[
    1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.8, 0.0, 0.0, 0.0, 0.0, 0.0,
];
const KICK_P1: &[f32] = &[
    1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.7, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
];
const KICK_P2: &[f32] = &[
    1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.8, 0.0, 0.0, 0.0, 0.8, 0.0, 0.0, 0.0, 0.0, 0.0,
];
const KICK_POLES: &[&[f32]] = &[KICK_W, KICK_P1, KICK_P2];

const SNARE_W: &[f32] = &[
    0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
];
const SNARE_P1: &[f32] = &[
    0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.5,
];
const SNARE_POLES: &[&[f32]] = &[SNARE_W, SNARE_P1];

const HAT_W: &[f32] = &[
    0.5, 0.0, 0.4, 0.0, 0.5, 0.0, 0.4, 0.3, 0.5, 0.0, 0.4, 0.0, 0.5, 0.0, 0.4, 0.3,
];
const HAT_P1: &[f32] = &[
    0.6, 0.3, 0.6, 0.3, 0.6, 0.3, 0.6, 0.3, 0.6, 0.3, 0.6, 0.3, 0.6, 0.3, 0.6, 0.3,
];
const HAT_POLES: &[&[f32]] = &[HAT_W, HAT_P1];

// Вобл: ворота восьмыми, короткие ноты (якоря держат интро).
const BASS_W: &[f32] = &[
    1.0, 0.0, 0.7, 0.0, 0.8, 0.0, 0.7, 0.0, 1.0, 0.0, 0.7, 0.0, 0.8, 0.0, 0.7, 0.5,
];
const BASS_P1: &[f32] = &[
    0.0, 0.0, 1.0, 0.0, 0.0, 0.8, 0.0, 0.0, 1.0, 0.0, 0.0, 0.8, 0.0, 0.0, 1.0, 0.0,
];
const BASS_P2: &[f32] = &[
    1.0, 0.0, 0.0, 0.6, 0.0, 0.0, 1.0, 0.0, 0.0, 0.6, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0,
];
const BASS_POLES: &[&[f32]] = &[BASS_W, BASS_P1, BASS_P2];

const SUB_W: &[f32] = &[
    1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.8, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
];
const DARK_W: &[f32] = &[
    1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
];
const FX_W: &[f32] = &[
    0.0, 0.0, 0.0, 0.0, 0.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.5, 0.0, 0.0, 0.0,
];
const CHOP_W: &[f32] = &[
    0.0, 0.0, 0.4, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.4, 0.0, 0.0, 0.0, 0.0, 0.0,
];
const CRASH_W: &[f32] = &[
    1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
];
const BED_W: &[f32] = &[
    0.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
];

pub struct DubstepStyle {}

impl DubstepStyle {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for DubstepStyle {
    fn default() -> Self {
        Self::new()
    }
}

impl Style for DubstepStyle {
    fn name(&self) -> &str {
        "dubstep"
    }

    fn bpm(&self) -> f32 {
        140.0
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
        0.2
    }

    fn reverb_size(&self) -> f32 {
        0.65
    }

    fn treble_cutoff(&self) -> f32 {
        17500.0
    }

    /// Сайдчейн давит заметно: микс качает под халфтайм.
    fn sidechain_depth(&self) -> f32 {
        1.2
    }

    /// Форма энергии: тяжелый провал и удар (халфтайм-качели).
    fn shape_energy(&self, energy: f32) -> f32 {
        0.35 + 0.65 * energy
    }

    fn ensemble(&self) -> &'static [Slot] {
        // Халфтайм: разреженная бочка, снейр на три, вобл, саб, тьма.
        // Без Rhodes и флейты.
        &[
            Slot::Bass,
            Slot::Kick,
            Slot::Kick808,
            Slot::Snare,
            Slot::Hat,
            Slot::Pad,
            Slot::Laser,
            Slot::Vocal,
            Slot::Crash,
        ]
    }

    fn slot_guides(&self) -> [RoleGuide; Slot::COUNT] {
        let bass = RoleGuide {
            energy_response: 1.8,
            duration_mult: 0.45,
            vel_base: 0.75,
            vel_energy: 0.2,
            vel_jitter: 0.05,
            voice: Voicing::Scale {
                degree: 0,
                octave: -1,
                jitter: 0,
            },
            octave_up: 0.4,
            top_pitch: 52,
            vel_high_cut: 0.6,
            poles: Some(BASS_POLES),
            ..RoleGuide::pitched(BASS_W, 0, -1, 0, 0.45, 0.75)
        };
        let kick = RoleGuide {
            climax_boost: 0.7,
            vel_base: 0.95,
            vel_energy: 0.05,
            poles: Some(KICK_POLES),
            ..RoleGuide::kit(KICK_W, 36, 0.95)
        };
        let pad = RoleGuide {
            duration_mult: 12.0,
            vel_base: 0.4,
            vel_jitter: 0.0,
            ..RoleGuide::pitched(DARK_W, 0, 0, 0, 12.0, 0.4)
        };
        let snare = RoleGuide {
            climax_boost: 0.6,
            vel_base: 0.9,
            poles: Some(SNARE_POLES),
            ..RoleGuide::kit(SNARE_W, 38, 0.9)
        };
        let hat = RoleGuide {
            energy_response: 1.1,
            duration_mult: 0.4,
            vel_base: 0.38,
            vel_energy: 0.2,
            vel_jitter: 0.08,
            poles: Some(HAT_POLES),
            ..RoleGuide::kit(HAT_W, 42, 0.38)
        };
        let pluck = RoleGuide {
            energy_response: 1.2,
            duration_mult: 0.8,
            vel_base: 0.35,
            voice: Voicing::Scale {
                degree: 0,
                octave: 1,
                jitter: 2,
            },
            ..RoleGuide::pitched(FX_W, 0, 1, 2, 0.8, 0.35)
        };
        let keys = RoleGuide {
            energy_response: 1.4,
            duration_mult: 2.0,
            vel_base: 0.45,
            vel_jitter: 0.06,
            voice: Voicing::Scale {
                degree: 4,
                octave: 1,
                jitter: 0,
            },
            ..RoleGuide::pitched(FX_W, 4, 1, 0, 2.0, 0.45)
        };
        // Саб-дропы на 808: длинные, за баслайном.
        let kick808 = RoleGuide {
            climax_boost: 0.4,
            vel_base: 0.9,
            vel_energy: 0.05,
            voice: Voicing::Scale {
                degree: 0,
                octave: -1,
                jitter: 0,
            },
            ..RoleGuide::pitched(SUB_W, 0, -1, 0, 2.0, 0.9)
        };
        // Свисты-райзеры вместо лида.
        let laser = RoleGuide {
            energy_response: 1.6,
            duration_mult: 1.2,
            vel_base: 0.4,
            vel_energy: 0.2,
            vel_jitter: 0.07,
            voice: Voicing::Scale {
                degree: 0,
                octave: 2,
                jitter: 2,
            },
            ..RoleGuide::pitched(FX_W, 0, 2, 2, 1.2, 0.4)
        };
        let vinyl = RoleGuide {
            duration_mult: 200.0,
            vel_base: 0.3,
            ..RoleGuide::pitched(BED_W, 0, 0, 0, 200.0, 0.3)
        };
        let vocal = RoleGuide {
            energy_response: 1.3,
            duration_mult: 1.6,
            vel_base: 0.45,
            vel_energy: 0.15,
            vel_jitter: 0.08,
            voice: Voicing::Scale {
                degree: 2,
                octave: 1,
                jitter: 2,
            },
            ..RoleGuide::pitched(CHOP_W, 2, 1, 2, 1.6, 0.45)
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
            ..RoleGuide::pitched(FX_W, 0, 1, 2, 4.0, 0.3)
        };
        let festival = RoleGuide {
            energy_response: 1.4,
            duration_mult: 0.8,
            vel_base: 0.4,
            vel_energy: 0.2,
            vel_jitter: 0.06,
            voice: Voicing::Scale {
                degree: 0,
                octave: 2,
                jitter: 2,
            },
            ..RoleGuide::pitched(FX_W, 0, 2, 2, 0.8, 0.4)
        };
        let crash = RoleGuide {
            climax_only: true,
            duration_mult: 14.0,
            vel_base: 0.8,
            vel_jitter: 0.05,
            ..RoleGuide::kit(CRASH_W, 49, 0.8)
        };
        let guitar = RoleGuide {
            energy_response: 1.0,
            duration_mult: 1.5,
            vel_base: 0.3,
            voice: Voicing::Scale {
                degree: 0,
                octave: 1,
                jitter: 2,
            },
            ..RoleGuide::pitched(FX_W, 0, 1, 2, 1.5, 0.3)
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
