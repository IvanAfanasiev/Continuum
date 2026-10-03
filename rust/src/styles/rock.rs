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
// Двойные удары: халф-тайм с дабл-кик флером перед даунбитом
// (15-й шаг пустой — вдох перед единицей).
const KICK_DBL_W: &[f32] = &[
    1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.8,
    0.0,
];
const KICK_POLES: &[&[f32]] = &[KICK_W, KICK_P1, KICK_P2, KICK_DBL_W];

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
// Шейкер: ровные восьмые, тихо.
const SHAKER_ROCK_W: &[f32] = &[
    0.0, 0.0, 0.6, 0.0, 0.0, 0.0, 0.6, 0.0, 0.0, 0.0, 0.6, 0.0, 0.0, 0.0, 0.6,
    0.0,
];

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
const RIFF_POLES: &[&[f32]] = &[RIFF_W, RIFF_P1, RIFF16_W];
// Пулеметные 16-е медиатором (ref: Thunderstruck): скорый перебор
// по ступеням темы. Общий полюс риффа: фраза решает, чоппы или пулемет,
// исполнители (фазз, гитара) играют ее каждый своим тембром.
const RIFF16_W: &[f32] = &[
    0.6, 0.5, 0.7, 0.5, 0.6, 0.5, 0.7, 0.5, 0.6, 0.5, 0.7, 0.5, 0.6, 0.5, 0.7,
    0.6,
];

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
        // Механика: бочка, снейр, хэт, бас, чистая гитара риффом и рев фазза.
        // Без тарелок: металлический звон не к месту даже на стыках.
        // Без гитарных переборов, клавиш и текстур.
        &[
            Slot::Bass,
            Slot::Kick,
            Slot::Snare,
            Slot::Hat,
            Slot::Guitar,
            Slot::Fuzz,
        ]
    }

    /// Гость по сиду: редкое пианино или далекие чопы поверх стены.
    /// Чопы чаще, пианино — редкая специя. Бюджет 1–2.
    fn flex_pool(&self) -> &'static [Slot] {
        &[Slot::Keys, Slot::Vocal]
    }

    fn flex_weights(&self) -> &'static [f32] {
        &[0.6, 1.0]
    }

    fn flex_range(&self) -> (usize, usize) {
        (1, 2)
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
            // Бас риффует вслед за гитарой (ref: doubling вплоть до D3),
            // а не лежит сабом: кап выше.
            bass_top: 52,
            ..RoleGuide::pitched(BASS_W, 0, -1, 0, 0.6, 0.8)
        };
        // Вдавленная бочка: якоря в пол, компрессия — в миксе.
        // Заливка умеренная, чтобы не ломать бит очередью.
        let kick = RoleGuide {
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
        // Хлесткий снейр: транзиент + тело. Заливка умеренная.
        let snare = RoleGuide {
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
            // Пульс вместо сетки: живой хэт поверх механики.
            emergent_beats: 0.5,
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
        // Чистая гитара риффом по мотиву: тот же план, что у фазза
        // (общий скелет риффа), своим тембром и броском гейта.
        // Унисон с фаззом октава в октаву: стена риффа, а не фон.
        // Чоппирующие восьмые поверх рева, без перегруза (он у фазза).
        // Второй полюс — пулеметные 16-е медиатором (ref: Thunderstruck).
        let guitar = RoleGuide {
            energy_response: 1.0,
            duration_mult: 1.0,
            vel_base: 0.75,
            voice: Voicing::Scale {
                degree: 0,
                octave: 1,
                jitter: 1,
            },
            follow_motif: true,
            poles: Some(RIFF_POLES),
            ..RoleGuide::pitched(RIFF_W, 0, 1, 1, 1.0, 0.75)
        };
        let rain = RoleGuide {
            duration_mult: 150.0,
            vel_base: 0.2,
            ..RoleGuide::pitched(BED_W, 0, 0, 0, 150.0, 0.2)
        };
        // Рев вместо гитар: рифф по мотиву сида, стена на дропе.
        let fuzz = RoleGuide {
            energy_response: 1.6,
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
        // Шейкер вне club: тихий ровный слой для ручки фронта.
        let shaker = RoleGuide {
            energy_response: 1.0,
            duration_mult: 0.4,
            vel_base: 0.3,
            vel_jitter: 0.06,
            ..RoleGuide::kit(SHAKER_ROCK_W, 60, 0.3)
        };
        // Формула вне club: редкая математическая специя для ручки фронта.
        let formula = RoleGuide {
            energy_response: 1.0,
            duration_mult: 1.0,
            vel_base: 0.3,
            voice: Voicing::Scale {
                degree: 0,
                octave: 1,
                jitter: 2,
            },
            ..RoleGuide::pitched(BED_W, 0, 1, 2, 1.0, 0.3)
        };
        [
            bass, kick, pad, snare, hat, pluck, keys, kick808, laser, vinyl, vocal, flute,
            festival, guitar, rain, fuzz, shaker, formula,
        ]
    }
}
