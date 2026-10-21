//! Написанная форма куска: 30–60 секунд, с финалом и швом в следующий.
//!
//! Раньше форма была бесконечным кольцом секций фиксированного скелета
//! (`Score.cycle`): слушатель слышал один и тот же круг снова и снова, а
//! рисунок пере-rollивался каждые два такта. Отсюда ощущение «рандомных
//! кусков» — крупного плана (цель, разгон, посадка, финал) не было вовсе.
//!
//! Теперь форма пишется кусками:
//! - **тело** куска — 30–60 секунд по скелету стиля, с варьированными
//!   длинами и темой (A/B) по секциям;
//! - **шов** — два такта в конце: написанный финал (каданс, голоса
//!   сворачиваются) и запуск следующего куска. Шов планируется вместе со
//!   следующим куском, поэтому стык не слышен: это не кроссфейд, а
//!   написанный стык.
//!
//! Аранж по слоям: план пишет, какие группы инструментов играют в
//! каждой секции (грув, пампа, тема, вокал, текстуры). Молчащая группа
//! молчит по-настоящему — точки на колесе и звук совпадают, потому что
//! решение одно и то же.
//!
//! План — чистая функция от (сид, стиль, номер куска, такт внутри куска),
//! поэтому остаток куска можно перепланировать на лету (живая смена сида
//! или стиля вступает с следующего такта).

use crate::arranger::{splitmix64, SongSection};
use crate::instruments::Group;

/// Маска слоев аранжировки: бит = группа инструментов, играющая в секции.
/// Слои — это не роли, а «партии»: грув (ударные и бас), пампа, тема,
/// вокал, текстуры. План куска пишет, кто играет в каждой секции:
/// это и есть аранж, а не бросок монет на каждом шаге.
pub type LayerMask = u8;

/// Аранж стиля: кто играет в каждой из четырех секций (порядок
/// [`LAYER_ORDER`]). Стиль решает сам: у рока рифф остается в
/// передышке, в клубе вокал молчит до дропа, в лофай пэд держит тему.
/// Это данные, а не код: правка стиля — правка таблицы.
pub type Layers = [LayerMask; 4];

/// Порядок секций в таблице аранжа.
pub const LAYER_ORDER: [SongSection; 4] = [
    SongSection::MainMotif,
    SongSection::BuildUp,
    SongSection::Drop,
    SongSection::PostChorus,
];

/// Индекс секции в таблице аранжа.
pub fn layer_index(section: SongSection) -> usize {
    match section {
        SongSection::MainMotif => 0,
        SongSection::BuildUp => 1,
        SongSection::Drop => 2,
        SongSection::PostChorus => 3,
    }
}

/// Грув: бочка, снейр, хэт, бас — то, что держит такт.
pub const L_GROOVE: LayerMask = (1 << Group::Kick.index())
    | (1 << Group::Snare.index())
    | (1 << Group::Hat.index())
    | (1 << Group::Bass.index());
/// Пампа: аккорды (клавиши, пэд).
pub const L_HARMONY: LayerMask = 1 << Group::Chords.index();
/// Тема: верхний голос (лид, флейта, формула).
pub const L_THEME: LayerMask = 1 << Group::Lead.index();
/// Вокал: отдельный слой (он же — эффект в дропе).
pub const L_VOCAL: LayerMask = 1 << Group::Vocal.index();
/// Текстуры (винил, дождь): ровный слой всегда.
pub const L_TEXTURE: LayerMask = 1 << Group::Texture.index();

/// Маска секции по её виду: кто играет в теме, разгоне, дропе, передышке.
/// Общий аранж на стиль — стили переопределяют таблицу у себя.
pub fn layers_for(section: SongSection) -> LayerMask {
    match section {
        // Тема: грув + пампа + тема. Эффекты и вокал молчат — их вход
        // на дропе слышен как событие, а не как фон.
        SongSection::MainMotif => L_GROOVE | L_HARMONY | L_THEME | L_TEXTURE,
        // Разгон: всё, включая эффекты, — набор растет.
        SongSection::BuildUp => L_GROOVE | L_HARMONY | L_THEME | L_VOCAL | L_TEXTURE,
        // Удар: весь состав.
        SongSection::Drop => L_GROOVE | L_HARMONY | L_THEME | L_VOCAL | L_TEXTURE,
        // Передышка: грув катит дальше, верх отдыхает.
        SongSection::PostChorus => L_GROOVE | L_HARMONY | L_TEXTURE,
    }
}

/// Аранж по общему правилу (тема, разгон, удар, передышка).
pub fn default_layers() -> Layers {
    [
        layers_for(SongSection::MainMotif),
        layers_for(SongSection::BuildUp),
        layers_for(SongSection::Drop),
        layers_for(SongSection::PostChorus),
    ]
}

/// Маска шва (финала куска): только грув и текстура. Верхние голоса
/// сворачиваются — сюда входит голос следующего куска, и стык получается
/// плавным, а не провалом.
pub const L_JOINT: LayerMask = L_GROOVE | L_TEXTURE;

/// Как стиль склеивает куски.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transition {
    /// Плавный: голос следующего куска входит поверх грува текущего
    /// (нахлёст на хвосте куска). Грув не прерывается.
    Rolling { bars: usize },
    /// Резкий: кусок обрывается на границе такта, следующий начинается
    /// сразу. Тихому стилю — тихий стык.
    Cut,
}

impl Transition {
    /// Длина шва (в тактах) для этого перехода.
    pub fn joint_bars(&self) -> usize {
        match self {
            Transition::Rolling { bars } => (*bars).max(1),
            // Резкий стык: короткий хвост (один такт), дальше — сразу
            // следующий кусок с полной энергией.
            Transition::Cut => 1,
        }
    }

    /// Налёт: сколько тактов следующего куска уже поёт голос тени,
    /// пока основной арранжер доводит шов. Для резкого стыка налёта
    /// нет — куски не перекрываются.
    pub fn overlap_bars(&self) -> usize {
        match self {
            Transition::Rolling { bars } => (*bars).max(1),
            Transition::Cut => 0,
        }
    }
}

/// Секция куска: что играет, сколько и с какой энергией.
#[derive(Clone, Debug)]
pub struct SectionPlan {
    pub section: SongSection,
    /// Базовое число тактов.
    pub bars: usize,
    /// Длина варьируется по сиду (бесконечный круг) или фиксирована
    /// (написанная форма куска).
    pub vary: bool,
    /// Базовая энергия секции: `None` — по общей формуле формы,
    /// `Some` — написано в плане.
    pub energy: Option<f32>,
    /// Кто играет в секции (маска групп).
    pub layers: LayerMask,
}

impl SectionPlan {
    pub fn new(section: SongSection, bars: usize) -> Self {
        Self {
            section,
            bars,
            vary: false,
            energy: None,
            layers: layers_for(section),
        }
    }
}

/// Кусок: тело (30–60 с) + шов (финал и запуск следующего).
#[derive(Clone, Debug)]
pub struct PiecePlan {
    /// Тело куска: секции по порядку.
    pub body: Vec<SectionPlan>,
    /// Шов: финал текущего куска и вход в следующий. Играется после
    /// тела, поэтому последний такт шва — каданс (тема сходит на тонику).
    pub joint: SectionPlan,
}

impl PiecePlan {
    /// Число тактов куска (тело + шов).
    pub fn bars(&self) -> usize {
        self.body.iter().map(|s| s.bars).sum::<usize>() + self.joint.bars
    }
}

/// Целевая длительность куска, с. Куски короче 30 с читаются как
/// отдельные номера, длиннее 60 — как один длинный трек.
pub const TARGET_PIECE_SECS: f32 = 42.0;
/// Границы цели: короче — кусок не успевает стать формой, длиннее —
/// слушатель успевает забыть начало.
const MIN_PIECE_SECS: f32 = 30.0;
const MAX_PIECE_SECS: f32 = 60.0;

/// Скелет стиля → план куска.
///
/// `skeleton` — последовательность секций стиля (`Score.cycle`).
/// `index` — номер куска в потоке: куски отличаются друг от друга
/// (длины секций, энергия), но детерминированно.
/// `start_bar` — сколько тактов куска уже отыграно: план обрезается с
/// этого места (перепланирование остатка при живой смене сида/стиля —
/// вступает с следующего такта).
/// `transition` — как стиль склеивает куски.
pub fn plan_piece(
    seed: u64,
    bpm: f32,
    skeleton: &[(SongSection, usize)],
    index: u64,
    start_bar: usize,
    transition: Transition,
    layers: Layers,
) -> PiecePlan {
    let salt = splitmix64(seed ^ 0x9E37_79B9 ^ index.wrapping_mul(0x2545_F491));
    let bar_secs = if bpm > 1.0 { 4.0 * 60.0 / bpm } else { 2.0 };
    let joint_bars = transition.joint_bars();
    // Границы считаем для тела: шов — часть куска.
    let target_bars =
        (((TARGET_PIECE_SECS / bar_secs).round() as usize).clamp(8, 48)).saturating_sub(joint_bars);
    let min_bars =
        (((MIN_PIECE_SECS / bar_secs).round() as usize).clamp(4, 40)).saturating_sub(joint_bars);
    let max_bars =
        (((MAX_PIECE_SECS / bar_secs).round() as usize).clamp(8, 64)).saturating_sub(joint_bars);

    let mut body: Vec<SectionPlan> = Vec::new();
    let mut total = 0usize;
    // Скелет может быть короче цели (или длиннее) — идём по нему циклом
    // и набираем нужное число тактов. Порядок секций при этом сохраняется:
    // тема → разгон → удар → передышка, а не случайная сборка.
    let mut round = 0usize;
    while total < min_bars && !skeleton.is_empty() {
        for (section, bars) in skeleton {
            let vary = *section != SongSection::Drop;
            // Длина секции в куске: база плюс 0–2 такта от сида (круг
            // варьировался так же — здесь это стало частью плана).
            let extra = if vary {
                (splitmix64(salt ^ (body.len() as u64).wrapping_mul(0x9E37_79B9)) % 2) as usize * 2
            } else {
                0
            };
            let mut len = bars + extra;
            // Секция не влезает в кусок — подрезаем её (ровно на пары
            // тактов: фразы остаются целыми) или пропускаем.
            if total + len > max_bars {
                let room = max_bars.saturating_sub(total);
                len = room - room % 2;
                if len < 2 {
                    break;
                }
            }
            body.push(SectionPlan {
                section: *section,
                bars: len,
                vary: false,
                energy: None,
                layers: layers[layer_index(*section)],
            });
            total += len;
            if total >= target_bars {
                break;
            }
        }
        round += 1;
        if round > 8 {
            break;
        }
    }
    if body.is_empty() {
        body.push(SectionPlan::new(SongSection::MainMotif, 8));
    }
    // Шов: написанный финал куска. Грув катит дальше, верхние голоса
    // сворачиваются — сюда входит голос следующего куска, и стык
    // получается плавным, а не провалом. Для резкого перехода шов
    // короче и с полным составом (удар — и сразу следующий кусок).
    let joint = match transition {
        Transition::Rolling { .. } => SectionPlan {
            section: SongSection::PostChorus,
            bars: joint_bars,
            vary: false,
            // Энергия шва — посадка, но не провал: грув (кик, снейр, хэт)
            // держится, иначе в стыке зияет дыра. Верх сворачивает
            // маска слоев, а не энергия.
            energy: Some(0.62),
            layers: L_JOINT,
        },
        // Резкий стык: короткий хвост в полном составе (удар) — дальше
        // следующий кусок сразу с полной энергией.
        Transition::Cut => SectionPlan {
            section: SongSection::PostChorus,
            bars: joint_bars,
            vary: false,
            energy: Some(0.55),
            layers: L_GROOVE | L_HARMONY | L_THEME | L_TEXTURE,
        },
    };
    // Остаток куска: отрезаем уже отыгранные такты. Перепланирование
    // остатка (живая смена сида/стиля) вступает с следующего такта.
    if start_bar > 0 {
        let mut left = start_bar;
        body.retain_mut(|s| {
            if left == 0 {
                return true;
            }
            if left >= s.bars {
                left -= s.bars;
                false
            } else {
                s.bars -= left;
                left = 0;
                true
            }
        });
    }
    PiecePlan { body, joint }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arranger::{CALM_SCORE, ENERGETIC_SCORE};

    fn rolling() -> Transition {
        Transition::Rolling { bars: 2 }
    }

    #[test]
    fn piece_is_30_to_60_seconds() {
        for (name, bpm, skeleton) in [
            ("club", 126.0, ENERGETIC_SCORE.cycle),
            ("lofi", 82.0, CALM_SCORE.cycle),
        ] {
            for index in 0..8u64 {
                let plan = plan_piece(42, bpm, skeleton, index, 0, rolling(), default_layers());
                let secs = plan.bars() as f32 * 4.0 * 60.0 / bpm;
                assert!(
                    (30.0..=60.0).contains(&secs),
                    "{name} кусок {index}: {secs:.1} с ({} тактов)",
                    plan.bars()
                );
            }
        }
    }

    #[test]
    fn piece_ends_with_a_joint() {
        // Финал есть у каждого куска: последние такты — шов.
        let plan = plan_piece(
            42,
            126.0,
            ENERGETIC_SCORE.cycle,
            0,
            0,
            rolling(),
            default_layers(),
        );
        assert_eq!(plan.joint.bars, 2);
        assert!(
            plan.joint.energy.is_some(),
            "шов должен иметь написанную энергию"
        );
        assert!(plan.body.len() >= 3, "тело куска слишком короткое");
    }

    #[test]
    fn joint_leaves_room_for_the_next_piece() {
        // В шве верхние голоса молчат: туда входит голос следующего
        // куска — иначе стык режет фразу.
        let plan = plan_piece(
            42,
            126.0,
            ENERGETIC_SCORE.cycle,
            0,
            0,
            rolling(),
            default_layers(),
        );
        assert_eq!(plan.joint.layers & L_THEME, 0, "в шве не должно быть темы");
        assert_ne!(plan.joint.layers & L_GROOVE, 0, "в шве должен идти грув");
    }

    #[test]
    fn sharp_transition_does_not_overlap() {
        // Резкий стык: короткий хвост, куски не перекрываются —
        // следующий начинается сразу, без нахлеста.
        let plan = plan_piece(
            42,
            126.0,
            ENERGETIC_SCORE.cycle,
            0,
            0,
            Transition::Cut,
            default_layers(),
        );
        assert_eq!(plan.joint.bars, 1);
        assert_eq!(Transition::Cut.overlap_bars(), 0);
        assert_eq!(Transition::Rolling { bars: 2 }.overlap_bars(), 2);
        // В шве резкого стыка состав полный: это удар, а не пустота.
        assert_ne!(plan.joint.layers & L_THEME, 0);
    }

    #[test]
    fn arrangement_writes_who_plays() {
        // Аранж по слоям: в теме эффектов нет, в дропе — весь состав.
        assert_eq!(
            layers_for(SongSection::MainMotif) & L_VOCAL,
            0,
            "в теме вокал молчит"
        );
        assert_ne!(
            layers_for(SongSection::Drop) & L_VOCAL,
            0,
            "в дропе вокал есть"
        );
        assert_eq!(
            layers_for(SongSection::PostChorus) & L_THEME,
            0,
            "в передышке тема отдыхает"
        );
    }

    #[test]
    fn pieces_differ_between_themselves() {
        // Соседние куски не повторяются дословно: длина секций и набор
        // отличаются (иначе поток снова станет «кольцом»).
        let a = plan_piece(
            42,
            126.0,
            ENERGETIC_SCORE.cycle,
            0,
            0,
            rolling(),
            default_layers(),
        );
        let b = plan_piece(
            42,
            126.0,
            ENERGETIC_SCORE.cycle,
            1,
            0,
            rolling(),
            default_layers(),
        );
        assert_ne!(
            a.body.iter().map(|s| s.bars).collect::<Vec<_>>(),
            b.body.iter().map(|s| s.bars).collect::<Vec<_>>(),
            "куски не отличаются"
        );
    }

    #[test]
    fn same_seed_same_piece() {
        let a = plan_piece(
            7,
            126.0,
            ENERGETIC_SCORE.cycle,
            3,
            0,
            rolling(),
            default_layers(),
        );
        let b = plan_piece(
            7,
            126.0,
            ENERGETIC_SCORE.cycle,
            3,
            0,
            rolling(),
            default_layers(),
        );
        assert_eq!(
            a.body.iter().map(|s| s.bars).collect::<Vec<_>>(),
            b.body.iter().map(|s| s.bars).collect::<Vec<_>>()
        );
    }

    #[test]
    fn remainder_is_planned_from_the_current_bar() {
        // Живая смена сида/стиля вступает с следующего такта: остаток
        // куска перепланируется, отыгранное — не трогаем.
        let full = plan_piece(
            42,
            126.0,
            ENERGETIC_SCORE.cycle,
            0,
            0,
            rolling(),
            default_layers(),
        );
        let rest = plan_piece(
            42,
            126.0,
            ENERGETIC_SCORE.cycle,
            0,
            8,
            rolling(),
            default_layers(),
        );
        let full_bars: usize = full.body.iter().map(|s| s.bars).sum();
        let rest_bars: usize = rest.body.iter().map(|s| s.bars).sum();
        assert_eq!(
            full_bars - rest_bars,
            8,
            "остаток должен быть короче на отыгранное"
        );
        // Шов (финал) в остатке остается: кусок по-прежнему доходит
        // до посадки.
        assert_eq!(rest.joint.bars, 2);
    }

    #[test]
    fn calm_style_piece_breathes() {
        // Спокойный скелет: тема dominates, подъем и передышка — гости.
        let plan = plan_piece(7, 82.0, CALM_SCORE.cycle, 0, 0, rolling(), default_layers());
        let mains = plan
            .body
            .iter()
            .filter(|s| s.section == SongSection::MainMotif)
            .count();
        assert!(mains >= 2, "кусок без темы: {:?}", plan.body);
    }
}
