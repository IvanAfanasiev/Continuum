use super::{RoleGuide, Style, Voicing};
use crate::instruments::Slot;

/// Trance: гимн и эйфория. Роллинг-бас шестнадцатыми, стена суперпилы,
/// арпеджио по мотиву сида и лид-гимн на дропе. Воздушно и огромно.
const SCALE: [u8; 7] = [48, 50, 52, 53, 55, 57, 59];

const KICK_W: &[f32] = &[
    1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0,
];
const KICK_P1: &[f32] = &[
    1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.5, 0.0,
];
const KICK_POLES: &[&[f32]] = &[KICK_W, KICK_P1];

const CLAP_W: &[f32] = &[
    0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.5, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.5,
];
const CLAP_P1: &[f32] = &[
    0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.5, 0.5, 0.5,
];
const CLAP_POLES: &[&[f32]] = &[CLAP_W, CLAP_P1];

const HAT_W: &[f32] = &[
    0.4, 0.4, 1.0, 0.4, 0.4, 0.4, 1.0, 0.4, 0.4, 0.4, 1.0, 0.4, 0.4, 0.4, 1.0, 0.4,
];
const HAT_P1: &[f32] = &[
    0.3, 0.3, 0.3, 0.3, 1.0, 0.3, 0.3, 0.3, 0.3, 0.3, 0.3, 0.3, 1.0, 0.3, 0.3, 0.3,
];
const HAT_POLES: &[&[f32]] = &[HAT_W, HAT_P1];

// Роллинг: катящиеся шестнадцатые с оффбитным акцентом-якорем.
const BASS_W: &[f32] = &[
    0.5, 0.4, 1.0, 0.4, 0.5, 0.4, 1.0, 0.4, 0.5, 0.4, 1.0, 0.4, 0.5, 0.4, 1.0, 0.6,
];
const BASS_P1: &[f32] = &[
    0.0, 0.0, 1.0, 0.0, 0.5, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.5, 0.0, 1.0, 0.5,
];
const BASS_POLES: &[&[f32]] = &[BASS_W, BASS_P1];

const WALL_W: &[f32] = &[
    1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.6, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
];
const ARP_W: &[f32] = &[
    0.7, 0.5, 0.7, 0.5, 0.7, 0.5, 0.7, 0.5, 0.7, 0.5, 0.7, 0.5, 0.7, 0.5, 0.7, 0.5,
];
const ANTHEM_W: &[f32] = &[
    1.0, 0.3, 0.6, 0.3, 0.6, 0.3, 0.6, 0.3, 0.6, 0.3, 0.6, 0.3, 0.6, 0.3, 0.6, 0.3,
];
const STAB_W: &[f32] = &[
    0.0, 0.0, 0.7, 0.0, 0.0, 0.0, 0.7, 0.0, 0.0, 0.0, 0.7, 0.0, 0.0, 0.0, 0.7, 0.0,
];
const CRASH_W: &[f32] = &[
    1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
];
const BED_W: &[f32] = &[
    0.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
];

pub struct TranceStyle {}

impl TranceStyle {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for TranceStyle {
    fn default() -> Self {
        Self::new()
    }
}

impl Style for TranceStyle {
    fn name(&self) -> &str {
        "trance"
    }

    fn bpm(&self) -> f32 {
        138.0
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
        0.25
    }

    fn reverb_size(&self) -> f32 {
        0.7
    }

    fn treble_cutoff(&self) -> f32 {
        19000.0
    }

    /// Форма энергии: взрывной контраст — куплет шепчет, дроп кричит.
    fn shape_energy(&self, energy: f32) -> f32 {
        energy * energy
    }

    fn ensemble(&self) -> &'static [Slot] {
        // Гимн: бочка, роллинг, стена пилы, арп, лид-гимн и крэш.
        // Без 808, вокала и винила.
        &[
            Slot::Bass,
            Slot::Kick,
            Slot::Pad,
            Slot::Snare,
            Slot::Hat,
            Slot::Pluck,
            Slot::Keys,
            Slot::Festival,
            Slot::Crash,
        ]
    }

    fn slot_guides(&self) -> [RoleGuide; Slot::COUNT] {
        let bass = RoleGuide {
            energy_response: 1.5,
            duration_mult: 0.5,
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
            ..RoleGuide::pitched(BASS_W, 0, -1, 0, 0.5, 0.7)
        };
        let kick = RoleGuide {
            climax_boost: 0.6,
            vel_base: 0.95,
            vel_energy: 0.05,
            poles: Some(KICK_POLES),
            ..RoleGuide::kit(KICK_W, 36, 0.95)
        };
        // Стена суперпилы: длинные аккорды.
        let pad = RoleGuide {
            duration_mult: 10.0,
            vel_base: 0.5,
            vel_jitter: 0.0,
            voice: Voicing::Scale {
                degree: 0,
                octave: 0,
                jitter: 0,
            },
            ..RoleGuide::pitched(WALL_W, 0, 0, 0, 10.0, 0.5)
        };
        let snare = RoleGuide {
            climax_boost: 0.7,
            vel_base: 0.85,
            poles: Some(CLAP_POLES),
            ..RoleGuide::kit(CLAP_W, 38, 0.85)
        };
        let hat = RoleGuide {
            energy_response: 1.2,
            long_offbeats: true,
            long_mult: 4.0,
            duration_mult: 0.4,
            vel_base: 0.4,
            vel_energy: 0.25,
            vel_jitter: 0.07,
            poles: Some(HAT_POLES),
            ..RoleGuide::kit(HAT_W, 42, 0.4)
        };
        // Арпеджио-бегунок по мотиву сида.
        let pluck = RoleGuide {
            energy_response: 1.5,
            duration_mult: 0.6,
            vel_base: 0.45,
            vel_energy: 0.25,
            vel_jitter: 0.06,
            voice: Voicing::Scale {
                degree: 0,
                octave: 2,
                jitter: 0,
            },
            follow_motif: true,
            ..RoleGuide::pitched(ARP_W, 0, 2, 0, 0.6, 0.45)
        };
        let keys = RoleGuide {
            energy_response: 1.8,
            duration_mult: 1.0,
            vel_base: 0.5,
            vel_jitter: 0.06,
            voice: Voicing::Scale {
                degree: 2,
                octave: 1,
                jitter: 0,
            },
            ..RoleGuide::pitched(STAB_W, 2, 1, 0, 1.0, 0.5)
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
        let vocal = RoleGuide {
            energy_response: 1.2,
            duration_mult: 1.5,
            vel_base: 0.4,
            voice: Voicing::Scale {
                degree: 2,
                octave: 2,
                jitter: 2,
            },
            ..RoleGuide::pitched(ANTHEM_W, 2, 2, 2, 1.5, 0.4)
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
        // Гимн: лид дропа по мотиву сида.
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
            ..RoleGuide::pitched(ANTHEM_W, 0, 2, 0, 0.9, 0.55)
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
            ..RoleGuide::pitched(ARP_W, 0, 1, 2, 1.5, 0.3)
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
