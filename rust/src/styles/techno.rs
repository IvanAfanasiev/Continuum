use super::{RoleGuide, Style, Voicing};
use crate::instruments::Slot;

/// Techno: темный роллинг. Неумолимая бочка, рокочущий оффбитный бас
/// шестнадцатыми, закрытые хэты стеной, даб-стаб редко и далеко.
/// Минимум мелодии, максимум гипноза.
const SCALE: [u8; 7] = [48, 50, 51, 53, 55, 56, 58];

const KICK_W: &[f32] = &[
    1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0,
];
const KICK_P1: &[f32] = &[
    1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.5, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0,
];
const KICK_POLES: &[&[f32]] = &[KICK_W, KICK_P1];

const CLAP_W: &[f32] = &[
    0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.3,
];
const CLAP_P1: &[f32] = &[
    0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.3, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.3, 0.0,
];
const CLAP_POLES: &[&[f32]] = &[CLAP_W, CLAP_P1];

const HAT_W: &[f32] = &[
    0.4, 0.5, 0.4, 0.5, 0.4, 0.5, 0.4, 0.5, 0.4, 0.5, 0.4, 0.5, 0.4, 0.5, 0.4, 0.5,
];
const HAT_P1: &[f32] = &[
    0.3, 0.0, 0.5, 0.0, 0.3, 0.0, 0.5, 0.4, 0.3, 0.0, 0.5, 0.0, 0.3, 0.0, 0.5, 0.4,
];
const HAT_P2: &[f32] = &[
    0.5, 0.5, 0.5, 0.5, 0.5, 0.0, 1.0, 0.0, 0.5, 0.5, 0.5, 0.5, 0.5, 0.0, 1.0, 0.0,
];
const HAT_POLES: &[&[f32]] = &[HAT_W, HAT_P1, HAT_P2];

// Рамбл: рокот шестнадцатыми с акцентом на оффбит (якоря держат интро).
const BASS_W: &[f32] = &[
    0.3, 0.5, 1.0, 0.5, 0.3, 0.5, 1.0, 0.5, 0.3, 0.5, 1.0, 0.5, 0.3, 0.5, 1.0, 0.5,
];
const BASS_P1: &[f32] = &[
    0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.5, 1.0, 0.0,
];
const BASS_P2: &[f32] = &[
    1.0, 0.0, 0.0, 0.5, 0.0, 0.0, 0.7, 0.0, 1.0, 0.5, 0.0, 0.0, 0.7, 0.0, 0.0, 0.5,
];
const BASS_POLES: &[&[f32]] = &[BASS_W, BASS_P1, BASS_P2];

const STAB_W: &[f32] = &[
    0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.6, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.6, 0.0,
];
const DRONE_W: &[f32] = &[
    1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
];
const ACID_W: &[f32] = &[
    0.0, 0.5, 0.0, 0.5, 0.0, 0.0, 0.5, 0.0, 0.0, 0.5, 0.0, 0.5, 0.0, 0.0, 0.6, 0.0,
];
const CRASH_W: &[f32] = &[
    1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
];
const BED_W: &[f32] = &[
    0.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
];

pub struct TechnoStyle {}

impl TechnoStyle {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for TechnoStyle {
    fn default() -> Self {
        Self::new()
    }
}

impl Style for TechnoStyle {
    fn name(&self) -> &str {
        "techno"
    }

    fn bpm(&self) -> f32 {
        132.0
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
        0.18
    }

    fn reverb_size(&self) -> f32 {
        0.6
    }

    fn treble_cutoff(&self) -> f32 {
        17000.0
    }

    /// Форма энергии: relentless — машина не глохнет даже в теме.
    fn shape_energy(&self, energy: f32) -> f32 {
        0.55 + 0.45 * energy
    }

    fn ensemble(&self) -> &'static [Slot] {
        // Машина: бочка, рамбл-бас, стена хэтов, клэп, даб-стаб, дрон.
        // Без вокала, лазера и 808.
        &[
            Slot::Bass,
            Slot::Kick,
            Slot::Pad,
            Slot::Snare,
            Slot::Hat,
            Slot::Pluck,
            Slot::Keys,
            Slot::Crash,
        ]
    }

    fn slot_guides(&self) -> [RoleGuide; Slot::COUNT] {
        let bass = RoleGuide {
            energy_response: 1.4,
            duration_mult: 0.6,
            vel_base: 0.7,
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
            bass_fifth: true,
            ..RoleGuide::pitched(BASS_W, 0, -1, 0, 0.6, 0.7)
        };
        let kick = RoleGuide {
            climax_boost: 0.5,
            vel_base: 0.95,
            vel_energy: 0.05,
            poles: Some(KICK_POLES),
            ..RoleGuide::kit(KICK_W, 36, 0.95)
        };
        let pad = RoleGuide {
            duration_mult: 16.0,
            vel_base: 0.35,
            vel_jitter: 0.0,
            ..RoleGuide::pitched(DRONE_W, 0, 0, 0, 16.0, 0.35)
        };
        let snare = RoleGuide {
            climax_boost: 0.6,
            vel_base: 0.8,
            poles: Some(CLAP_POLES),
            ..RoleGuide::kit(CLAP_W, 38, 0.8)
        };
        let hat = RoleGuide {
            energy_response: 1.1,
            duration_mult: 0.3,
            vel_base: 0.38,
            vel_energy: 0.2,
            vel_jitter: 0.08,
            poles: Some(HAT_POLES),
            ..RoleGuide::kit(HAT_W, 42, 0.38)
        };
        // Кислотная нить: редкая, по мотиву сида.
        let pluck = RoleGuide {
            energy_response: 1.6,
            duration_mult: 0.5,
            vel_base: 0.4,
            vel_energy: 0.25,
            vel_jitter: 0.06,
            voice: Voicing::Scale {
                degree: 0,
                octave: 1,
                jitter: 0,
            },
            follow_motif: true,
            ..RoleGuide::pitched(ACID_W, 0, 1, 0, 0.5, 0.4)
        };
        // Даб-стаб: короткий удар в темноту.
        let keys = RoleGuide {
            energy_response: 2.0,
            duration_mult: 2.5,
            vel_base: 0.5,
            vel_jitter: 0.06,
            voice: Voicing::Scale {
                degree: 4,
                octave: 1,
                jitter: 0,
            },
            ..RoleGuide::pitched(STAB_W, 4, 1, 0, 2.5, 0.5)
        };
        // Чужие слоты: сдержанные направляющие для ручек фронта.
        let kick808 = RoleGuide {
            vel_base: 0.8,
            ..RoleGuide::kit(KICK_W, 36, 0.8)
        };
        let laser = RoleGuide {
            energy_response: 1.0,
            duration_mult: 1.0,
            vel_base: 0.35,
            voice: Voicing::Scale {
                degree: 0,
                octave: 2,
                jitter: 2,
            },
            ..RoleGuide::pitched(ACID_W, 0, 2, 2, 1.0, 0.35)
        };
        let vinyl = RoleGuide {
            duration_mult: 200.0,
            vel_base: 0.3,
            ..RoleGuide::pitched(BED_W, 0, 0, 0, 200.0, 0.3)
        };
        let vocal = RoleGuide {
            energy_response: 1.0,
            duration_mult: 2.0,
            vel_base: 0.35,
            voice: Voicing::Scale {
                degree: 0,
                octave: 1,
                jitter: 2,
            },
            ..RoleGuide::pitched(STAB_W, 0, 1, 2, 2.0, 0.35)
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
            ..RoleGuide::pitched(ACID_W, 0, 1, 2, 4.0, 0.35)
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
            ..RoleGuide::pitched(ACID_W, 0, 2, 2, 0.8, 0.4)
        };
        let crash = RoleGuide {
            climax_only: true,
            duration_mult: 12.0,
            vel_base: 0.7,
            vel_jitter: 0.05,
            ..RoleGuide::kit(CRASH_W, 49, 0.7)
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
            ..RoleGuide::pitched(ACID_W, 0, 1, 2, 1.5, 0.3)
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
