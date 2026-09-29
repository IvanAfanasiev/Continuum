use super::{RoleGuide, Style, Voicing};
use crate::instruments::Slot;

/// Electro-house: грязная пила и большие хуки. Панч-бочка со слоем 808,
/// агрессивный оффбитный бас с октавными прыжками, давящие стабы
/// и фестивальный лид-гимн. Жирно и в лицо.
const SCALE: [u8; 7] = [48, 50, 51, 53, 55, 56, 58];

const KICK_W: &[f32] = &[
    1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0,
];
const KICK_P1: &[f32] = &[
    1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.7, 0.0,
];
const KICK_POLES: &[&[f32]] = &[KICK_W, KICK_P1];

const CLAP_W: &[f32] = &[
    0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.3, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.3,
];
const CLAP_P1: &[f32] = &[
    0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.6,
];
const CLAP_POLES: &[&[f32]] = &[CLAP_W, CLAP_P1];

const HAT_W: &[f32] = &[
    0.4, 0.3, 1.0, 0.3, 0.4, 0.3, 1.0, 0.3, 0.4, 0.3, 1.0, 0.3, 0.4, 0.3, 1.0, 0.3,
];
const HAT_P1: &[f32] = &[
    0.5, 0.5, 1.0, 0.5, 0.5, 0.5, 1.0, 0.5, 0.5, 0.0, 1.0, 0.5, 0.5, 0.5, 1.0, 0.5,
];
const HAT_POLES: &[&[f32]] = &[HAT_W, HAT_P1];

// Грязная пила: оффбит с октавными выпрыгиваниями (якоря держат интро).
const BASS_W: &[f32] = &[
    0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.4,
];
const BASS_P1: &[f32] = &[
    0.7, 0.0, 1.0, 0.0, 0.0, 0.7, 1.0, 0.0, 0.7, 0.0, 1.0, 0.0, 0.0, 0.7, 1.0, 0.0,
];
const BASS_POLES: &[&[f32]] = &[BASS_W, BASS_P1];

const STAB_W: &[f32] = &[
    0.0, 0.0, 0.8, 0.0, 0.0, 0.0, 0.8, 0.0, 0.0, 0.0, 0.8, 0.0, 0.0, 0.0, 0.8, 0.0,
];
const HOOK_W: &[f32] = &[
    1.0, 0.3, 0.6, 0.3, 0.6, 0.3, 0.6, 0.3, 0.6, 0.3, 0.6, 0.3, 0.6, 0.3, 0.7, 0.3,
];
const ARP_W: &[f32] = &[
    0.5, 0.4, 0.5, 0.4, 0.5, 0.4, 0.5, 0.4, 0.5, 0.4, 0.5, 0.4, 0.5, 0.4, 0.5, 0.4,
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

pub struct ElectroStyle {}

impl ElectroStyle {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for ElectroStyle {
    fn default() -> Self {
        Self::new()
    }
}

impl Style for ElectroStyle {
    fn name(&self) -> &str {
        "electro"
    }

    fn bpm(&self) -> f32 {
        128.0
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
        0.1
    }

    fn reverb_size(&self) -> f32 {
        0.35
    }

    fn treble_cutoff(&self) -> f32 {
        19000.0
    }

    /// Форма энергии: жирный кач от куплета к дропу.
    fn shape_energy(&self, energy: f32) -> f32 {
        0.2 + 0.8 * energy
    }

    fn ensemble(&self) -> &'static [Slot] {
        // Жир: панч+808, пила-бас, стабы, хук, вокал и крэш.
        // Без винила, флейты и дождя.
        &[
            Slot::Bass,
            Slot::Kick,
            Slot::Snare,
            Slot::Hat,
            Slot::Pluck,
            Slot::Keys,
            Slot::Kick808,
            Slot::Festival,
            Slot::Vocal,
            Slot::Crash,
        ]
    }

    fn slot_guides(&self) -> [RoleGuide; Slot::COUNT] {
        let bass = RoleGuide {
            energy_response: 2.0,
            duration_mult: 0.8,
            vel_base: 0.75,
            vel_energy: 0.2,
            vel_jitter: 0.06,
            voice: Voicing::Scale {
                degree: 0,
                octave: -1,
                jitter: 0,
            },
            octave_up: 0.5,
            top_pitch: 52,
            vel_high_cut: 0.6,
            poles: Some(BASS_POLES),
            bass_fifth: true,
            ..RoleGuide::pitched(BASS_W, 0, -1, 0, 0.8, 0.75)
        };
        let kick = RoleGuide {
            climax_boost: 0.7,
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
            vel_base: 0.9,
            poles: Some(CLAP_POLES),
            ..RoleGuide::kit(CLAP_W, 38, 0.9)
        };
        let hat = RoleGuide {
            energy_response: 1.2,
            long_offbeats: true,
            long_mult: 4.0,
            duration_mult: 0.4,
            vel_base: 0.42,
            vel_energy: 0.25,
            vel_jitter: 0.08,
            poles: Some(HAT_POLES),
            ..RoleGuide::kit(HAT_W, 42, 0.42)
        };
        let pluck = RoleGuide {
            energy_response: 1.5,
            duration_mult: 0.7,
            vel_base: 0.45,
            vel_energy: 0.25,
            vel_jitter: 0.07,
            voice: Voicing::Scale {
                degree: 0,
                octave: 2,
                jitter: 0,
            },
            follow_motif: true,
            ..RoleGuide::pitched(ARP_W, 0, 2, 0, 0.7, 0.45)
        };
        // Давящие стабы.
        let keys = RoleGuide {
            energy_response: 2.0,
            duration_mult: 0.9,
            vel_base: 0.6,
            vel_energy: 0.2,
            vel_jitter: 0.06,
            voice: Voicing::Scale {
                degree: 0,
                octave: 1,
                jitter: 0,
            },
            ..RoleGuide::pitched(STAB_W, 0, 1, 0, 0.9, 0.6)
        };
        // Слой саба под панчем.
        let kick808 = RoleGuide {
            climax_boost: 0.3,
            vel_base: 0.9,
            vel_energy: 0.05,
            voice: Voicing::Scale {
                degree: 0,
                octave: -1,
                jitter: 0,
            },
            poles: Some(KICK_POLES),
            ..RoleGuide::pitched(KICK_W, 0, -1, 0, 1.0, 0.9)
        };
        let laser = RoleGuide {
            energy_response: 1.4,
            duration_mult: 0.8,
            vel_base: 0.4,
            vel_energy: 0.2,
            vel_jitter: 0.07,
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
        let vocal = RoleGuide {
            energy_response: 1.4,
            duration_mult: 1.4,
            vel_base: 0.5,
            vel_energy: 0.15,
            vel_jitter: 0.08,
            voice: Voicing::Scale {
                degree: 2,
                octave: 2,
                jitter: 2,
            },
            ..RoleGuide::pitched(CHOP_W, 2, 2, 2, 1.4, 0.5)
        };
        let flute = RoleGuide {
            energy_response: 1.0,
            duration_mult: 4.0,
            vel_base: 0.35,
            voice: Voicing::Scale {
                degree: 0,
                octave: 1,
                jitter: 2,
            },
            ..RoleGuide::pitched(ARP_W, 0, 1, 2, 4.0, 0.35)
        };
        // Хук дропа по мотиву сида.
        let festival = RoleGuide {
            energy_response: 2.0,
            climax_boost: 0.3,
            duration_mult: 0.9,
            vel_base: 0.55,
            vel_energy: 0.25,
            vel_jitter: 0.06,
            voice: Voicing::Scale {
                degree: 0,
                octave: 2,
                jitter: 0,
            },
            follow_motif: true,
            ..RoleGuide::pitched(HOOK_W, 0, 2, 0, 0.9, 0.55)
        };
        let crash = RoleGuide {
            climax_only: true,
            duration_mult: 12.0,
            vel_base: 0.8,
            vel_jitter: 0.05,
            ..RoleGuide::kit(CRASH_W, 49, 0.8)
        };
        let guitar = RoleGuide {
            energy_response: 1.0,
            duration_mult: 1.2,
            vel_base: 0.35,
            voice: Voicing::Scale {
                degree: 0,
                octave: 1,
                jitter: 2,
            },
            ..RoleGuide::pitched(ARP_W, 0, 1, 2, 1.2, 0.35)
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
