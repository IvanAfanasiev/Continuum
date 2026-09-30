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
// Синкопированный фанк-кик (ref: Redbone — без прямой бочки).
// Разреженный и заякоренный на 1: бум-бэп, а не пулемет.
// (7 частых кандидатов давали кашу на высокой энергии/интенсивности.)
const KICK_SYNCOP_W: &[f32] = &[
    1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.7, 0.0, 0.0, 0.0, 0.6, 0.0, 0.0, 0.0, 0.4, 0.0,
];
const KICK_POLES: &[&[f32]] = &[KICK_W, KICK_ALT_W, KICK_SYNCOP_W];
const BASS_ALT_W: &[f32] = &[
    1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
];
const HAT_ALT_W: &[f32] = &[
    0.7, 0.45, 0.7, 0.45, 0.7, 0.45, 0.7, 0.45, 0.7, 0.45, 0.7, 0.45, 0.7, 0.45, 0.7, 0.45,
];
// Разреженные оффбиты (ref: Snowman, Redbone — только слабые доли,
// никаких шестнадцатых тиков).
const HAT_SPARSE_W: &[f32] = &[
    0.0, 0.0, 0.9, 0.0, 0.0, 0.0, 0.9, 0.0, 0.0, 0.0, 0.9, 0.0, 0.0, 0.0, 0.9, 0.4,
];
const HAT_POLES: &[&[f32]] = &[HAT_W, HAT_ALT_W, HAT_SPARSE_W];
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
        1.45
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
        // Нарезок-плака нет: поверх Rhodes и гитары они дают кашу —
        // включить можно вручную, дефолт чистый. Лазера, 808, вокала
        // и фестиваля нет слышно даже рядом.
        &[
            Slot::Bass,
            Slot::Kick,
            Slot::Pad,
            Slot::Snare,
            Slot::Hat,
            Slot::Keys,
            Slot::Vinyl,
            Slot::Flute,
            Slot::Guitar,
            Slot::Rain,
        ]
    }

    /// Гость по сиду: пыльные чопы или глубокий саб-толчок.
    fn flex_pool(&self) -> &'static [Slot] {
        &[Slot::Vocal, Slot::Kick808]
    }

    fn flex_count(&self) -> usize {
        1
    }

    fn slot_guides(&self) -> [RoleGuide; Slot::COUNT] {
        // Саб-ковер: мягко и длинно, без резких атак. Тише драйва —
        // акцент на гитаре и клавишах, бас только держит низ.
        // Сустейн умеренный (4 шага): длинные ноты слоями давали кашу.
        let bass = RoleGuide {
            energy_response: 0.8,
            duration_mult: 4.0,
            vel_base: 0.55,
            vel_jitter: 0.05,
            voice: Voicing::Scale {
                degree: 0,
                octave: -1,
                jitter: 0,
            },
            vel_high_cut: 0.6,
            alt: Some(BASS_ALT_W),
            ..RoleGuide::pitched(BASS_W, 0, -1, 0, 4.0, 0.55)
        };
        let kick = RoleGuide {
            energy_response: 0.8,
            vel_base: 0.9,
            vel_jitter: 0.1,
            alt: Some(KICK_ALT_W),
            poles: Some(KICK_POLES),
            ..RoleGuide::kit(KICK_W, 36, 0.9)
        };
        let pad = RoleGuide {
            duration_mult: 8.0,
            vel_base: 0.35,
            voice: Voicing::Scale {
                degree: 0,
                octave: 0,
                jitter: 0,
            },
            ..RoleGuide::pitched(PAD_W, 0, 0, 0, 8.0, 0.35)
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
            poles: Some(HAT_POLES),
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
            duration_mult: 4.0,
            vel_base: 0.6,
            vel_jitter: 0.08,
            voice: Voicing::Scale {
                degree: 0,
                octave: 1,
                jitter: 2,
            },
            alt: Some(KEYS_ALT_W),
            ..RoleGuide::pitched(KEYS_W, 0, 1, 2, 4.0, 0.6)
        };
        // Чужие слоты: приглушенные рабочие направляющие — включаются
        // только ручкой фронта, дефолтный характер не ломают.
        // Гость-808 в lofi — саб-дроп на 1, а не дабл паттерна кика:
        // удвоение давало flam и наслоение низа.
        const SUB_DROP_W: &[f32] = &[
            1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
            0.0, 0.0,
        ];
        let kick808 = RoleGuide {
            vel_base: 0.8,
            ..RoleGuide::kit(SUB_DROP_W, 36, 0.8)
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
        // Ведущий голос lofi — чуть громче саба.
        let guitar = RoleGuide {
            energy_response: 1.0,
            duration_mult: 1.8,
            vel_base: 0.6,
            vel_jitter: 0.08,
            voice: Voicing::Scale {
                degree: 0,
                octave: 1,
                jitter: 0,
            },
            follow_motif: true,
            ..RoleGuide::pitched(GUITAR_W, 0, 1, 0, 1.8, 0.6)
        };
        // Дождь: ровная пелена во все фразы.
        let rain = RoleGuide {
            duration_mult: 150.0,
            vel_base: 0.25,
            vel_jitter: 0.05,
            ..RoleGuide::pitched(BED_W, 0, 0, 0, 150.0, 0.25)
        };
        // Чужой крэш: тихий, только на стыках секций.
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
            festival, guitar, rain, fuzz, acid,
        ]
    }
}
