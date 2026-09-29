use super::{RoleGuide, Style, StyleGroup, Voicing};
use crate::instruments::Slot;

/// Lo-fi: пыльно, тепло, криво. Якоря бум-бэпа (кик на 1 и синкопу,
/// снейр на 2 и 4, глубокий бас, Rhodes) + виниловый треск.
/// Никаких EDM-лидов, квантайза в ноль и взрывных дропов: энергия
/// сплющена, свинг сильный, верх срезан.
const SCALE: [u8; 5] = [48, 51, 53, 55, 58];

const KICK_W: &[f32] = &[
    1.0, 0.0, 0.0, 0.2, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.35, 0.0, 0.0, 0.0, 0.0, 0.0,
];
const SNARE_W: &[f32] = &[
    0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.15,
];
const HAT_W: &[f32] = &[
    1.0, 0.25, 1.0, 0.25, 1.0, 0.25, 1.0, 0.25, 1.0, 0.25, 1.0, 0.25, 1.0, 0.25, 1.0, 0.25,
];
const BASS_W: &[f32] = &[
    1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.5, 0.0, 0.0, 0.0, 0.0, 0.0,
];
const PAD_W: &[f32] = &[
    0.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
];
const PLUCK_W: &[f32] = &[
    1.0, 0.0, 0.28, 0.0, 0.0, 0.0, 0.28, 0.0, 0.0, 0.0, 0.0, 0.28, 0.0, 0.0, 0.28, 0.0,
];
const KEYS_W: &[f32] = &[
    1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
];
const BED_W: &[f32] = &[
    1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
];
const CRASH_W: &[f32] = &[
    1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
];
// Длинная флейта поверх Rhodes: редкие ноты с дыханием.
const FLUTE_W: &[f32] = &[
    0.3, 0.0, 0.0, 0.0, 0.0, 0.0, 0.25, 0.0, 0.0, 0.0, 0.0, 0.3, 0.0, 0.0, 0.0, 0.0,
];
// Фингерпикинг гитары: синкопы между долями.
const GUITAR_W: &[f32] = &[
    0.7, 0.0, 0.0, 0.4, 0.0, 0.0, 0.6, 0.0, 0.0, 0.0, 0.5, 0.0, 0.6, 0.0, 0.0, 0.3,
];
// Второй бум-бэп: синкопа уехала с 7 на 6, ответ с 10 на 12.
const KICK_ALT_W: &[f32] = &[
    1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.8, 0.0, 0.0, 0.0, 0.35, 0.0, 0.2, 0.0, 0.0, 0.0,
];
const BASS_ALT_W: &[f32] = &[
    1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
];
const HAT_ALT_W: &[f32] = &[
    0.7, 0.45, 0.7, 0.45, 0.7, 0.45, 0.7, 0.45, 0.7, 0.45, 0.7, 0.45, 0.7, 0.45, 0.7, 0.45,
];
const PLUCK_ALT_W: &[f32] = &[
    0.0, 0.28, 0.0, 0.0, 0.0, 0.28, 0.0, 0.0, 0.0, 0.28, 0.0, 0.0, 0.0, 0.28, 0.0, 0.0,
];
const KEYS_ALT_W: &[f32] = &[
    1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.5, 0.0, 0.0, 0.0, 0.0,
];

pub struct LofiStyle {}

impl LofiStyle {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for LofiStyle {
    fn default() -> Self {
        Self::new()
    }
}

impl Style for LofiStyle {
    fn name(&self) -> &str {
        "lofi"
    }

    fn group(&self) -> StyleGroup {
        StyleGroup::Chill
    }

    fn bpm(&self) -> f32 {
        78.0
    }

    fn steps_per_measure(&self) -> usize {
        16
    }

    fn swing_ratio(&self) -> f32 {
        1.35
    }

    fn scale(&self) -> &'static [u8] {
        &SCALE
    }

    fn arrangement(&self) -> crate::arranger::Score {
        crate::arranger::CALM_SCORE
    }

    fn reverb_wet(&self) -> f32 {
        0.12
    }

    fn reverb_size(&self) -> f32 {
        0.35
    }

    fn treble_cutoff(&self) -> f32 {
        7500.0
    }

    /// Пыльная полка: энергия 0.32..0.70 — ни взрывов, ни провалов.
    fn shape_energy(&self, energy: f32) -> f32 {
        0.32 + 0.38 * energy
    }

    /// Сайдчейн едва дышит: lofi качает мягко, без EDM-пампа.
    fn sidechain_depth(&self) -> f32 {
        0.5
    }

    fn ensemble(&self) -> &'static [Slot] {
        // Классика: барабаны + бас + Rhodes + гитара + флейта + винил + дождь.
        // Лазера, 808, вокала и фестиваля нет слышно даже рядом.
        &[
            Slot::Bass,
            Slot::Kick,
            Slot::Pad,
            Slot::Snare,
            Slot::Hat,
            Slot::Pluck,
            Slot::Keys,
            Slot::Vinyl,
            Slot::Flute,
            Slot::Guitar,
            Slot::Rain,
        ]
    }

    fn slot_guides(&self) -> [RoleGuide; Slot::COUNT] {
        let bass = RoleGuide {
            energy_response: 0.8,
            duration_mult: 4.0,
            vel_base: 0.7,
            vel_jitter: 0.08,
            voice: Voicing::Scale {
                degree: 0,
                octave: -1,
                jitter: 0,
            },
            vel_high_cut: 0.6,
            alt: Some(BASS_ALT_W),
            ..RoleGuide::pitched(BASS_W, 0, -1, 0, 4.0, 0.7)
        };
        let kick = RoleGuide {
            energy_response: 0.8,
            vel_base: 0.9,
            vel_jitter: 0.1,
            alt: Some(KICK_ALT_W),
            ..RoleGuide::kit(KICK_W, 36, 0.9)
        };
        let pad = RoleGuide {
            duration_mult: 12.0,
            vel_base: 0.35,
            voice: Voicing::Scale {
                degree: 0,
                octave: 0,
                jitter: 0,
            },
            ..RoleGuide::pitched(PAD_W, 0, 0, 0, 12.0, 0.35)
        };
        let snare = RoleGuide {
            vel_base: 0.8,
            vel_jitter: 0.06,
            ..RoleGuide::kit(SNARE_W, 38, 0.8)
        };
        let hat = RoleGuide {
            long_offbeats: true,
            long_mult: 5.0,
            duration_mult: 0.3,
            vel_base: 0.45,
            vel_energy: 0.15,
            vel_jitter: 0.08,
            alt: Some(HAT_ALT_W),
            ..RoleGuide::kit(HAT_W, 42, 0.45)
        };
        // Нарезки гитары: редкие щипки по мотиву сида поверх Rhodes.
        let pluck = RoleGuide {
            duration_mult: 2.0,
            vel_base: 0.5,
            vel_jitter: 0.08,
            voice: Voicing::Scale {
                degree: 0,
                octave: 1,
                jitter: 0,
            },
            follow_motif: true,
            alt: Some(PLUCK_ALT_W),
            ..RoleGuide::pitched(PLUCK_W, 0, 1, 0, 2.0, 0.5)
        };
        let keys = RoleGuide {
            duration_mult: 6.0,
            vel_base: 0.55,
            vel_jitter: 0.08,
            voice: Voicing::Scale {
                degree: 0,
                octave: 1,
                jitter: 2,
            },
            alt: Some(KEYS_ALT_W),
            ..RoleGuide::pitched(KEYS_W, 0, 1, 2, 6.0, 0.55)
        };
        // Чужие слоты: приглушенные рабочие направляющие — включаются
        // только ручкой фронта, дефолтный характер не ломают.
        let kick808 = RoleGuide {
            vel_base: 0.8,
            ..RoleGuide::kit(KICK_W, 36, 0.8)
        };
        let laser = RoleGuide {
            energy_response: 1.2,
            duration_mult: 1.2,
            vel_base: 0.4,
            voice: Voicing::Scale {
                degree: 0,
                octave: 2,
                jitter: 3,
            },
            ..RoleGuide::pitched(PLUCK_W, 0, 2, 3, 1.2, 0.4)
        };
        let vinyl = RoleGuide {
            duration_mult: 300.0,
            vel_base: 0.3,
            vel_jitter: 0.05,
            ..RoleGuide::pitched(BED_W, 0, 0, 0, 300.0, 0.3)
        };
        let vocal = RoleGuide {
            energy_response: 1.2,
            duration_mult: 1.5,
            vel_base: 0.45,
            voice: Voicing::Scale {
                degree: 2,
                octave: 1,
                jitter: 2,
            },
            ..RoleGuide::pitched(KEYS_W, 2, 1, 2, 1.5, 0.45)
        };
        // Флейта поверх Rhodes: мотив сида длинными нотами с дыханием.
        let flute = RoleGuide {
            energy_response: 1.0,
            duration_mult: 6.0,
            vel_base: 0.45,
            vel_jitter: 0.08,
            voice: Voicing::Scale {
                degree: 0,
                octave: 1,
                jitter: 0,
            },
            follow_motif: true,
            ..RoleGuide::pitched(FLUTE_W, 0, 1, 0, 6.0, 0.45)
        };
        let festival = RoleGuide {
            energy_response: 1.2,
            duration_mult: 1.2,
            vel_base: 0.4,
            voice: Voicing::Scale {
                degree: 0,
                octave: 2,
                jitter: 3,
            },
            ..RoleGuide::pitched(PLUCK_W, 0, 2, 3, 1.2, 0.4)
        };
        // Гитара: синкопированный фингерпикинг по мотиву сида.
        let guitar = RoleGuide {
            energy_response: 1.0,
            duration_mult: 1.8,
            vel_base: 0.55,
            vel_jitter: 0.08,
            voice: Voicing::Scale {
                degree: 0,
                octave: 1,
                jitter: 0,
            },
            follow_motif: true,
            ..RoleGuide::pitched(GUITAR_W, 0, 1, 0, 1.8, 0.55)
        };
        // Дождь: ровная пелена во все фразы.
        let rain = RoleGuide {
            duration_mult: 150.0,
            vel_base: 0.25,
            vel_jitter: 0.05,
            ..RoleGuide::pitched(BED_W, 0, 0, 0, 150.0, 0.25)
        };
        // Чужой крэш: тихий, только на стыках секций.
        let crash = RoleGuide {
            climax_only: true,
            duration_mult: 8.0,
            vel_base: 0.3,
            ..RoleGuide::kit(CRASH_W, 49, 0.3)
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
