use super::{RoleGuide, Style, Voicing};
use crate::instruments::Slot;

/// Deep house: тёплый подземный дом. Ровный танцевальный бит 4/4,
/// глубокий мягкий бас, плавная повторяющаяся мелодия, Rhodes и пэд
/// как джазовая подложка, вокал — редкий чувственный слой.
///
/// Отличие от клуба не в тембрах, а в характере: темп ниже (122),
/// ход секций длиннее и мягче, в передышке уходят барабаны (мечтательный
/// breakdown), а бас глубже и без презенса.
const SCALE: [u8; 7] = [48, 50, 51, 53, 55, 56, 58];

// Сетка: бочка в четыре доли, клэп на 2 и 4, хэт — оффбитные 16-е.
const FOUR_FLOOR: &[f32] = &[
    1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0,
];
const CLAP_W: &[f32] = &[
    0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0,
];
// Клэп с призвуком: бэкбит держат якоря, между — едва слышный шепот.
const CLAP_GHOST_W: &[f32] = &[
    0.0, 0.0, 0.15, 0.0, 1.0, 0.0, 0.0, 0.2, 0.0, 0.0, 0.15, 0.0, 1.0, 0.0, 0.0, 0.2,
];
const SNARE_POLES: &[&[f32]] = &[CLAP_W, CLAP_GHOST_W];
// Хэт: открытые оффбитные 16-е (классика 909), шаги тише опорных.
const HAT_W: &[f32] = &[
    0.0, 0.35, 1.0, 0.35, 0.0, 0.35, 1.0, 0.35, 0.0, 0.35, 1.0, 0.35, 0.0, 0.35, 1.0, 0.35,
];
const HAT_OFF_W: &[f32] = &[
    0.0, 0.55, 0.9, 0.55, 0.0, 0.55, 0.9, 0.55, 0.0, 0.55, 0.9, 0.55, 0.0, 0.55, 0.9, 0.55,
];
const HAT_BUSY16_W: &[f32] = &[
    0.6, 0.4, 0.8, 0.4, 0.6, 0.4, 0.9, 0.4, 0.6, 0.4, 0.8, 0.4, 0.6, 0.4, 0.9, 0.5,
];
const HAT_POLES: &[&[f32]] = &[HAT_W, HAT_OFF_W, HAT_BUSY16_W];
// Полюса бочки: ровная четверка и редкие тихие толчки.
const KICK_SYNCOP_W: &[f32] = &[
    1.0, 0.0, 0.0, 0.3, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.3, 0.0, 1.0, 0.0, 0.35, 0.0,
];
const KICK_PUSH_W: &[f32] = &[
    1.0, 0.0, 0.3, 0.0, 1.0, 0.0, 0.0, 0.3, 1.0, 0.0, 0.3, 0.0, 1.0, 0.0, 0.0, 0.0,
];
const KICK_POLES: &[&[f32]] = &[FOUR_FLOOR, KICK_SYNCOP_W, KICK_PUSH_W];
// Бас: оффбитные восьмые вместо стены 16-х — бас качается между
// ударами бочки, а не ложится ковром под нее.
const BASS_W: &[f32] = &[
    0.0, 0.0, 0.85, 0.0, 0.0, 0.0, 0.85, 0.0, 0.0, 0.0, 0.85, 0.0, 0.0, 0.0, 0.85, 0.0,
];
// Пэд: длинная подушка, вторая доля такта — движение подложки.
const PAD_W: &[f32] = &[
    1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.4, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
];
// Мелодия: плавная линия с длинными нотами, повторяется фразами.
const LEAD_W: &[f32] = &[
    1.0, 0.25, 0.55, 0.25, 0.55, 0.25, 0.55, 0.25, 0.55, 0.25, 0.55, 0.25, 0.55, 0.25, 0.55, 0.25,
];
// Rhodes-стабы: джазовая подложка, мягкие акценты.
const STAB_W: &[f32] = &[
    0.0, 0.0, 0.65, 0.0, 0.0, 0.0, 0.65, 0.0, 0.0, 0.0, 0.65, 0.0, 0.0, 0.0, 0.65, 0.0,
];
// Шейкер: песок 16-х, ровный.
const SHAKER_W: &[f32] = &[
    0.5, 0.3, 1.0, 0.3, 0.5, 0.3, 1.0, 0.3, 0.5, 0.3, 1.0, 0.3, 0.5, 0.3, 1.0, 0.4,
];
// Вокал: редкие долгие ноты, сидит сзади.
const VOCAL_W: &[f32] = &[
    0.0, 0.0, 0.4, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.4, 0.0, 0.0, 0.0, 0.0, 0.0,
];
// Постельная подложка (винил, дождь): одна нота на такт.
const BED_W: &[f32] = &[
    1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
];

pub struct DeepHouseStyle {}

impl DeepHouseStyle {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for DeepHouseStyle {
    fn default() -> Self {
        Self::new()
    }
}

impl Style for DeepHouseStyle {
    fn name(&self) -> &str {
        "deep"
    }

    fn bpm(&self) -> f32 {
        // Дом-техно: 120–125. Ниже клуба (126) — движение медленнее,
        // танец мягче.
        122.0
    }

    fn steps_per_measure(&self) -> usize {
        16
    }

    fn swing_ratio(&self) -> f32 {
        // Тот же кач, что в клубе: свингованные 16-е против прямых.
        1.2
    }

    fn scale(&self) -> &'static [u8] {
        &SCALE
    }

    /// Форма deep house: длинная тема, длинный разгон (развитие идет
    /// постепенно), длинный удар, средняя передышка.
    fn arrangement(&self) -> crate::arranger::Score {
        crate::arranger::Score {
            cycle: &[
                (crate::arranger::SongSection::MainMotif, 12),
                (crate::arranger::SongSection::BuildUp, 8),
                (crate::arranger::SongSection::Drop, 12),
                (crate::arranger::SongSection::PostChorus, 6),
            ],
        }
    }

    /// Аранж deep house: мечтательная подложка (пэд, Rhodes, винил) и
    /// мягкий бас с первого такта; в передышке уходят барабаны — это
    /// breakdown, а не отдых.
    fn arrangement_layers(&self) -> crate::piece::Layers {
        use crate::piece::{L_GROOVE, L_HARMONY, L_TEXTURE, L_THEME, L_VOCAL};
        [
            // Тема: грув, бас, пэд с Rhodes, тема и винил.
            L_GROOVE | L_HARMONY | L_THEME | L_TEXTURE,
            // Разгон: подключается вокал (мягкий, с эхом).
            L_GROOVE | L_HARMONY | L_THEME | L_VOCAL | L_TEXTURE,
            // Удар: весь состав.
            L_GROOVE | L_HARMONY | L_THEME | L_VOCAL | L_TEXTURE,
            // Передышка: только подложка — барабаны и бас молчат.
            L_HARMONY | L_TEXTURE,
        ]
    }

    fn transition(&self) -> crate::piece::Transition {
        // Мягкая склейка: грув катит дальше, голос следующего куска
        // входит поверх него.
        crate::piece::Transition::Rolling { bars: 2 }
    }

    /// Больше реверберации, чем в клубе: «объёмный» — это хвост.
    fn reverb_wet(&self) -> f32 {
        0.17
    }

    fn reverb_size(&self) -> f32 {
        0.55
    }

    /// Тёплый верх: полка ниже, чем в клубе (12 кГц вместо 16) —
    /// цифрового блеска меньше, подушка мягче.
    fn treble_cutoff(&self) -> f32 {
        12000.0
    }

    /// Сайдчейн мягче клуба: дом-техно не должен перекачивать бас.
    fn sidechain_depth(&self) -> f32 {
        0.32
    }

    fn ensemble(&self) -> &'static [Slot] {
        // Синтезаторы (щипок, пэд), клавишные (Rhodes), барабанная
        // машина (бочка, клэп, хэт, шейкер), бас, винил и вокал.
        // Джаз и соул — в Rhodes и в тёплом пэде.
        &[
            Slot::Bass,
            Slot::Kick,
            Slot::Pad,
            Slot::Snare,
            Slot::Hat,
            Slot::Pluck,
            Slot::Keys,
            Slot::Shaker,
            Slot::Vinyl,
            Slot::Vocal,
        ]
    }

    fn slot_guides(&self) -> [RoleGuide; Slot::COUNT] {
        // Бас: глубокий и мягкий. Чистый синус (sub_bias 0.88 — почти без
        // презенса), длинный сустейн (2.4), низкий потолок (41) и
        // оффбитные восьмые с ходом на квинту: бас качается, а не стоит.
        let bass = RoleGuide {
            energy_response: 2.0,
            duration_mult: 2.4,
            vel_base: 0.72,
            vel_energy: 0.25,
            vel_jitter: 0.05,
            voice: Voicing::Scale {
                degree: 0,
                octave: -1,
                jitter: 1,
            },
            octave_up: 0.06,
            top_pitch: 48,
            vel_high_cut: 0.6,
            bass_fifth: true,
            bass_top: 41,
            sub_bias: 0.88,
            ..RoleGuide::pitched(BASS_W, 0, -1, 1, 2.4, 0.74)
        };
        // Бочка: ровная четверка. Толчки редкие и тихие — бит ровный.
        let kick = RoleGuide {
            vel_base: 0.95,
            vel_energy: 0.05,
            poles: Some(KICK_POLES),
            ..RoleGuide::kit(FOUR_FLOOR, 36, 0.95)
        };
        // Пэд: тёплая подушка на весь такт, ноты длинные.
        let pad = RoleGuide {
            duration_mult: 10.0,
            vel_base: 0.45,
            vel_jitter: 0.0,
            ..RoleGuide::pitched(PAD_W, 0, 0, 0, 10.0, 0.45)
        };
        // Клэп на 2 и 4, с призвуком. Забивка на стыке фразы — стык
        // слышно как ход, а не как обрыв.
        let snare = RoleGuide {
            phrase_fill: true,
            vel_base: 0.9,
            poles: Some(SNARE_POLES),
            build_roll: true,
            ..RoleGuide::kit(CLAP_W, 60, 0.9)
        };
        // Хэт: открытые оффбитные 16-е — подпись 909.
        let hat = RoleGuide {
            energy_response: 1.2,
            drop_before_climax: true,
            long_offbeats: true,
            long_mult: 5.0,
            duration_mult: 0.5,
            vel_base: 0.6,
            vel_energy: 0.35,
            vel_jitter: 0.1,
            poles: Some(HAT_POLES),
            section_poles: true,
            ..RoleGuide::kit(HAT_W, 42, 0.6)
        };
        // Тема: плавная, повторяющаяся. Джиттер 0 — линия цельная,
        // ноты длинные: мелодия льется фразами.
        let pluck = RoleGuide {
            energy_response: 1.4,
            duration_mult: 2.2,
            vel_base: 0.42,
            vel_energy: 0.28,
            vel_jitter: 0.05,
            voice: Voicing::Scale {
                degree: 0,
                octave: 2,
                jitter: 0,
            },
            follow_motif: true,
            phrase_rest: 0.15,
            ..RoleGuide::pitched(LEAD_W, 0, 2, 0, 2.2, 0.42)
        };
        // Rhodes: джазовая подложка, мягкие акценты, ровное присутствие.
        let keys = RoleGuide {
            energy_response: 1.1,
            duration_mult: 1.8,
            vel_base: 0.5,
            vel_jitter: 0.08,
            voice: Voicing::Scale {
                degree: 2,
                octave: 1,
                jitter: 0,
            },
            ..RoleGuide::pitched(STAB_W, 2, 1, 0, 1.8, 0.5)
        };
        // 808: гостевой бас для тех, кто хочет скользящий 808 вместо саба.
        let kick808 = RoleGuide {
            vel_base: 0.95,
            vel_energy: 0.05,
            voice: Voicing::Scale {
                degree: 0,
                octave: -1,
                jitter: 0,
            },
            bass_top: 41,
            ..RoleGuide::pitched(FOUR_FLOOR, 0, -1, 0, 1.0, 0.95)
        };
        // Вокал: мягкий, чувственный, редкий. Эффект обработки дает
        // мастер (эхо и реверб), сам голос сидит сзади.
        let vocal = RoleGuide {
            energy_response: 1.1,
            duration_mult: 3.4,
            vel_base: 0.38,
            vel_energy: 0.12,
            vel_jitter: 0.06,
            voice: Voicing::Scale {
                degree: 2,
                octave: 2,
                jitter: 2,
            },
            ..RoleGuide::pitched(VOCAL_W, 2, 2, 2, 3.4, 0.38)
        };
        // Винил: треск и пыль, постоянная подложка.
        let vinyl = RoleGuide {
            duration_mult: 200.0,
            vel_base: 0.35,
            vel_jitter: 0.05,
            ..RoleGuide::pitched(BED_W, 0, 0, 0, 200.0, 0.35)
        };
        // Гитара — ручной гость (духовый элемент для слушателя).
        let guitar = RoleGuide {
            energy_response: 1.1,
            duration_mult: 1.8,
            vel_base: 0.35,
            voice: Voicing::Scale {
                degree: 0,
                octave: 1,
                jitter: 2,
            },
            ..RoleGuide::pitched(LEAD_W, 0, 1, 2, 1.8, 0.35)
        };
        // Дождь: ровная пелена, тише винила.
        let rain = RoleGuide {
            duration_mult: 150.0,
            vel_base: 0.3,
            ..RoleGuide::pitched(BED_W, 0, 0, 0, 150.0, 0.3)
        };
        // Шейкер: песок 16-х поверх хэта.
        let shaker = RoleGuide {
            energy_response: 1.0,
            duration_mult: 0.4,
            vel_base: 0.45,
            vel_energy: 0.15,
            vel_jitter: 0.08,
            ..RoleGuide::kit(SHAKER_W, 60, 0.45)
        };
        // Соло-гитара — ручной гость.
        let guitar_lead = RoleGuide {
            energy_response: 1.0,
            duration_mult: 1.5,
            vel_base: 0.3,
            voice: Voicing::Scale {
                degree: 0,
                octave: 2,
                jitter: 1,
            },
            ..RoleGuide::pitched(BED_W, 0, 2, 1, 1.5, 0.3)
        };
        [
            bass,
            kick,
            pad,
            snare,
            hat,
            pluck,
            keys,
            kick808,
            vinyl,
            vocal,
            guitar,
            rain,
            shaker,
            guitar_lead,
        ]
    }
}
