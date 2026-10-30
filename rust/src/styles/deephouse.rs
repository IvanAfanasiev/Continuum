use super::riff::RiffKind;
use super::transition::{Kind, Spec};
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
// Rhodes/DX: подпись электро — арпеджио вместо аккорда. Ступени
// аккорда идут по очереди, а не одновременно: DX-электропиано звучит
// колокольной линией, а не готовым трезвучием.
// Аккорд: квинта и септима (электро-септима, не джазовая большая
// секста).
// Вес 1.0 — якорь: линия идет ровно, без вероятностного «дыхания»
// (при весе 0.8 гейт срезал арпеджио до двух нот на такт — это был
// не электро, а случайные штрихи).
const EP_ARP_W: &[f32] = &[
    1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0,
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
        // Дом-техно плейлиста: 120–125. Ниже клуба (126) — движение
        // медленнее, танец мягче.
        124.0
    }

    fn steps_per_measure(&self) -> usize {
        16
    }

    fn swing_ratio(&self) -> f32 {
        // Тот же кач, что в клубе: свингованные 16-е против прямых.
        1.2
    }

    fn style_contract(&self) -> super::contract::StyleContract {
        // Deep house: бас уступает кику, фраза дышит velocity-аркой,
        // не больше двух мелодических онсетов на шаг, верхний регистр 76.
        super::contract::StyleContract::new(true, true, 2, Some(76))
    }

    fn scale(&self) -> &'static [u8] {
        &SCALE
    }

    /// Форма deep house: длинная тема, длинный разгон (развитие идет
    /// постепенно), длинный удар, средняя передышка.
    fn arrangement(&self) -> crate::arranger::Score {
        use crate::arranger::{Score, SongSection};
        const LONG_VIEW: Score = Score {
            cycle: &[
                (SongSection::MainMotif, 12),
                (SongSection::BuildUp, 8),
                (SongSection::Drop, 12),
                (SongSection::PostChorus, 6),
            ],
        };
        LONG_VIEW
    }

    fn arrangement_for_seed(&self, seed: u64) -> crate::arranger::Score {
        use crate::arranger::{Score, SongSection};
        const LONG_VIEW: Score = Score {
            cycle: &[
                (SongSection::MainMotif, 12),
                (SongSection::BuildUp, 8),
                (SongSection::Drop, 12),
                (SongSection::PostChorus, 6),
            ],
        };
        const LONG_DROP: Score = Score {
            cycle: &[
                (SongSection::MainMotif, 8),
                (SongSection::BuildUp, 6),
                (SongSection::Drop, 14),
                (SongSection::PostChorus, 6),
            ],
        };
        const DEEP_BREATH: Score = Score {
            cycle: &[
                (SongSection::MainMotif, 10),
                (SongSection::BuildUp, 10),
                (SongSection::Drop, 10),
                (SongSection::PostChorus, 8),
            ],
        };
        crate::arranger::pick_score(seed, 0xD33F_F0A7, &[LONG_VIEW, LONG_DROP, DEEP_BREATH])
    }

    /// Аранж deep house: мечтательная подложка (пэд и Rhodes) и мягкий
    /// бас с первого такта; в передышке уходят барабаны — это breakdown,
    /// а не отдых. Текстуры нет: винил убран, «затертость» не нужна.
    fn arrangement_layers(&self) -> crate::piece::Layers {
        use crate::piece::{L_GROOVE, L_HARMONY, L_THEME, L_VOCAL};
        [
            // Тема: грув, бас, пэд с Rhodes и тема.
            L_GROOVE | L_HARMONY | L_THEME,
            // Разгон: подключается вокал (мягкий, с эхом).
            L_GROOVE | L_HARMONY | L_THEME | L_VOCAL,
            // Удар: весь состав.
            L_GROOVE | L_HARMONY | L_THEME | L_VOCAL,
            // Передышка: только подложка — барабаны и бас молчат.
            L_HARMONY,
        ]
    }

    fn transition(&self) -> crate::piece::Transition {
        // Мягкая склейка: грув катит дальше, голос следующего куска
        // входит поверх него.
        crate::piece::Transition::Rolling { bars: 2 }
    }

    /// Deep входит в удар шейкерным вихрем: песок сгущается к дропу,
    /// а после удара еще несколько шагов осыпается хвостом.
    fn transition_gesture(&self) -> Option<Spec> {
        Some(Spec::new(
            Kind::ShakerSpiral,
            &[Slot::Shaker],
            &[Slot::Shaker],
            4,
            70,
        ))
    }

    fn riff_kind(&self) -> RiffKind {
        RiffKind::Deep
    }

    /// Пространство сухое, но с хвостом: в плейлисте «Deephouse +
    /// Electro» звук плотный, объем дают не wash, а линия и пэд.
    fn reverb_wet(&self) -> f32 {
        0.12
    }

    fn reverb_size(&self) -> f32 {
        0.45
    }

    /// Верх с характером: полка 13.5 кГц — цифрового блеска меньше,
    /// чем в клубе (16), но DX-электропиано должно звенеть.
    fn treble_cutoff(&self) -> f32 {
        13500.0
    }

    /// Сайдчейн: бас должен дышать под битом, но не перекачиваться.
    fn sidechain_depth(&self) -> f32 {
        0.36
    }

    fn ensemble(&self) -> &'static [Slot] {
        // Синтезаторы (щипок, пэд), клавишные (Rhodes), барабанная
        // машина (бочка, клэп, хэт, шейкер), бас и вокал. Винила нет:
        // генерация должна быть чистой и «современной», без затертости.
        &[
            Slot::Bass,
            Slot::Kick,
            Slot::Pad,
            Slot::Snare,
            Slot::Pluck,
            Slot::Keys,
            Slot::Shaker,
            Slot::Vocal,
        ]
    }

    fn signature_slots(&self) -> &'static [Slot] {
        // Пэд держит мечтательную подложку, плак поёт тему.
        &[Slot::Pad, Slot::Pluck]
    }

    fn slot_guides(&self) -> [RoleGuide; Slot::COUNT] {
        // Бас: глубокий, но с присутствием. Чистый синус (sub_bias 0.88)
        // на телефоне пропадал совсем — дом-техно слышен именно басом.
        // Теперь синус слышен, но презенс придавлен (0.74): низ мягкий,
        // но бас не исчезает.
        let bass = RoleGuide {
            energy_response: 2.0,
            gates: &[1.5, 2.0, 3.0],
            tie_repeats: 0.65,
            vel_base: 0.78,
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
            sub_bias: 0.74,
            ..RoleGuide::pitched(BASS_W, 0, -1, 1, &[1.5, 2.0, 3.0], 0.78)
        };
        // Бочка: ровная четверка. Толчки редкие и тихие — бит ровный.
        let kick = RoleGuide {
            vel_base: 0.95,
            vel_energy: 0.05,
            poles: Some(KICK_POLES),
            ..RoleGuide::kit(FOUR_FLOOR, 36, 0.95)
        };
        // Пэд: глубокая подушка под всем (октава ниже корня), тише — он
        // фон, а не голос. Наверху подложки мёртвый верх переполняется.
        let pad = RoleGuide {
            gates: &[8.0, 10.0, 12.0],
            vel_base: 0.45,
            vel_jitter: 0.0,
            ..RoleGuide::pitched(PAD_W, 0, -1, 0, &[8.0, 10.0, 12.0], 0.45)
        };
        // Клэп на 2 и 4, с призвуком.
        let snare = RoleGuide {
            vel_base: 0.95,
            poles: Some(SNARE_POLES),
            ..RoleGuide::kit(CLAP_W, 60, 0.9)
        };
        // Хэт: закрытый, короткий, тише шейкера. Звонкий металл на
        // верху убивал характер: в микс шл «треугольник» вместо бита.
        // Ритм держит шейкер — быстрый и сухой.
        let hat = RoleGuide {
            energy_response: 1.2,
            long_offbeats: true,
            gates: &[0.35, 0.7, 1.1],
            vel_base: 0.4,
            vel_energy: 0.3,
            vel_jitter: 0.1,
            poles: Some(HAT_POLES),
            section_poles: true,
            ..RoleGuide::kit(HAT_W, 42, 0.4)
        };
        // Тема: плавная, повторяющаяся. Джиттер 0 — линия цельная,
        // ноты длинные: мелодия льется фразами. Октава ниже: синтезатор
        // наверху спорил с EP и хэтом — линия должна быть в середине.
        let pluck = RoleGuide {
            energy_response: 1.4,
            gates: &[1.5, 2.0, 3.0, 4.0],
            vel_base: 0.4,
            vel_energy: 0.28,
            vel_jitter: 0.05,
            voice: Voicing::Scale {
                degree: 0,
                octave: 1,
                jitter: 0,
            },
            follow_motif: true,
            phrase_rest: 0.15,
            ..RoleGuide::pitched(LEAD_W, 0, 1, 0, &[1.5, 2.0, 3.0, 4.0], 0.4)
        };
        // Rhodes/DX: подпись электро — арпеджио вместо аккорда. Одна
        // ступень за удар, квинта и септима: звук колокольный, как у
        // DX-электропиано. Ноты короткие — EP здесь перкуссивный.
        let keys = RoleGuide {
            energy_response: 1.1,
            gates: &[0.9, 1.2, 1.6],
            vel_base: 0.42,
            vel_jitter: 0.08,
            voice: Voicing::Scale {
                degree: 2,
                octave: 1,
                jitter: 0,
            },
            chord: Some(&[0, 2, 4, 6]),
            arp: true,
            ..RoleGuide::pitched(EP_ARP_W, 2, 1, 0, &[0.9, 1.2, 1.6], 0.42)
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
            ..RoleGuide::pitched(FOUR_FLOOR, 0, -1, 0, &[1.0], 0.95)
        };
        // Вокал: мягкий, чувственный, редкий. Эффект обработки дает
        // мастер (эхо и реверб), сам голос сидит сзади.
        let vocal = RoleGuide {
            energy_response: 1.1,
            gates: &[2.0, 3.0, 4.0, 6.0],
            vel_base: 0.38,
            vel_energy: 0.12,
            vel_jitter: 0.06,
            voice: Voicing::Scale {
                degree: 2,
                octave: 2,
                jitter: 2,
            },
            ..RoleGuide::pitched(VOCAL_W, 2, 2, 2, &[2.0, 3.0, 4.0, 6.0], 0.38)
        };
        // Винил: треск и пыль, постоянная подложка.
        let vinyl = RoleGuide {
            gates: &[200.0],
            vel_base: 0.35,
            vel_jitter: 0.05,
            ..RoleGuide::pitched(BED_W, 0, 0, 0, &[200.0], 0.35)
        };
        // Гитара — ручной гость (духовый элемент для слушателя).
        let guitar = RoleGuide {
            energy_response: 1.1,
            gates: &[1.5, 2.0, 3.0],
            vel_base: 0.35,
            voice: Voicing::Scale {
                degree: 0,
                octave: 1,
                jitter: 2,
            },
            ..RoleGuide::pitched(LEAD_W, 0, 1, 2, &[1.5, 2.0, 3.0], 0.35)
        };
        // Дождь: ровная пелена, тише винила.
        let rain = RoleGuide {
            gates: &[150.0],
            vel_base: 0.3,
            ..RoleGuide::pitched(BED_W, 0, 0, 0, &[150.0], 0.3)
        };
        // Шейкер: песок 16-х поверх хэта.
        let shaker = RoleGuide {
            energy_response: 1.0,
            gates: &[0.4],
            vel_base: 0.62,
            vel_energy: 0.15,
            vel_jitter: 0.08,
            ..RoleGuide::kit(SHAKER_W, 60, 0.62)
        };
        // Соло-гитара — ручной гость.
        let guitar_lead = RoleGuide {
            energy_response: 1.0,
            gates: &[1.0, 1.5, 2.0],
            vel_base: 0.3,
            voice: Voicing::Scale {
                degree: 0,
                octave: 2,
                jitter: 1,
            },
            ..RoleGuide::pitched(BED_W, 0, 2, 1, &[1.0, 1.5, 2.0], 0.3)
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
