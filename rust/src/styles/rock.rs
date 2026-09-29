use super::{RoleGuide, Style, StyleGroup, Voicing};
use crate::instruments::Slot;

/// Rock: механический концерт. Перегруженный синт вместо гитар,
/// вдавленная бочка, хлесткий снейр, бас восьмыми и крэши как удары
/// молота. Никаких переборов, свинга и эмбиент-пелены: жестко и ровно.
const SCALE: [u8; 5] = [48, 51, 53, 55, 58];

const KICK_W: &[f32] = &[
    1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0,
];
const KICK_P1: &[f32] = &[
    1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.9, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0,
];
const KICK_P2: &[f32] = &[
    1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.9, 0.0, 0.0, 0.0, 0.0, 0.0,
];
const KICK_POLES: &[&[f32]] = &[KICK_W, KICK_P1, KICK_P2];

const SNARE_W: &[f32] = &[
    0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0,
];
const SNARE_P1: &[f32] = &[
    0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.5, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0,
];
const SNARE_POLES: &[&[f32]] = &[SNARE_W, SNARE_P1];

const HAT_W: &[f32] = &[
    0.7, 0.0, 0.5, 0.0, 0.7, 0.0, 0.5, 0.0, 0.7, 0.0, 0.5, 0.0, 0.7, 0.0, 0.5, 0.4,
];
const HAT_P1: &[f32] = &[
    0.6, 0.4, 0.6, 0.4, 0.6, 0.4, 0.6, 0.4, 0.6, 0.4, 0.6, 0.4, 0.6, 0.4, 0.6, 0.4,
];
const HAT_POLES: &[&[f32]] = &[HAT_W, HAT_P1];

const BASS_W: &[f32] = &[
    1.0, 0.0, 0.6, 0.0, 1.0, 0.0, 0.6, 0.0, 1.0, 0.0, 0.6, 0.0, 1.0, 0.0, 0.6, 0.5,
];
const BASS_P1: &[f32] = &[
    1.0, 0.0, 0.0, 0.6, 0.0, 0.0, 1.0, 0.0, 0.0, 0.6, 0.0, 0.0, 1.0, 0.0, 0.6, 0.0,
];
const BASS_POLES: &[&[f32]] = &[BASS_W, BASS_P1];

// Рев: рифф чопами по мотиву сида.
const RIFF_W: &[f32] = &[
    0.8, 0.0, 0.6, 0.0, 0.8, 0.0, 0.6, 0.0, 0.8, 0.0, 0.6, 0.0, 0.8, 0.0, 0.6, 0.6,
];
const RIFF_P1: &[f32] = &[
    0.8, 0.0, 0.0, 0.6, 0.0, 0.8, 0.0, 0.0, 0.6, 0.0, 0.8, 0.0, 0.0, 0.6, 0.8, 0.0,
];
const RIFF_POLES: &[&[f32]] = &[RIFF_W, RIFF_P1];

const BED_W: &[f32] = &[
    0.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
];

pub struct RockStyle {}

impl RockStyle {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for RockStyle {
    fn default() -> Self {
        Self::new()
    }
}

impl Style for RockStyle {
    fn name(&self) -> &str {
        "rock"
    }

    fn group(&self) -> StyleGroup {
        StyleGroup::Rock
    }

    fn bpm(&self) -> f32 {
        130.0
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
        18500.0
    }

    /// Форма энергии: пресс держит весь круг, дроп — пик пресса.
    fn shape_energy(&self, energy: f32) -> f32 {
        0.55 + 0.45 * energy
    }

    /// Сайдчейн давит агрессивно: весь микс качает под прямой ритм.
    fn sidechain_depth(&self) -> f32 {
        1.5
    }

    fn ensemble(&self) -> &'static [Slot] {
        // Механика: бочка, снейр, хэт, бас и рев фазза.
        // Без тарелок: металлический звон не к месту даже на стыках.
        // Без гитарных переборов, клавиш и текстур.
        &[
            Slot::Bass,
            Slot::Kick,
            Slot::Snare,
            Slot::Hat,
            Slot::Fuzz,
        ]
    }

    /// Гость по сиду: редкое пианино или далекие чопы поверх стены.
    fn flex_pool(&self) -> &'static [Slot] {
        &[Slot::Keys, Slot::Vocal]
    }

    fn flex_count(&self) -> usize {
        1
    }

    fn slot_guides(&self) -> [RoleGuide; Slot::COUNT] {
        let bass = RoleGuide {
            energy_response: 1.4,
            duration_mult: 0.6,
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
            ..RoleGuide::pitched(BASS_W, 0, -1, 0, 0.6, 0.8)
        };
        // Вдавленная бочка: якоря в пол, компрессия — в миксе.
        let kick = RoleGuide {
            climax_boost: 0.6,
            vel_base: 1.0,
            vel_energy: 0.0,
            vel_jitter: 0.02,
            poles: Some(KICK_POLES),
            ..RoleGuide::kit(KICK_W, 36, 1.0)
        };
        let pad = RoleGuide {
            duration_mult: 8.0,
            vel_base: 0.25,
            ..RoleGuide::pitched(BED_W, 0, 0, 0, 8.0, 0.25)
        };
        // Хлесткий снейр: транзиент + тело.
        let snare = RoleGuide {
            climax_boost: 0.7,
            vel_base: 0.9,
            vel_jitter: 0.04,
            poles: Some(SNARE_POLES),
            ..RoleGuide::kit(SNARE_W, 38, 0.9)
        };
        let hat = RoleGuide {
            energy_response: 1.0,
            duration_mult: 0.35,
            vel_base: 0.42,
            vel_energy: 0.18,
            vel_jitter: 0.06,
            poles: Some(HAT_POLES),
            ..RoleGuide::kit(HAT_W, 42, 0.42)
        };
        let pluck = RoleGuide {
            energy_response: 1.0,
            duration_mult: 0.8,
            vel_base: 0.28,
            voice: Voicing::Scale {
                degree: 0,
                octave: 1,
                jitter: 2,
            },
            ..RoleGuide::pitched(RIFF_W, 0, 1, 2, 0.8, 0.28)
        };
        let keys = RoleGuide {
            energy_response: 1.0,
            duration_mult: 3.0,
            vel_base: 0.28,
            voice: Voicing::Scale {
                degree: 0,
                octave: 1,
                jitter: 0,
            },
            ..RoleGuide::pitched(BED_W, 0, 1, 0, 3.0, 0.28)
        };
        // Чужие слоты: сдержанные направляющие для ручек фронта.
        let kick808 = RoleGuide {
            vel_base: 0.7,
            ..RoleGuide::kit(KICK_W, 36, 0.7)
        };
        let laser = RoleGuide {
            energy_response: 1.0,
            duration_mult: 0.8,
            vel_base: 0.28,
            voice: Voicing::Scale {
                degree: 0,
                octave: 2,
                jitter: 2,
            },
            ..RoleGuide::pitched(RIFF_W, 0, 2, 2, 0.8, 0.28)
        };
        let vinyl = RoleGuide {
            duration_mult: 200.0,
            vel_base: 0.2,
            ..RoleGuide::pitched(BED_W, 0, 0, 0, 200.0, 0.2)
        };
        let vocal = RoleGuide {
            energy_response: 1.0,
            duration_mult: 1.2,
            vel_base: 0.3,
            voice: Voicing::Scale {
                degree: 0,
                octave: 1,
                jitter: 2,
            },
            ..RoleGuide::pitched(RIFF_W, 0, 1, 2, 1.2, 0.3)
        };
        let flute = RoleGuide {
            energy_response: 1.0,
            duration_mult: 4.0,
            vel_base: 0.25,
            voice: Voicing::Scale {
                degree: 0,
                octave: 1,
                jitter: 2,
            },
            ..RoleGuide::pitched(RIFF_W, 0, 1, 2, 4.0, 0.25)
        };
        let festival = RoleGuide {
            energy_response: 1.2,
            duration_mult: 0.8,
            vel_base: 0.3,
            vel_jitter: 0.06,
            voice: Voicing::Scale {
                degree: 0,
                octave: 1,
                jitter: 0,
            },
            follow_motif: true,
            ..RoleGuide::pitched(RIFF_W, 0, 1, 0, 0.8, 0.3)
        };
        let guitar = RoleGuide {
            energy_response: 1.0,
            duration_mult: 1.2,
            vel_base: 0.3,
            voice: Voicing::Scale {
                degree: 0,
                octave: 0,
                jitter: 1,
            },
            ..RoleGuide::pitched(RIFF_W, 0, 0, 1, 1.2, 0.3)
        };
        let rain = RoleGuide {
            duration_mult: 150.0,
            vel_base: 0.2,
            ..RoleGuide::pitched(BED_W, 0, 0, 0, 150.0, 0.2)
        };
        // Рев вместо гитар: рифф по мотиву сида, стена на дропе.
        let fuzz = RoleGuide {
            energy_response: 1.6,
            climax_boost: 0.2,
            duration_mult: 0.7,
            vel_base: 0.65,
            vel_energy: 0.2,
            vel_jitter: 0.06,
            voice: Voicing::Scale {
                degree: 0,
                octave: 1,
                jitter: 0,
            },
            follow_motif: true,
            poles: Some(RIFF_POLES),
            ..RoleGuide::pitched(RIFF_W, 0, 1, 0, 0.7, 0.65)
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
            festival, guitar, rain, fuzz, acid,
        ]
    }
}
