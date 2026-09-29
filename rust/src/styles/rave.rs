use super::{RoleGuide, Style, Voicing};
use crate::instruments::Slot;

/// Rave: олдскул-эйфория. Пианино-брейки, хувер-стабы, кислотный бас,
/// кричалки и безостановочная бочка. Ностальгия по складу 92-го.
const SCALE: [u8; 7] = [48, 50, 52, 53, 55, 57, 59];

const KICK_W: &[f32] = &[
    1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0,
];
const KICK_P1: &[f32] = &[
    1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.6, 0.6,
];
const KICK_POLES: &[&[f32]] = &[KICK_W, KICK_P1];

const SNARE_W: &[f32] = &[
    0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.5, 0.5,
];
const SNARE_P1: &[f32] = &[
    0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.5, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0,
];
const SNARE_POLES: &[&[f32]] = &[SNARE_W, SNARE_P1];

const HAT_W: &[f32] = &[
    0.5, 0.3, 1.0, 0.3, 0.5, 0.3, 1.0, 0.3, 0.5, 0.3, 1.0, 0.3, 0.5, 0.3, 1.0, 0.5,
];
const HAT_P1: &[f32] = &[
    0.4, 0.4, 0.4, 0.4, 1.0, 0.4, 0.4, 0.4, 0.4, 0.4, 0.4, 0.4, 1.0, 0.4, 0.4, 0.4,
];
const HAT_POLES: &[&[f32]] = &[HAT_W, HAT_P1];

// Кислота: бегущие шестнадцатые (якоря держат интро).
const BASS_W: &[f32] = &[
    1.0, 0.5, 0.6, 0.5, 0.0, 0.5, 0.6, 0.5, 1.0, 0.5, 0.6, 0.5, 0.0, 0.5, 0.6, 0.5,
];
const BASS_P1: &[f32] = &[
    0.0, 0.0, 1.0, 0.0, 0.6, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.6, 0.0, 1.0, 0.6,
];
const BASS_POLES: &[&[f32]] = &[BASS_W, BASS_P1];

const PIANO_W: &[f32] = &[
    0.7, 0.0, 0.0, 0.5, 0.0, 0.0, 0.7, 0.0, 0.0, 0.0, 0.5, 0.0, 0.0, 0.7, 0.0, 0.0,
];
const HOOVER_W: &[f32] = &[
    0.9, 0.0, 0.0, 0.7, 0.0, 0.0, 0.9, 0.0, 0.0, 0.7, 0.0, 0.0, 0.9, 0.0, 0.0, 0.0,
];
const SHOUT_W: &[f32] = &[
    0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.5, 0.0,
];
const ARP_W: &[f32] = &[
    0.5, 0.4, 0.5, 0.4, 0.5, 0.4, 0.5, 0.4, 0.5, 0.4, 0.5, 0.4, 0.5, 0.4, 0.5, 0.4,
];
const CRASH_W: &[f32] = &[
    1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
];
const BED_W: &[f32] = &[
    0.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
];

pub struct RaveStyle {}

impl RaveStyle {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for RaveStyle {
    fn default() -> Self {
        Self::new()
    }
}

impl Style for RaveStyle {
    fn name(&self) -> &str {
        "rave"
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
        0.15
    }

    fn reverb_size(&self) -> f32 {
        0.5
    }

    fn treble_cutoff(&self) -> f32 {
        18500.0
    }

    /// Форма энергии: эйфория держится почти весь круг.
    fn shape_energy(&self, energy: f32) -> f32 {
        0.4 + 0.6 * energy
    }

    fn ensemble(&self) -> &'static [Slot] {
        // Склад: бочка, кислота, пианино, хувер, кричалки и крэш.
        // Без 808, винила и флейты.
        &[
            Slot::Bass,
            Slot::Kick,
            Slot::Snare,
            Slot::Hat,
            Slot::Pluck,
            Slot::Keys,
            Slot::Festival,
            Slot::Vocal,
            Slot::Crash,
        ]
    }

    fn slot_guides(&self) -> [RoleGuide; Slot::COUNT] {
        let bass = RoleGuide {
            energy_response: 1.6,
            duration_mult: 0.4,
            vel_base: 0.7,
            vel_energy: 0.2,
            vel_jitter: 0.06,
            voice: Voicing::Scale {
                degree: 0,
                octave: -1,
                jitter: 1,
            },
            top_pitch: 52,
            vel_high_cut: 0.6,
            poles: Some(BASS_POLES),
            bass_fifth: true,
            ..RoleGuide::pitched(BASS_W, 0, -1, 1, 0.4, 0.7)
        };
        let kick = RoleGuide {
            climax_boost: 0.6,
            vel_base: 0.95,
            vel_energy: 0.05,
            poles: Some(KICK_POLES),
            ..RoleGuide::kit(KICK_W, 36, 0.95)
        };
        let pad = RoleGuide {
            duration_mult: 8.0,
            vel_base: 0.4,
            vel_jitter: 0.0,
            ..RoleGuide::pitched(BED_W, 0, 0, 0, 8.0, 0.4)
        };
        let snare = RoleGuide {
            climax_boost: 0.7,
            vel_base: 0.85,
            poles: Some(SNARE_POLES),
            ..RoleGuide::kit(SNARE_W, 38, 0.85)
        };
        let hat = RoleGuide {
            energy_response: 1.2,
            long_offbeats: true,
            long_mult: 4.0,
            duration_mult: 0.4,
            vel_base: 0.4,
            vel_energy: 0.25,
            vel_jitter: 0.08,
            poles: Some(HAT_POLES),
            ..RoleGuide::kit(HAT_W, 42, 0.4)
        };
        let pluck = RoleGuide {
            energy_response: 1.3,
            duration_mult: 0.6,
            vel_base: 0.4,
            vel_energy: 0.2,
            vel_jitter: 0.07,
            voice: Voicing::Scale {
                degree: 0,
                octave: 2,
                jitter: 0,
            },
            follow_motif: true,
            ..RoleGuide::pitched(ARP_W, 0, 2, 0, 0.6, 0.4)
        };
        // Пианино-брейк: аккорды синкопами.
        let keys = RoleGuide {
            energy_response: 1.6,
            duration_mult: 2.0,
            vel_base: 0.55,
            vel_jitter: 0.07,
            voice: Voicing::Scale {
                degree: 0,
                octave: 1,
                jitter: 0,
            },
            ..RoleGuide::pitched(PIANO_W, 0, 1, 0, 2.0, 0.55)
        };
        // Чужие слоты: сдержанные направляющие для ручек фронта.
        let kick808 = RoleGuide {
            vel_base: 0.8,
            ..RoleGuide::kit(KICK_W, 36, 0.8)
        };
        let laser = RoleGuide {
            energy_response: 1.2,
            duration_mult: 0.8,
            vel_base: 0.4,
            voice: Voicing::Scale {
                degree: 0,
                octave: 2,
                jitter: 2,
            },
            ..RoleGuide::pitched(ARP_W, 0, 2, 2, 0.8, 0.4)
        };
        let vinyl = RoleGuide {
            duration_mult: 200.0,
            vel_base: 0.3,
            ..RoleGuide::pitched(BED_W, 0, 0, 0, 200.0, 0.3)
        };
        // Кричалки толпы.
        let vocal = RoleGuide {
            energy_response: 1.5,
            duration_mult: 1.0,
            vel_base: 0.5,
            vel_energy: 0.2,
            vel_jitter: 0.08,
            voice: Voicing::Scale {
                degree: 0,
                octave: 2,
                jitter: 1,
            },
            ..RoleGuide::pitched(SHOUT_W, 0, 2, 1, 1.0, 0.5)
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
            ..RoleGuide::pitched(ARP_W, 0, 1, 2, 4.0, 0.3)
        };
        // Хувер-стабы: короткие злые аккорды.
        let festival = RoleGuide {
            energy_response: 2.0,
            duration_mult: 0.7,
            vel_base: 0.55,
            vel_energy: 0.2,
            vel_jitter: 0.06,
            voice: Voicing::Scale {
                degree: 0,
                octave: 1,
                jitter: 0,
            },
            ..RoleGuide::pitched(HOOVER_W, 0, 1, 0, 0.7, 0.55)
        };
        let crash = RoleGuide {
            climax_only: true,
            duration_mult: 12.0,
            vel_base: 0.75,
            vel_jitter: 0.05,
            ..RoleGuide::kit(CRASH_W, 49, 0.75)
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
            ..RoleGuide::pitched(ARP_W, 0, 1, 2, 1.2, 0.3)
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
