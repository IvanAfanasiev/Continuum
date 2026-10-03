/// Конечный автомат структуры композиции.
//
// Активные стили гоняют бесконечный круг без финала (Outro убран:
// он противоречит «бесконечной» генерации):
// Intro (раз) -> MainMotif -> BuildUp -> Drop -> PostChorus -> по кругу.
// Спокойные стили дышат темой: Intro (раз) -> MainMotif почти всегда,
// редкие BuildUp/PostChorus для разнообразия.
//
// Мотив сида фиксирован на весь круг после интро: одна и та же тема
// развивается формой, а не сочиняется заново. Круги отличаются длинами
// секций и дыханием энергии, мелодическое ДНК стабильно.
use crate::instruments::InstrumentRole;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SongSection {
    Intro,
    MainMotif,
    BuildUp,
    Drop,
    PostChorus,
}

/// Баланс партий по секциям: насколько громко звучит роль.
/// Текстуры идут ровным слоем всегда; остальное дышит формой.
/// Intro нарастает от тишины. Порядок колонок = порядок
/// дискриминантов [`InstrumentRole`].
const STAGE_GAINS: [[f32; 8]; 5] = [
    // Intro
    [0.45, 0.5, 0.0, 0.5, 0.8, 0.35, 0.3, 1.0],
    // MainMotif
    [0.85, 0.9, 0.8, 0.85, 0.8, 0.75, 0.65, 1.0],
    // BuildUp
    [0.7, 0.9, 0.9, 0.9, 0.7, 0.8, 0.7, 1.0],
    // Drop
    [1.0, 1.0, 1.0, 1.0, 0.8, 1.0, 0.8, 1.0],
    // PostChorus: грув катит дальше (не рестарт!), отдыхает только верх.
    [0.8, 0.9, 0.7, 0.85, 0.75, 0.55, 0.5, 1.0],
];

/// Партитура: интро играется один раз, дальше бесконечный цикл.
#[derive(Debug, Clone, Copy)]
pub struct Score {
    pub intro: &'static [(SongSection, usize)],
    pub cycle: &'static [(SongSection, usize)],
}

/// Полная форма активных стилей: тема → разгон → удар → передышка.
pub const ENERGETIC_SCORE: Score = Score {
    intro: &[(SongSection::Intro, 4)],
    cycle: &[
        (SongSection::MainMotif, 8),
        (SongSection::BuildUp, 4),
        (SongSection::Drop, 8),
        (SongSection::PostChorus, 4),
    ],
};

/// Спокойная форма: тема почти всегда, редкие подъем и передышка
/// для разнообразия — без взрывных дропов по кругу.
pub const CALM_SCORE: Score = Score {
    intro: &[(SongSection::Intro, 4)],
    cycle: &[
        (SongSection::MainMotif, 8),
        (SongSection::MainMotif, 8),
        (SongSection::BuildUp, 4),
        (SongSection::MainMotif, 8),
        (SongSection::PostChorus, 4),
        (SongSection::MainMotif, 8),
    ],
};

/// Порядок входа инструментов в интро — в две волны, и это обоснование,
/// а не случайность: сначала пульс и тема (что играем), потом грув
/// (как качаем). Бочка входит первой везде — heartbeat с первого такта,
/// иначе стили без пэдов стартуют в тишину. Внутри грува порядок
/// поворачивается сидом: у разных сидов музыка «собирается» по-разному.
const EARLY_JOIN: [InstrumentRole; 4] = [
    InstrumentRole::Kick,
    InstrumentRole::Chords,
    InstrumentRole::Texture,
    InstrumentRole::Lead,
];
const LATE_JOIN: [InstrumentRole; 5] = [
    InstrumentRole::Kick,
    InstrumentRole::Hat,
    InstrumentRole::Bass,
    InstrumentRole::Snare,
    InstrumentRole::Vocal,
];

fn smoothstep(x: f32) -> f32 {
    let t = x.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Музыкальная фраза меняется каждые N тактов.
const PHRASE_LENGTH_MEASURES: usize = 2;
/// Сдвиг фраз между кругами: 3 взаимно просто и с 7, и с 5 —
/// мелодия не повторяется ни в 7-ступенных ладах, ни в пентатонике.
const CYCLE_PHRASE_JUMP: usize = 3;

#[derive(Clone)]
pub struct SongContext {
    pub section: SongSection,
    pub measure_index: usize,
    pub phrase_index: usize,
    pub energy: f32,
    pub progress: f32,
    pub is_last_measure: bool,
    /// Ступень круга 0..8 (тема встречается на 1 и 5).
    pub stage: usize,
    /// Номер круга формы (0 = первый проход).
    pub cycle: u64,
    /// Громкости ролей на этой ступени с учетом фейдов краев.
    pub stage_gains: [f32; 8],
    /// Хэш сида: ротация лада и модальный вариант в генерации.
    /// Стабилен внутри сида — тональность не прыгает посреди трека.
    pub seed_hash: u64,
    /// Плотность генерации 0.7..=1.3 от сида: разреженные и густые сиды.
    pub density: f32,
    /// Мотив сида: 16 ступеней лада 0..6 — мелодическое ДНК трека.
    /// Длинная тема через 2 такта (окно фразы) плюс полный диапазон
    /// лада с прыжками: сиды различаются на слух, а не только в нотах.
    /// Создается в интро и НЕ меняется весь круг: один сид = одна
    /// узнаваемая тема, развиваемая формой, а не пересочиняемая.
    pub motif: [u8; 16],
    /// Вторая тема сида: B — вариация A (3 мутации), материал кульминаций.
    /// Drop и PostChorus поют B, остальное A: куплет/припев различаются
    /// темой, а не только громкостью. Границы — границы секций (их длины
    /// от сида), отдельной блочной логики нет.
    pub motif_b: [u8; 16],
    /// B звучит на кульминациях (Drop/PostChorus), A — везде иначе.
    pub use_alt_motif: bool,
    /// Гармоническая прогрессия сида: сдвиги ступеней 0..4 для аккомпанемента,
    /// меняются каждые 2 такта. Лид держит мотив, гармония движется —
    /// сиды отличаются и темой, и гармоническим движением.
    pub prog: [u8; 4],
}

impl SongContext {
    /// Громкость роли на текущей ступени (1.0 = как написано в ноте).
    pub fn stage_gain(&self, role: InstrumentRole) -> f32 {
        self.stage_gains[role.index()]
    }
}

pub struct Arranger {
    score: Score,
    in_intro: bool,
    idx: usize,
    section_measure_count: usize,
    section_len: usize,
    total_measures: usize,
    stages_done: u64,
    seed_hash: u64,
    cycle: u64,
    phrase_offset: usize,
    density: f32,
    motif: [u8; 16],
    motif_b: [u8; 16],
    prog: [u8; 4],
    /// Сглаженные громкости ролей: едут к таблице полпути за такт,
    /// а не прыгают на стыках (инструменты не гаснут/не врываются).
    prev_gains: [f32; 8],
}

/// Детерминированный 64-битный хэш (splitmix64).
pub(crate) fn splitmix64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E3779B97F4A7C15);
    let mut z = x;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
    z ^ (z >> 31)
}

/// Хэш трех чисел в [0, 1).
fn hash01(a: u64, b: u64, c: u64) -> f32 {
    let h = splitmix64(a ^ b.wrapping_mul(0x9E3779B9) ^ c.wrapping_mul(0x85EBCA6B));
    (h >> 32) as f32 / (u32::MAX as f32)
}

/// Гармонические прогрессии (ступени лада 0..4, все диатонично —
/// любая комбинация остается в ладу). Выбор — сидом: аккомпанемент
/// разных сидов движется по-разному при том же мотиве лида.
/// Пул широкий (8): сиды заметно отличаются гармоническим движением.
const PROGS: [[u8; 4]; 8] = [
    [0, 2, 3, 1],
    [0, 3, 2, 3],
    [2, 3, 0, 1],
    [3, 1, 0, 2],
    [1, 3, 0, 2],
    [2, 0, 1, 3],
    [0, 1, 2, 3],
    [3, 0, 2, 1],
];

/// Длина визита в данном круге: база или база + 2 такта.
/// Дропы не варьируются — якоря формы. `salt` уникальна на визит.
fn varied_len(section: SongSection, base: usize, seed_hash: u64, salt: u64) -> usize {
    if section == SongSection::Drop {
        return base;
    }
    let extra = (splitmix64(seed_hash ^ salt) % 2) as usize * 2;
    base + extra
}

/// Громкости секции с нарастанием интро. Первая волна (тема и пелена)
/// входит с начала, грув подключается со второй половины интро
/// в порядке сида.
fn stage_gains_for(section: SongSection, progress: f32, seed_hash: u64) -> [f32; 8] {
    let mut gains = STAGE_GAINS[section as usize];
    // Текстуры (последняя колонка) идут ровным слоем всегда.
    let last = gains.len() - 1;
    let edge = match section {
        SongSection::Intro => 0.45 + 0.55 * progress,
        _ => 1.0,
    };
    // Поворот порядка грува: та же лесенка, другой первый инструмент.
    let rot = (seed_hash % LATE_JOIN.len() as u64) as usize;
    for (i, gain) in gains.iter_mut().enumerate() {
        if i == last {
            continue;
        }
        *gain *= edge;
        // В интро инструменты подключаются постепенно.
        if section == SongSection::Intro {
            if let Some(k) = EARLY_JOIN.iter().position(|r| *r as usize == i) {
                *gain *= smoothstep((progress - k as f32 * 0.06) / 0.15);
            } else if let Some(k) = LATE_JOIN.iter().position(|r| *r as usize == i) {
                let pos = (k + rot) % LATE_JOIN.len();
                *gain *= smoothstep((progress - 0.28 - pos as f32 * 0.07) / 0.18);
            }
        }
    }
    gains
}

impl Arranger {
    pub fn new(seed: u64) -> Self {
        Self::with_score(seed, ENERGETIC_SCORE)
    }

    pub fn with_score(seed: u64, score: Score) -> Self {
        let seed_hash = splitmix64(seed ^ 0x51ED);
        // Мотив сида: цепочка splitmix дает 16 ступеней 0..6 —
        // длинная тема на 2 такта полным диапазоном лада.
        // Создается в интро и живет весь круг неизменной.
        let mut motif = [0u8; 16];
        let mut m = seed_hash ^ 0x9071F;
        for slot in motif.iter_mut() {
            m = splitmix64(m);
            *slot = (m % 7) as u8;
        }
        // Тема B: близкая вариация A — 3 точечные мутации (±ступень-две).
        // Не «вторая композиция», а развитие той же: припев к куплету,
        // а не другой трек. Позиции и сдвиги — от хэша сида.
        // Страж на случай вырождения мутаций в исходную тему.
        let mut motif_b = motif;
        let mut g = splitmix64(seed_hash ^ 0xB17B);
        for _ in 0..3 {
            g = splitmix64(g);
            let pos = (g % 16) as usize;
            g = splitmix64(g);
            let delta = 1 + (g % 2) as u8;
            motif_b[pos] = (motif_b[pos] + delta) % 7;
        }
        if motif_b == motif {
            motif_b[0] = (motif_b[0] + 1) % 7;
        }
        let mut a = Self {
            score,
            in_intro: true,
            idx: 0,
            section_measure_count: 0,
            section_len: 0,
            total_measures: 0,
            stages_done: 0,
            seed_hash,
            cycle: 0,
            // Стартовый сдвиг фраз + плотность — от сида, стабильны весь трек.
            phrase_offset: (seed_hash % 8) as usize,
            density: 0.7 + 0.6 * hash01(seed_hash, 0xD517, 0),
            motif,
            motif_b,
            prog: PROGS[(seed_hash % PROGS.len() as u64) as usize],
            prev_gains: [1.0; 8],
        };
        a.enter_section();
        // Старт без лага: сглаживание начинается с цели первого такта,
        // глиссандо работает только на СМЕНАХ таблицы.
        a.prev_gains = stage_gains_for(a.section(), 1.0 / a.section_len as f32, seed_hash);
        a
    }

    fn current(&self) -> (SongSection, usize) {
        if self.in_intro {
            self.score.intro[self.idx]
        } else {
            self.score.cycle[self.idx]
        }
    }

    /// Вход в секцию: длина и монотонная соль визита.
    fn enter_section(&mut self) {
        let (section, base) = self.current();
        self.section_len = varied_len(section, base, self.seed_hash, self.stages_done);
        self.stages_done += 1;
        self.section_measure_count = 0;
    }

    fn section(&self) -> SongSection {
        self.current().0
    }

    pub fn advance_measure(&mut self) -> SongContext {
        self.section_measure_count += 1;
        self.total_measures += 1;

        if self.section_measure_count > self.section_len {
            // Переход: внутри части — дальше, в конце части —
            // интро разворачивается в цикл, цикл замыкается.
            if self.in_intro {
                if self.idx + 1 < self.score.intro.len() {
                    self.idx += 1;
                } else {
                    self.in_intro = false;
                    self.idx = 0;
                }
            } else if self.idx + 1 < self.score.cycle.len() {
                self.idx += 1;
            } else {
                self.idx = 0;
                self.cycle += 1;
            }
            self.enter_section();
            // enter_section сбросил счетчик — текущий такт первый.
            self.section_measure_count = 1;
        }

        let section = self.section();
        let is_last = self.section_measure_count == self.section_len;
        let progress = self.section_measure_count as f32 / self.section_len as f32;
        let phrase_index = self.total_measures / PHRASE_LENGTH_MEASURES
            + self.phrase_offset
            + (self.cycle as usize).wrapping_mul(CYCLE_PHRASE_JUMP);
        // Глиссандо громкостей: полпути к таблице за такт. Стыки секций
        // текут, а не щелкают — никто не гаснет и не врывается внезапно.
        let target = stage_gains_for(section, progress, self.seed_hash);
        let mut gains = self.prev_gains;
        for (g, t) in gains.iter_mut().zip(target) {
            *g += (t - *g) * 0.5;
        }
        self.prev_gains = gains;

        SongContext {
            section,
            measure_index: self.section_measure_count,
            phrase_index,
            energy: energy_for(
                section,
                progress,
                self.cycle,
                self.seed_hash,
                self.stages_done,
            ),
            progress,
            is_last_measure: is_last,
            stage: self.stages_done as usize,
            cycle: self.cycle,
            stage_gains: gains,
            seed_hash: self.seed_hash,
            density: self.density,
            motif: self.motif,
            motif_b: self.motif_b,
            // Тема секции: кульминации (Drop/PostChorus) поют B,
            // остальное — A. Энергия и тема усиливают друг друга:
            // настоящий припев, а не та же мелодия погромче.
            use_alt_motif: matches!(
                section,
                SongSection::Drop | SongSection::PostChorus
            ),
            prog: self.prog,
        }
    }
}

/// Открытость фильтра формы 0..1: движок домножает на нее срез верхов.
/// Интро приоткрывается по мере входа инструментов, тема держит середину,
/// билд свистит вверх весь разгон и раскрывается на дропе, передышка
/// прикрывается обратно. Чистая функция — тестируется без аудио.
pub fn section_openness(section: SongSection, progress: f32) -> f32 {
    let p = progress.clamp(0.0, 1.0);
    match section {
        SongSection::Intro => 0.15 + 0.25 * p,
        SongSection::MainMotif => 0.4,
        SongSection::BuildUp => 0.4 + 0.6 * p,
        SongSection::Drop => 1.0,
        SongSection::PostChorus => 0.3,
    }
}

fn energy_for(section: SongSection, progress: f32, cycle: u64, seed_hash: u64, salt: u64) -> f32 {
    let base = match section {
        SongSection::Intro => 0.2 + progress * 0.2,
        SongSection::MainMotif => 0.55,
        SongSection::BuildUp => 0.6 + progress * 0.4,
        SongSection::Drop => return 1.0,
        SongSection::PostChorus => 0.45 - progress * 0.1,
    };
    // Легкое дыхание энергии по кругам: форма та же, накал чуть разный.
    // Дроп — якорь, всегда ровно 1.0.
    let wobble = (hash01(seed_hash, cycle, salt) - 0.5) * 0.15;
    (base + wobble).clamp(0.05, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_opens_toward_drop() {
        // Свип формы: интро растет, билд доходит до 1.0, дроп открыт,
        // передышка ниже темы.
        assert!(section_openness(SongSection::Intro, 0.0) < section_openness(SongSection::Intro, 1.0));
        assert_eq!(section_openness(SongSection::BuildUp, 1.0), 1.0);
        assert_eq!(section_openness(SongSection::Drop, 0.5), 1.0);
        assert!(
            section_openness(SongSection::PostChorus, 0.0)
                < section_openness(SongSection::MainMotif, 0.0)
        );
    }

    #[test]
    fn full_cycle_visits_every_section_no_outro() {
        // Круг активных: интро, тема, разгон, дроп, передышка — и никаких
        // финалов: генерация бесконечная.
        let mut a = Arranger::new(1);
        let mut seen = std::collections::HashSet::new();
        for _ in 0..96 {
            seen.insert(a.advance_measure().section);
        }
        for s in [
            SongSection::Intro,
            SongSection::MainMotif,
            SongSection::BuildUp,
            SongSection::Drop,
            SongSection::PostChorus,
        ] {
            assert!(seen.contains(&s), "section {s:?} never visited");
        }
    }

    #[test]
    fn stages_follow_the_score() {
        // Один проход круга: тема → разгон → дроп → передышка.
        let mut a = Arranger::new(1);
        let mut order = Vec::new();
        let mut last = SongSection::PostChorus;
        let mut first = true;
        for _ in 0..200 {
            let ctx = a.advance_measure();
            if ctx.cycle > 0 {
                break;
            }
            if first || ctx.section != last {
                order.push(ctx.section);
                last = ctx.section;
                first = false;
            }
        }
        use SongSection::*;
        assert_eq!(
            order,
            vec![Intro, MainMotif, BuildUp, Drop, PostChorus],
            "{order:?}"
        );
    }

    #[test]
    fn calm_score_breathes_main_with_rare_variety() {
        // Спокойная форма: интро один раз, дальше почти всегда тема,
        // подъем и передышка — редкие гости.
        let mut a = Arranger::with_score(1, CALM_SCORE);
        let mut order = Vec::new();
        let mut last = SongSection::Drop;
        let mut first = true;
        for _ in 0..100 {
            let ctx = a.advance_measure();
            if first || ctx.section != last {
                order.push(ctx.section);
                last = ctx.section;
                first = false;
            }
        }
        use SongSection::*;
        let intros = order.iter().filter(|s| **s == Intro).count();
        assert_eq!(intros, 1, "intro must play once: {order:?}");
        // Смежные повторы темы сливаются в один визит.
        assert_eq!(
            &order[..6],
            &[Intro, MainMotif, BuildUp, MainMotif, PostChorus, MainMotif],
            "{order:?}"
        );
        let mains = order.iter().filter(|s| **s == MainMotif).count();
        let builds = order.iter().filter(|s| **s == BuildUp).count();
        assert!(mains > builds * 2, "calm must breathe main: {order:?}");
    }

    #[test]
    fn main_gains_stable_across_visits() {
        // Без реприз: устоявшийся баланс темы одинаков во всех визитах.
        // Сравниваем последние такты визитов (глиссандо уже доехало).
        let mut a = Arranger::new(1);
        let mut mains = Vec::new();
        for _ in 0..200 {
            let ctx = a.advance_measure();
            if ctx.cycle > 0 {
                break;
            }
            if ctx.section == SongSection::MainMotif && ctx.is_last_measure {
                mains.push(ctx.stage_gains);
            }
        }
        assert!(!mains.is_empty(), "no mains visited");
        for gains in &mains[1..] {
            assert_eq!(*gains, mains[0], "main balance drifted");
        }
    }

    #[test]
    fn groove_continues_through_postchorus() {
        // Передышка — не рестарт: ритм-секция катит дальше,
        // отдыхает только верх (лид/вокал).
        let mut a = Arranger::new(1);
        for _ in 0..200 {
            let ctx = a.advance_measure();
            if ctx.cycle > 0 {
                break;
            }
            if ctx.section == SongSection::PostChorus && ctx.is_last_measure {
                // Устоявшаяся передышка (глиссандо доехало): ритм катит,
                // верх отдыхает. Первый такт переходный — не меряем.
                let g = ctx.stage_gains;
                let kick = InstrumentRole::Kick.index();
                let hat = InstrumentRole::Hat.index();
                let lead = InstrumentRole::Lead.index();
                assert!(g[kick] >= 0.8, "kick stopped in post: {}", g[kick]);
                assert!(g[hat] >= 0.8, "hat stopped in post: {}", g[hat]);
                assert!(g[lead] < 0.7, "lead did not rest: {}", g[lead]);
                return;
            }
        }
        panic!("postchorus never visited");
    }

    #[test]
    fn postchorus_is_a_breather() {
        // Передышка тише дропа: круг дышит, а не лупит в полку.
        let mut a = Arranger::new(1);
        let mut drop_sum = None;
        let mut post_sum = None;
        for _ in 0..200 {
            let ctx = a.advance_measure();
            if ctx.cycle > 0 {
                break;
            }
            let sum: f32 = ctx.stage_gains[..7].iter().sum();
            match ctx.section {
                SongSection::Drop if drop_sum.is_none() => {
                    drop_sum = Some(sum);
                }
                SongSection::PostChorus if post_sum.is_none() => {
                    post_sum = Some(sum);
                }
                _ => {}
            }
        }
        assert!(
            post_sum.unwrap() < drop_sum.unwrap(),
            "postchorus must dip after drop"
        );
    }

    #[test]
    fn same_seed_same_song() {
        let mut a = Arranger::new(777);
        let mut b = Arranger::new(777);
        for _ in 0..20 {
            let x = a.advance_measure();
            let y = b.advance_measure();
            assert_eq!(x.phrase_index, y.phrase_index);
            assert_eq!(x.energy, y.energy);
            assert_eq!(x.cycle, y.cycle);
            assert_eq!(x.motif, y.motif);
            assert_eq!(x.motif_b, y.motif_b);
            assert_eq!(x.use_alt_motif, y.use_alt_motif);
        }
    }

    #[test]
    fn alt_motif_marks_drop_sections() {
        // B — материал кульминаций: Drop и PostChorus поют B, остальное A.
        // Куплет/припев различаются темой, а не только громкостью.
        for seed in [0u64, 1, 7, 42, 777, 424242] {
            let mut a = Arranger::new(seed);
            let first = a.advance_measure();
            assert_ne!(first.motif, first.motif_b, "seed {seed}: A == B");
            assert!(!first.use_alt_motif, "intro must start with A");
            for v in first.motif_b {
                assert!(v <= 6, "motif_b out of range: {v}");
            }
            for v in first.motif {
                assert!(v <= 6, "motif out of range: {v}");
            }
        }
        let mut a = Arranger::new(5);
        let (mut main_a, mut drop_b, mut post_b) = (false, false, false);
        for _ in 0..200 {
            let ctx = a.advance_measure();
            match ctx.section {
                SongSection::Drop => {
                    assert!(ctx.use_alt_motif, "drop must sing B");
                    drop_b = true;
                }
                SongSection::PostChorus => {
                    assert!(ctx.use_alt_motif, "postchorus must sing B");
                    post_b = true;
                }
                _ => {
                    assert!(!ctx.use_alt_motif, "{:?} must sing A", ctx.section);
                    if ctx.section == SongSection::MainMotif {
                        main_a = true;
                    }
                }
            }
            if main_a && drop_b && post_b {
                break;
            }
        }
        assert!(main_a && drop_b && post_b, "sections never complete a cycle");
        // Границы детерминированы сидом.
        let mut x = Arranger::new(5);
        let mut y = Arranger::new(5);
        for _ in 0..40 {
            assert_eq!(
                x.advance_measure().use_alt_motif,
                y.advance_measure().use_alt_motif,
                "theme boundary must be deterministic"
            );
        }
    }

    #[test]
    fn drop_has_full_energy() {
        let mut a = Arranger::new(0);
        let mut found_drop = false;
        for _ in 0..96 {
            let ctx = a.advance_measure();
            if ctx.section == SongSection::Drop {
                assert_eq!(ctx.energy, 1.0);
                found_drop = true;
            }
        }
        assert!(found_drop);
    }

    #[test]
    fn cycles_advance_and_vary_lengths() {
        // Круги идут вперед, а длины темы между кругами различаются:
        // форма не повторяется дословно.
        let mut a = Arranger::new(1);
        let mut max_cycle = 0;
        let mut main_lens = std::collections::HashSet::new();
        let mut current_main = 0;
        let mut in_main = false;
        for _ in 0..300 {
            let ctx = a.advance_measure();
            max_cycle = max_cycle.max(ctx.cycle);
            if ctx.section == SongSection::MainMotif {
                if !in_main {
                    if current_main > 0 {
                        main_lens.insert(current_main);
                    }
                    current_main = 0;
                    in_main = true;
                }
                current_main += 1;
            } else {
                in_main = false;
            }
        }
        assert!(max_cycle >= 2, "cycles do not advance: {max_cycle}");
        assert!(
            main_lens.len() > 1,
            "main length never varies: {main_lens:?}"
        );
    }

    #[test]
    fn intro_joins_instruments_gradually() {
        // В начале интро звучат единицы, к концу — почти все:
        // инструменты подключаются постепенно.
        let mut a = Arranger::new(1);
        let mut first_count = None;
        let mut last_count = None;
        for _ in 0..200 {
            let ctx = a.advance_measure();
            if ctx.cycle > 0 {
                break;
            }
            if ctx.section == SongSection::Intro {
                let n = ctx.stage_gains[..7].iter().filter(|g| **g > 0.3).count();
                if first_count.is_none() {
                    first_count = Some(n);
                }
                last_count = Some(n);
            }
        }
        assert!(
            last_count.unwrap() > first_count.unwrap(),
            "intro must build up"
        );
    }

    #[test]
    fn intro_builds_theme_before_groove() {
        // Первый такт интро: heartbeat бочки уже стучит,
        // остальной грув (бас/снейр/хэт) еще молчит.
        // Порядок: пульс, потом мясо.
        let mut a = Arranger::new(3);
        let ctx = a.advance_measure();
        assert_eq!(ctx.section, SongSection::Intro);
        let g = ctx.stage_gains;
        let bass = InstrumentRole::Bass.index();
        let kick = InstrumentRole::Kick.index();
        let snare = InstrumentRole::Snare.index();
        let hat = InstrumentRole::Hat.index();
        assert!(g[kick] > 0.1, "no heartbeat: {}", g[kick]);
        assert!(
            g[bass] < 0.05 && g[snare] < 0.05 && g[hat] < 0.05,
            "groove leaked early: bass={} snare={} hat={}",
            g[bass],
            g[snare],
            g[hat]
        );
    }

    #[test]
    fn density_stays_in_range() {
        for seed in 0..20 {
            let d = Arranger::new(seed).advance_measure().density;
            assert!(
                (0.7..=1.3).contains(&d),
                "seed {seed}: density {d} out of range"
            );
        }
    }

    #[test]
    fn progressions_differ_between_seeds() {
        // Гармоническое движение — от сида: разные сиды ведут
        // аккомпанемент по-разному при узнаваемом мотиве.
        let progs: std::collections::HashSet<_> =
            (0..16u64).map(|s| Arranger::new(s).advance_measure().prog).collect();
        assert!(progs.len() > 1, "progression never varies");
    }

    #[test]
    fn same_seed_same_progression() {
        // Прогрессия стабильна внутри сида — гармония не прыгает.
        let a = Arranger::new(4242).advance_measure().prog;
        let b = Arranger::new(4242).advance_measure().prog;
        assert_eq!(a, b);
        for step in a {
            assert!(step <= 4, "prog step out of scale range: {step}");
        }
    }

    #[test]
    fn motifs_differ_between_seeds() {
        let motifs: std::collections::HashSet<_> = (0..16u64)
            .map(|s| Arranger::new(s).advance_measure().motif)
            .collect();
        assert!(motifs.len() > 3, "motifs do not vary");
    }
}
