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
    // Пыльные тики: акцент на оффбитах, ровные доли тише.
    // Было наоборот (1.0 на каждой доле) — яркий металлический хэт
    // доминировал в миксе (стем-аудит: самый ВЧ голос).
    0.3, 0.0, 0.9, 0.0, 0.3, 0.0, 0.9, 0.0, 0.3, 0.0, 0.9, 0.0, 0.3, 0.0, 0.9, 0.4,
];
const BASS_W: &[f32] = &[
    1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.5, 0.0, 0.0, 0.0, 0.0, 0.0,
];
const PAD_W: &[f32] = &[
    // Пэд — фундамент: якорь на даунбите (дышит приоритетом 0.95),
    // а не редкий гость. Без дрона lofi разваливается на точки.
    1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
];
const KEYS_W: &[f32] = &[
    // Rhodes-компинг: даунбит-якорь + стабы внутри такта.
    // Два хита на такт давали «раз в 3 секунды с обрывом» —
    // пыльность держим редкостью фраз, а не пустотой такта.
    1.0, 0.0, 0.0, 0.45, 0.0, 0.0, 0.5, 0.0, 0.6, 0.0, 0.4, 0.0, 0.0, 0.0, 0.45, 0.0,
];
const BED_W: &[f32] = &[
    1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
];
// Длинная гитара поверх Rhodes: редкие ноты с дыханием.
// База боя — ровные восьмые (ref: Coming Home); даунстроуки на долях —
// якоря (бой слышно всегда), апстроуки заполняют.
const GUITAR_STRUM8_W: &[f32] = &[
    1.0, 0.0, 0.65, 0.0, 1.0, 0.0, 0.65, 0.0, 1.0, 0.0, 0.65, 0.0, 1.0, 0.0, 0.65, 0.4,
];
// Второй бум-бэп: синкопа уехала с 7 на 6, ответ с 10 на 12.
const KICK_BOOM_W: &[f32] = &[
    1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.8, 0.0, 0.0, 0.0, 0.35, 0.0, 0.2, 0.0, 0.0, 0.0,
];
// Халф-тайм: бочка на 1 и 2, воздух вместо суеты.
const KICK_HALF_W: &[f32] = &[
    1.0, 0.0, 0.0, 0.0, 0.7, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
];
// Синкопированный фанк-кик (ref: Redbone — без прямой бочки).
// Разреженный и заякоренный на 1: бум-бэп, а не пулемет.
// (7 частых кандидатов давали кашу на высокой энергии/интенсивности.)
const KICK_SYNCOP_W: &[f32] = &[
    1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.7, 0.0, 0.0, 0.0, 0.6, 0.0, 0.0, 0.0, 0.4, 0.0,
];
const KICK_POLES: &[&[f32]] = &[KICK_W, KICK_BOOM_W, KICK_SYNCOP_W, KICK_HALF_W];
const HAT_EVEN_W: &[f32] = &[
    0.7, 0.45, 0.7, 0.45, 0.7, 0.45, 0.7, 0.45, 0.7, 0.45, 0.7, 0.45, 0.7, 0.45, 0.7, 0.45,
];
// Разреженные оффбиты (ref: Snowman, Redbone — только слабые доли,
// никаких шестнадцатых тиков).
const HAT_SPARSE_W: &[f32] = &[
    0.0, 0.0, 0.9, 0.0, 0.0, 0.0, 0.9, 0.0, 0.0, 0.0, 0.9, 0.0, 0.0, 0.0, 0.9, 0.4,
];
const HAT_POLES: &[&[f32]] = &[HAT_W, HAT_EVEN_W, HAT_SPARSE_W];
// Шейкер: пыльные оффбиты.
const SHAKER_LOFI_W: &[f32] = &[
    0.0, 0.0, 0.7, 0.0, 0.0, 0.0, 0.7, 0.0, 0.0, 0.0, 0.7, 0.0, 0.0, 0.0, 0.7, 0.0,
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
        // Тяжелый свинг референсов (Coming Home 1.69, Autumn ~1.5):
        // бой качается, а не тикает.
        1.55
    }

    fn scale(&self) -> &'static [u8] {
        &SCALE
    }

    fn arrangement(&self) -> crate::arranger::Score {
        crate::arranger::CALM_SCORE
    }

    /// Аранж лофай: пэд и Rhodes держат тему (тема не должна пустовать),
    /// винил и дождь — ровный фон, вокал-чопы — только как гость в
    /// передышке. Дроп у лофай — это не удар, а смена тембра.
    fn arrangement_layers(&self) -> crate::piece::Layers {
        use crate::piece::{default_layers, L_GROOVE, L_HARMONY, L_TEXTURE, L_THEME};
        let base = default_layers();
        [
            // Тема: грув, бас, пампа, тема и текстура.
            L_GROOVE | L_HARMONY | L_THEME | L_TEXTURE,
            // Разгон: добавляется вокал.
            base[1],
            // Удар: тот же состав, что в теме, — у лофай нет «удара».
            L_GROOVE | L_HARMONY | L_THEME | L_TEXTURE,
            // Передышка: грув и текстура, голос отдыхает.
            L_GROOVE | L_TEXTURE,
        ]
    }

    fn transition(&self) -> crate::piece::Transition {
        // Тихий стык: кусок закрывается и следующий начинается с чистого
        // такта. Нахлёт здесь звучал бы как заедание.
        crate::piece::Transition::Cut
    }

    fn reverb_wet(&self) -> f32 {
        0.12
    }

    fn reverb_size(&self) -> f32 {
        0.35
    }

    fn treble_cutoff(&self) -> f32 {
        // Замер против референсов (Autumn 1557, Snowman 824, Redbone 798):
        // наш был 2504 — ярче всех. Темним к середине референсов.
        5200.0
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
        // Классика: барабаны + бас + Rhodes + гитара + винил + вокал-чопы.
        // Нарезок-плака нет: поверх Rhodes и гитары они дают кашу —
        // включить можно вручную, дефолт чистый.
        &[
            Slot::Bass,
            Slot::Kick,
            Slot::Pad,
            Slot::Snare,
            Slot::Hat,
            Slot::Keys,
            Slot::Vinyl,
            Slot::Vocal,
            Slot::Guitar,
        ]
    }

    fn slot_guides(&self) -> [RoleGuide; Slot::COUNT] {
        // Саб-ковер: мягко и длинно, без резких атак. Тише драйва —
        // акцент на гитаре и клавишах, бас только держит низ.
        // Легато вместо чмоканья: длинные ноты (6 шагов) + частичный
        // саб-перекос против дребезга презенса. Тепло оставлено (0.4,
        // не 1.0) — глухой синус вместо баса не нужен.
        // Уровень ниже гитары: замер спектра (Coming Home) — у нас доля
        // низа 0.38 против 0.25 у референса.
        // Пыльный бас ходит корнями (джиттер 1 — соседние ступени).
        let bass = RoleGuide {
            energy_response: 0.8,
            duration_mult: 6.0,
            vel_base: 0.5,
            vel_jitter: 0.05,
            voice: Voicing::Scale {
                degree: 0,
                octave: -1,
                jitter: 1,
            },
            vel_high_cut: 0.6,
            sub_bias: 0.4,
            ..RoleGuide::pitched(BASS_W, 0, -1, 1, 6.0, 0.5)
        };
        let kick = RoleGuide {
            energy_response: 0.8,
            vel_base: 0.9,
            vel_jitter: 0.1,
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
            vel_base: 0.7,
            vel_jitter: 0.06,
            ..RoleGuide::kit(SNARE_W, 38, 0.7)
        };
        let hat = RoleGuide {
            long_offbeats: true,
            long_mult: 5.0,
            duration_mult: 0.3,
            vel_base: 0.34,
            vel_energy: 0.15,
            vel_jitter: 0.08,
            poles: Some(HAT_POLES),
            // Хэт держит сетку полюсов: пульс вместо сетки сбивал ритм.
            ..RoleGuide::kit(HAT_W, 42, 0.34)
        };
        // Нарезки гитары: тот же план боя, что у гитары (общий скелет
        // роли), своим тембром и своим броском гейта. Гетерофония.
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
            phrase_rest: 0.2,
            ..RoleGuide::pitched(GUITAR_STRUM8_W, 0, 1, 0, 2.0, 0.5)
        };
        let keys = RoleGuide {
            duration_mult: 6.0,
            vel_base: 0.6,
            vel_jitter: 0.08,
            voice: Voicing::Scale {
                degree: 0,
                octave: 1,
                jitter: 2,
            },
            ..RoleGuide::pitched(KEYS_W, 0, 1, 2, 6.0, 0.6)
        };
        // Чужие слоты: приглушенные рабочие направляющие — включаются
        // только ручкой фронта, дефолтный характер не ломают.
        // Гость-808 в lofi — саб-дроп на 1, а не дабл паттерна кика:
        // удвоение давало flam и наслоение низа.
        const SUB_DROP_W: &[f32] = &[
            1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
        ];
        let kick808 = RoleGuide {
            vel_base: 0.8,
            ..RoleGuide::kit(SUB_DROP_W, 36, 0.8)
        };
        // Пыль тише музыки: стем-аудит показал винил самым ярким
        // после тёмных клавиш — убавляем подложку, не музыку.
        let vinyl = RoleGuide {
            duration_mult: 300.0,
            vel_base: 0.15,
            vel_jitter: 0.05,
            ..RoleGuide::pitched(BED_W, 0, 0, 0, 300.0, 0.15)
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
        // Гитара поверх Rhodes: мотив сида длинными нотами с дыханием.
        // База — ровные восьмые (бой слышно), апстроуки заполняют.
        // Ведущий голос lofi — чуть громче саба.
        // Громкость выше порога открытых струн (0.6): в теме звонко,
        // в интро глухие ghost-удары. Трезвучие вместо одиночных щипков:
        // уют открытых струн, атака мягкая (голос).
        const STRUM_IV: &[i8] = &[0, 2, 4];
        let guitar = RoleGuide {
            energy_response: 1.0,
            duration_mult: 1.8,
            vel_base: 0.85,
            vel_jitter: 0.08,
            voice: Voicing::Scale {
                degree: 0,
                octave: 1,
                jitter: 0,
            },
            follow_motif: true,
            phrase_rest: 0.2,
            chord: Some(STRUM_IV),
            arp: true,
            ..RoleGuide::pitched(GUITAR_STRUM8_W, 0, 1, 0, 1.8, 0.85)
        };
        // Шейкер вне club: пыльные оффбиты для ручки фронта.
        // Дождь: ровная пелена во все фразы (тише винила и музыки).
        let rain = RoleGuide {
            duration_mult: 150.0,
            vel_base: 0.35,
            ..RoleGuide::pitched(BED_W, 0, 0, 0, 150.0, 0.35)
        };
        let shaker = RoleGuide {
            energy_response: 1.0,
            duration_mult: 0.4,
            vel_base: 0.25,
            vel_jitter: 0.06,
            ..RoleGuide::kit(SHAKER_LOFI_W, 60, 0.25)
        };
        // Соло-гитара вне автомикса: тихий перебор для ручки фронта.
        // Дефолт lofi не тронут (там своя гитара-фингерпикинг).
        let guitar_lead = RoleGuide {
            energy_response: 1.0,
            duration_mult: 1.5,
            vel_base: 0.25,
            voice: Voicing::Scale {
                degree: 0,
                octave: 1,
                jitter: 2,
            },
            ..RoleGuide::pitched(BED_W, 0, 1, 2, 1.5, 0.25)
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
