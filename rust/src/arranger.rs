/// Конечный автомат структуры композиции.
//
// Активные стили гоняют бесконечный круг без финала и без разогрева
// (Outro противоречит «бесконечной» генерации, Intro — требованию
// играть сразу в полную): MainMotif -> BuildUp -> Drop -> PostChorus
// -> по кругу. Первый такт — уже тема в полную силу.
// Спокойные стили дышат темой: MainMotif почти всегда,
// редкие BuildUp/PostChorus для разнообразия.
//
// Мотив сида фиксирован на весь круг: одна и та же тема
// развивается формой, а не сочиняется заново. Круги отличаются длинами
// секций и дыханием энергии, мелодическое ДНК стабильно.
use crate::instruments::InstrumentRole;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SongSection {
    MainMotif,
    BuildUp,
    Drop,
    PostChorus,
}

/// Баланс партий по секциям: насколько громко звучит роль.
/// Текстуры идут ровным слоем всегда; остальное дышит формой.
/// Порядок колонок = порядок дискриминантов [`InstrumentRole`].
const STAGE_GAINS: [[f32; 8]; 4] = [
    // MainMotif
    [0.85, 0.9, 0.8, 0.85, 0.8, 0.75, 0.65, 1.0],
    // BuildUp
    [0.7, 0.9, 0.9, 0.9, 0.7, 0.8, 0.7, 1.0],
    // Drop
    [1.0, 1.0, 1.0, 1.0, 0.8, 1.0, 0.8, 1.0],
    // PostChorus: грув катит дальше (не рестарт!), отдыхает только верх.
    [0.8, 0.9, 0.7, 0.85, 0.75, 0.55, 0.5, 1.0],
];

/// Партитура: бесконечный цикл секций без разогрева и финала.
#[derive(Debug, Clone, Copy)]
pub struct Score {
    pub cycle: &'static [(SongSection, usize)],
}

/// Полная форма активных стилей: тема → разгон → удар → передышка.
pub const ENERGETIC_SCORE: Score = Score {
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
    cycle: &[
        (SongSection::MainMotif, 8),
        (SongSection::MainMotif, 8),
        (SongSection::BuildUp, 4),
        (SongSection::MainMotif, 8),
        (SongSection::PostChorus, 4),
        (SongSection::MainMotif, 8),
    ],
};

/// Музыкальная фраза меняется каждые N тактов.
const PHRASE_LENGTH_MEASURES: usize = 2;
/// Сдвиг фраз между кругами: 3 взаимно просто и с 7, и с 5 —
/// мелодия не повторяется ни в 7-ступенных ладах, ни в пентатонике.
const CYCLE_PHRASE_JUMP: usize = 3;

#[derive(Clone, Debug)]
pub struct SongContext {
    pub section: SongSection,
    /// Сквозной номер такта от старта сессии (1-based). Не сбрасывается
    /// на границах секций: по нему живут прогрессия, `every_n_measures`
    /// и вторая половина мотива (см. `styles::play_guide`).
    pub measure_index: usize,
    /// Монотонный номер музыкальной фразы (2 такта). Фразы выровнены
    /// по нечетным тактам: `total=1,2 -> 0; 3,4 -> 1; ...`. Внутри фразы
    /// фаза такта = `(measure_index - 1) % 2`.
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
    /// Кто играет в этой секции (маска групп): аранж написан в плане
    /// куска, а не брошен на каждом шаге.
    pub layers: crate::piece::LayerMask,
    /// Хэш сида: ротация лада и модальный вариант в генерации.
    /// Стабилен внутри сида — тональность не прыгает посреди трека.
    pub seed_hash: u64,
    /// Плотность генерации 0.7..=1.3 от сида: разреженные и густые сиды.
    pub density: f32,
    /// Мотив сида: 16 ступеней лада 0..4 — мелодическое ДНК трека.
    /// Длинная тема через 2 такта (окно фразы) узким напевом:
    /// сиды различаются темой, а не случайными прыжками.
    /// Широкие скачки давали «свою композицию» у каждого слоя —
    /// неслышимую кашу вместо гетерофонии.
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
    /// Сырое блуждание тоники (префикс-сумма шагов −1/0/+1 для пар
    /// визитов `0..stage/2`). Потребитель берёт `% длина_лада` сам.
    /// Инкрементально: генерация вечна, пересчёт с нуля каждый шаг —
    /// нет (см. `Arranger::extend_tonic`).
    pub tonic_walk: i64,
}

impl SongContext {
    /// Громкость роли на текущей ступени (1.0 = как написано в ноте).
    pub fn stage_gain(&self, role: InstrumentRole) -> f32 {
        self.stage_gains[role.index()]
    }
}

#[derive(Clone)]
pub struct Arranger {
    /// План куска: конечная форма (30-60 с), а не бесконечное кольцо.
    /// Секции идут по плану; шов в конце — написанный финал.
    plan: crate::piece::PiecePlan,
    /// Позиция в плане: индекс секции тела.
    plan_idx: usize,
    /// План исчерпан: тело и шов отыграны, ждём следующий кусок.
    plan_done: bool,
    /// Текущая секция плана (из плана, а не из кольца скелета).
    section: SongSection,
    /// Написанная энергия секции (None — по общей формуле формы).
    section_energy: Option<f32>,
    /// Кто играет в секции (маска групп) — из плана куска.
    section_layers: crate::piece::LayerMask,
    /// Как стиль склеивает куски (плавный внахлёст или резкий стык).
    transition: crate::piece::Transition,
    /// Номер куска в потоке: куски отличаются друг от друга.
    piece: u64,
    /// Сквозной такт, на котором начался текущий кусок: по нему считаем,
    /// сколько тактов куска уже отыграно.
    piece_start_measure: usize,
    /// Сид куска (сырой): планировщик солит им форму.
    seed: u64,
    /// Темп стиля: от него зависит длина куска в тактах.
    bpm: f32,
    /// Скелет стиля: из него планируются следующие куски.
    score: Score,
    /// План следующего куска: пишется, когда начинается шов текущего.
    next_piece: Option<crate::piece::PiecePlan>,
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
    /// Инкрементальное блуждание тоники: префикс-сумма шагов для пар
    /// визитов `0..tonic_done`. Стадии монотонны, поэтому walk только
    /// extends — каждый шаг генерации стоит O(1), а не O(stage).
    /// Сырая сумма (не по модулю): модуль лада берёт потребитель,
    /// лад может отличаться у стилей.
    tonic_pos: i64,
    tonic_done: usize,
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

/// Один шаг детерминированного блуждания тоники (−1/0/+1 из битов
/// хэша пары визитов). Шаг не больше ступени за пару (музыкально
/// связно), путь непериодичен (период потока 2^64 — скелет
/// не замыкается никогда). Тот же сид — тот же путь.
/// Атом генерации: полный walk складывается инкрементально
/// в [`Arranger::extend_tonic`], тесты складывают вручную.
pub(crate) fn drift_step(seed_hash: u64, pair: usize) -> i64 {
    let h = splitmix64(seed_hash ^ (pair as u64).wrapping_mul(0x9E3779B97F4A7C15) ^ 0xD21F7);
    match h % 3 {
        0 => 0,
        1 => 1,
        _ => -1,
    }
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

/// Громкости секции из таблицы. Стыки текут глиссандо в advance_measure,
// никто не гаснет и не врывается внезапно.
fn stage_gains_for(section: SongSection) -> [f32; 8] {
    STAGE_GAINS[section as usize]
}

impl Arranger {
    pub fn new(seed: u64) -> Self {
        Self::with_score(seed, ENERGETIC_SCORE)
    }

    pub fn with_score(seed: u64, score: Score) -> Self {
        // Форма по умолчанию: кусок из скелета стиля. Темп дефолтный —
        // стиль знает свой и подставляет свой план (см. `with_plan`).
        let plan = crate::piece::plan_piece(
            seed,
            126.0,
            score.cycle,
            0,
            0,
            crate::piece::Transition::Rolling { bars: 2 },
        );
        Self::with_plan(
            seed,
            126.0,
            score,
            plan,
            crate::piece::Transition::Rolling { bars: 2 },
        )
    }

    /// Арранжер с готовым планом куска: форма конечна (30-60 с), шов в
    /// конце — написанный финал. Следующие куски арранжер планирует сам,
    /// когда начинается шов: стык кусков написан заранее.
    pub fn with_plan(
        seed: u64,
        bpm: f32,
        score: Score,
        plan: crate::piece::PiecePlan,
        transition: crate::piece::Transition,
    ) -> Self {
        let mut a = Self::with_plan_inner(seed, bpm, score, plan, transition);
        a.enter_section();
        a
    }

    fn with_plan_inner(
        seed: u64,
        bpm: f32,
        score: Score,
        plan: crate::piece::PiecePlan,
        transition: crate::piece::Transition,
    ) -> Self {
        let seed_hash = splitmix64(seed ^ 0x51ED);
        // Размах темы — почерк сида: половина сидов поёт узко
        // (поступенно, как раньше), половина — широко (скачки до
        // терции). Один напев на всех давал «одинаковую генерацию»;
        // теперь характер темы (напев vs прыжки) различает сиды.
        let wide = splitmix64(seed_hash ^ 0x1DE5) % 2 == 0;
        // Мотив сида: напевная линия 0..4 (узкий напев),
        // а не 16 независимых бросков. Случайное блуждание шагом ≤ 1
        // (широкие сиды — до 2) с аркой (подъем к середине, спад
        // к кадансу): мелодия цельная внутри сида, развивается окном
        // фразы и тоникой, а не пересдается каждый шаг. Генерация —
        // задом наперед от тоники: последний шаг всегда 0 (каданс),
        // предпоследний — сосед (правильный подход, а не прыжок).
        // Живет весь круг неизменной.
        let mut motif = [0u8; 16];
        let mut m = splitmix64(seed_hash ^ 0x9071F);
        motif[15] = 0;
        let mut pos: i8 = 0;
        let mut dir: i8 = 1;
        for i in (0..15).rev() {
            m = splitmix64(m);
            let r = m % 100;
            if r >= 20 {
                // Смена направления — редко (напев, а не пила).
                if r >= 94 {
                    dir = -dir;
                }
                // Арка: к середине тянем вверх, к началу — к середине
                // диапазона (пик где-то в первой половине пути назад).
                if (6..11).contains(&i) && pos < 3 {
                    dir = 1;
                }
                if i < 4 && pos > 2 {
                    dir = -1;
                }
                if i < 4 && pos < 2 {
                    dir = 1;
                }
                // Широкие сиды иногда шагают через ступень (терция).
                // Не влезает — откат на поступенный (всегда валиден).
                let mut next = pos + dir;
                if wide && (m >> 32) % 4 == 0 {
                    let ext = pos + dir * 2;
                    if (0..=4).contains(&ext) {
                        next = ext;
                    }
                }
                if !(0..=4).contains(&next) {
                    dir = -dir;
                    next = pos + dir;
                }
                pos = next.clamp(0, 4);
            }
            motif[i] = pos as u8;
        }
        // Тема B: близкая вариация A — до 3 мутаций (±ступень, у широких
        // сидов до терции). Не «вторая композиция», а развитие той же:
        // припев к куплету, а не другой трек. Позиции — от хэша сида,
        // шаг всегда соседний (кламп вместо модуля: модуль давал
        // прыжок 4→0).
        // Мутация, рвущая линию с соседями, отклоняется (другим
        // направлением или пропуском): B держит размах своей темы.
        // Страж на случай вырождения мутаций в исходную тему.
        let mut motif_b = motif;
        let mut g = splitmix64(seed_hash ^ 0xB17B);
        // Допуск линии: широкий сид терпит скачки до 2, узкий — до 1.
        let line_lim: i8 = if wide { 2 } else { 1 };
        for _ in 0..3 {
            g = splitmix64(g);
            // Позиции 0..14: каданс (шаг 15 = 0) мутации не трогают.
            let pos = (g % 15) as usize;
            g = splitmix64(g);
            let cur = motif_b[pos] as i8;
            let left = if pos > 0 { motif_b[pos - 1] as i8 } else { cur };
            let right = motif_b[pos + 1] as i8;
            let mut done = false;
            // Широкие мутируют терциями (±2 первым кандидатом),
            // узкие — поступенно: B в размахе своей A.
            let mag: i8 = if wide { 2 } else { 1 };
            for d in [
                if g % 2 == 0 { mag } else { -mag },
                if g % 2 == 0 { -1 } else { 1 },
            ] {
                let v = (cur + d).clamp(0, 4);
                if (v - left).abs() <= line_lim && (v - right).abs() <= line_lim {
                    motif_b[pos] = v as u8;
                    done = true;
                    break;
                }
            }
            let _ = done;
        }
        if motif_b == motif {
            motif_b[0] = if motif[0] < 4 { motif[0] + 1 } else { 3 };
        }
        let first_section = plan
            .body
            .first()
            .map(|s| s.section)
            .unwrap_or(SongSection::MainMotif);
        Self {
            plan,
            plan_idx: 0,
            plan_done: false,
            section: first_section,
            section_energy: None,
            section_layers: crate::piece::layers_for(SongSection::MainMotif),
            transition,
            piece: 0,
            piece_start_measure: 0,
            seed,
            bpm,
            score,
            next_piece: None,
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
            // Старт в полную: первый такт — уже тема, без разогрева.
            prev_gains: stage_gains_for(first_section),
            tonic_pos: 0,
            tonic_done: 0,
        }
    }

    /// Секция плана по индексу: тело, потом шов (финал куска).
    fn plan_section(&self, idx: usize) -> Option<&crate::piece::SectionPlan> {
        if idx < self.plan.body.len() {
            self.plan.body.get(idx)
        } else if idx == self.plan.body.len() {
            Some(&self.plan.joint)
        } else {
            None
        }
    }

    /// План исчерпан: кусок отыгран, ждём следующий (его подставляет
    /// композитор). Пока не подменён — держим последнюю секцию, музыка
    /// не встает.
    pub fn plan_exhausted(&self) -> bool {
        self.plan_done
    }

    /// Сколько тактов текущего куска уже отыграно: по этому кусок
    /// перепланируется при живой смене сида/стиля (смена вступает с
    /// следующего такта, отыгранное не трогаем).
    pub fn piece_bar(&self) -> usize {
        self.total_measures.saturating_sub(self.piece_start_measure)
    }

    /// Номер текущего куска: по нему перепланируется остаток (живая
    /// смена сида/стиля не должна ломать форму куска).
    pub fn piece(&self) -> u64 {
        self.piece
    }

    /// Тень перехода: копия арранжера, у которой впереди — план
    /// следующего куска. Копия, а не новый арранжер: состояние (счетчик
    /// визитов, тоника, гармония) должно совпадать с основным, иначе
    /// тень спела бы другой кусок в другой тональности.
    pub fn shadow_of(&self, plan: crate::piece::PiecePlan) -> Self {
        let mut s = self.clone();
        s.plan = plan;
        s.plan_idx = 0;
        s.plan_done = false;
        s.next_piece = None;
        s.section_measure_count = 0;
        // Круг куска у тени уже следующий — как у основного после
        // приема плана.
        s.cycle = self.cycle + 1;
        s.enter_section();
        // Первый такт нового куска тень поет сразу: счетчик секции
        // начинается с единицы, а общий счетчик тактов сессии не
        // сдвигается — тень и основной идут по одним часам.
        s.section_measure_count = 1;
        s
    }

    /// Идёт ли шов (финал куска): в эти такты следующий кусок входит
    /// голосом поверх грува.
    pub fn in_joint(&self) -> bool {
        self.plan_idx >= self.plan.body.len()
    }

    /// План следующего куска (пишется, когда начинается шов): по нему
    /// композитор строит «тень» — голос следующего куска.
    pub fn next_plan(&self) -> Option<&crate::piece::PiecePlan> {
        self.next_piece.as_ref()
    }

    /// Насколько шов прожит (0..1): громкость тени нарастает к его концу.
    pub fn joint_progress(&self) -> f32 {
        if self.section_len == 0 {
            return 0.0;
        }
        (self.section_measure_count as f32 / self.section_len as f32).clamp(0.0, 1.0)
    }

    /// Прогресс шва по шагам (0..1): голос тени входит не с такта, а
    /// плавно, шаг за шагом — вход слышен как событие, а не как удар.
    pub fn joint_step_progress(&self, steps_per_measure: usize, step_in_measure: usize) -> f32 {
        if self.section_len == 0 {
            return 0.0;
        }
        let steps = self
            .section_measure_count
            .saturating_sub(1)
            .saturating_mul(steps_per_measure)
            .saturating_add(step_in_measure) as f32;
        let total = (self.section_len * steps_per_measure).max(1) as f32;
        (steps / total).clamp(0.0, 1.0)
    }

    /// Подменить план куска: перепланированный остаток текущего (живая
    /// смена сида/стиля вступает с следующего такта).
    pub fn set_plan(&mut self, plan: crate::piece::PiecePlan) {
        self.plan = plan;
        self.plan_idx = 0;
        self.plan_done = false;
        self.next_piece = None;
        self.piece_start_measure = self.total_measures;
        self.section_measure_count = 0;
        self.enter_section();
    }

    /// Следующий кусок потока: форма пишется сама, когда начинается шов
    /// текущего. Стык написан заранее — следующий кусок известен, пока
    /// играет финал текущего.
    fn plan_next_piece(&mut self) {
        let index = self.piece + 1;
        self.next_piece = Some(crate::piece::plan_piece(
            self.seed,
            self.bpm,
            self.score.cycle,
            index,
            0,
            self.transition,
        ));
    }

    /// Вход в секцию: длина и монотонная соль визита.
    fn enter_section(&mut self) {
        // Шов (финал куска) — момент, когда пишется следующий кусок:
        // стык написан заранее, а не сшит на бегу.
        if self.plan_idx == self.plan.body.len() && self.next_piece.is_none() {
            self.plan_next_piece();
        }
        let planned = self.plan_section(self.plan_idx).map(|sp| {
            (
                sp.section,
                sp.energy,
                sp.layers,
                if sp.vary {
                    varied_len(sp.section, sp.bars, self.seed_hash, self.stages_done)
                } else {
                    sp.bars
                },
            )
        });
        match planned {
            Some((section, energy, layers, len)) => {
                self.section = section;
                self.section_energy = energy;
                self.section_layers = layers;
                self.section_len = len;
            }
            None => {
                // План исчерпан: держим текущую секцию (её уже отыграли),
                // пока не подменят план.
                self.plan_done = true;
                self.section_len = self.section_len.max(1);
            }
        }
        self.stages_done += 1;
        self.section_measure_count = 0;
    }

    fn section(&self) -> SongSection {
        self.section
    }

    /// Дотянуть walk тоники до пары визитов `pair` (включительно —
    /// точнее, сложить шаги `tonic_done..pair`). Пары растут монотонно
    /// вместе со сценами, поэтому каждый шаг считается один раз
    /// за всю бесконечную сессию: O(1) на такт вместо O(stage).
    fn extend_tonic(&mut self, pair: usize) -> i64 {
        while self.tonic_done < pair {
            self.tonic_pos += drift_step(self.seed_hash, self.tonic_done);
            self.tonic_done += 1;
        }
        self.tonic_pos
    }

    pub fn advance_measure(&mut self) -> SongContext {
        self.section_measure_count += 1;
        self.total_measures += 1;

        if self.section_measure_count > self.section_len {
            // Секция кончилась. Шов отыгран — кусок исчерпан: берём
            // следующий кусок (его план написан, когда начался шов).
            if self.plan_idx + 1 > self.plan.body.len() {
                if let Some(next) = self.next_piece.take() {
                    let overlap = self.transition.overlap_bars();
                    self.plan = next;
                    self.piece += 1;
                    self.cycle += 1;
                    self.plan_idx = 0;
                    // Налёт перехода: первые такты нового куска уже
                    // спел голос тени (он входил в шве). Продолжаем с
                    // того же места, длина секции остаётся полной — иначе
                    // энергия прыгнула бы на стыке.
                    self.piece_start_measure = self.total_measures.saturating_sub(overlap);
                    // enter_section сбрасывает счетчик — текущий такт первый.
                    self.enter_section();
                    self.section_measure_count = overlap + 1;
                    self.section_len = self.section_len.max(overlap + 1);
                } else {
                    // Плана нет (форма не должна встать): держим секцию.
                    self.plan_done = true;
                    self.section_len = self.section_len.max(1);
                    self.section_measure_count = 1;
                }
            } else {
                self.plan_idx += 1;
                self.enter_section();
                // enter_section сбросил счетчик — текущий такт первый.
                self.section_measure_count = 1;
            }
        }

        self.build_context()
    }

    /// Контекст текущего такта (без продвижения счетчиков). Отдельно от
    /// `advance_measure`, чтобы тень перехода читала тот же такт, что и
    /// основной арранжер, не сдвигая счетчик сессии.
    pub fn build_context(&mut self) -> SongContext {
        let section = self.section();
        let is_last = self.section_measure_count == self.section_len;
        let progress = self.section_measure_count as f32 / self.section_len as f32;
        // Фразы — по 2 такта, границы на нечетных тактах (1,3,5,...):
        // секции активных стилей имеют четную длину и стартуют с нечетного
        // такта, поэтому фраза никогда не рвется на стыке секций.
        let phrase_index = (self.total_measures - 1) / PHRASE_LENGTH_MEASURES
            + self.phrase_offset
            + (self.cycle as usize).wrapping_mul(CYCLE_PHRASE_JUMP);
        // Глиссандо громкостей: полпути к таблице за такт. Стыки секций
        // текут, а не щелкают — никто не гаснет и не врывается внезапно.
        // Первый такт уже в цели (prev_gains инициализированы таблицей).
        let target = stage_gains_for(section);
        let mut gains = self.prev_gains;
        for (g, t) in gains.iter_mut().zip(target) {
            *g += (t - *g) * 0.5;
        }
        self.prev_gains = gains;

        // Тоника стоит парами визитов (см. extend_tonic): инкрементально,
        // один раз за сессию, а не пересчётом с нуля на каждый шаг.
        let stage = self.stages_done as usize;
        let tonic_walk = self.extend_tonic(stage / 2);

        SongContext {
            section,
            // Сквозной такт сессии: гармония, `every_n_measures` и вторая
            // половина мотива живут по нему, а не по локальному счетчику.
            measure_index: self.total_measures,
            phrase_index,
            energy: energy_for(
                section,
                progress,
                self.cycle,
                self.seed_hash,
                self.stages_done,
                self.section_energy,
            ),
            progress,
            is_last_measure: is_last,
            stage,
            cycle: self.cycle,
            stage_gains: gains,
            layers: self.section_layers,
            seed_hash: self.seed_hash,
            density: self.density,
            motif: self.motif,
            motif_b: self.motif_b,
            // Тема секции: кульминации (Drop/PostChorus) поют B,
            // остальное — A. Энергия и тема усиливают друг друга:
            // настоящий припев, а не та же мелодия погромче.
            use_alt_motif: matches!(section, SongSection::Drop | SongSection::PostChorus),
            prog: self.prog,
            tonic_walk,
        }
    }
}

/// Открытость фильтра формы 0..1: движок домножает на нее срез верхов.
/// Тема держит середину, билд свистит вверх весь разгон и раскрывается
/// на дропе, передышка прикрывается обратно. Чистая функция.
pub fn section_openness(section: SongSection, progress: f32) -> f32 {
    let p = progress.clamp(0.0, 1.0);
    match section {
        SongSection::MainMotif => 0.4,
        SongSection::BuildUp => 0.4 + 0.6 * p,
        SongSection::Drop => 1.0,
        SongSection::PostChorus => 0.3,
    }
}

fn energy_for(
    section: SongSection,
    progress: f32,
    cycle: u64,
    seed_hash: u64,
    salt: u64,
    written: Option<f32>,
) -> f32 {
    // Написанная энергия секции (из плана куска) — если есть, берём её
    // за основу; иначе общая формула формы.
    let base = match written {
        Some(b) => b,
        None => match section {
            SongSection::MainMotif => 0.55,
            SongSection::BuildUp => 0.6,
            SongSection::Drop => return 1.0,
            SongSection::PostChorus => 0.45,
        },
    };
    // Дроп — якорь, всегда ровно 1.0.
    if section == SongSection::Drop {
        return 1.0;
    }
    // Разгон идет вверх, передышка оседает: энергия — ход секции.
    let ramp = match section {
        SongSection::BuildUp => progress * 0.4,
        SongSection::PostChorus => -progress * 0.1,
        _ => 0.0,
    };
    // Легкое дыхание энергии по кругам: форма та же, накал чуть разный.
    let wobble = (hash01(seed_hash, cycle, salt) - 0.5) * 0.15;
    (base + ramp + wobble).clamp(0.05, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shadow_continues_the_next_piece_without_a_seam() {
        // Переход внахлёст: тень поет первые такты следующего куска,
        // а основной арранжер продолжает его же с того же места.
        // Секция, прогресс и тональность обязаны совпасть — иначе
        // стык слышно как смена тона.
        use crate::piece::{plan_piece, Transition};
        let transition = Transition::Rolling { bars: 2 };
        let plan = plan_piece(424242, 126.0, ENERGETIC_SCORE.cycle, 0, 0, transition);
        let mut a = Arranger::with_plan(424242, 126.0, ENERGETIC_SCORE, plan, transition);
        let mut shadow: Option<Arranger> = None;
        let mut shadow_ctx: Option<SongContext> = None;
        let mut was_joint = false;
        let mut checked = false;
        for _ in 0..96 {
            let ctx = a.advance_measure();
            // Тень идет в ногу с основным: те же такты.
            if let Some(s) = shadow.as_mut() {
                shadow_ctx = Some(s.advance_measure());
            }
            if a.in_joint() {
                if shadow.is_none() {
                    let next = a.next_plan().expect("plan of the next piece").clone();
                    let mut s = a.shadow_of(next);
                    let song = s.build_context();
                    shadow = Some(s);
                    shadow_ctx = Some(song);
                }
            } else {
                if was_joint {
                    // Основной арранжер взял следующий кусок: тень должна
                    // стоять ровно на том же такте.
                    let sc = shadow_ctx.expect("shadow context");
                    assert_eq!(ctx.section, sc.section, "секция разошлась");
                    assert!(
                        (ctx.progress - sc.progress).abs() < 0.01,
                        "прогресс разошелся: {} против {}",
                        ctx.progress,
                        sc.progress
                    );
                    assert_eq!(ctx.tonic_walk, sc.tonic_walk, "тоника разошлась");
                    assert_eq!(ctx.motif, sc.motif, "мотив разошелся");
                    checked = true;
                }
                shadow = None;
                shadow_ctx = None;
            }
            was_joint = a.in_joint();
        }
        assert!(checked, "стык не проверен: шов не наступил");
    }

    #[test]
    fn filter_opens_toward_drop() {
        // Свип формы: билд доходит до 1.0, дроп открыт,
        // передышка ниже темы.
        assert_eq!(section_openness(SongSection::BuildUp, 1.0), 1.0);
        assert_eq!(section_openness(SongSection::Drop, 0.5), 1.0);
        assert!(
            section_openness(SongSection::PostChorus, 0.0)
                < section_openness(SongSection::MainMotif, 0.0)
        );
    }

    #[test]
    fn full_cycle_visits_every_section_no_outro() {
        // Круг активных: тема, разгон, дроп, передышка — и никаких
        // финалов и разогревов: генерация бесконечная и сразу в полную.
        let mut a = Arranger::new(1);
        let mut seen = std::collections::HashSet::new();
        for _ in 0..96 {
            seen.insert(a.advance_measure().section);
        }
        for s in [
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
        // Разогрева нет: первый визит — сразу тема.
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
            vec![MainMotif, BuildUp, Drop, PostChorus],
            "{order:?}"
        );
    }

    #[test]
    fn calm_score_breathes_main_with_rare_variety() {
        // Спокойный кусок дышит темой: тема занимает большую часть,
        // подъем — редкий гость, а конец куска — передышка (шов-финал).
        // Разогрева нет.
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
        // Кусок: тема, подъем, передышка на конце — и снова кусок.
        assert_eq!(&order[..3], &[MainMotif, BuildUp, PostChorus], "{order:?}");
        let mains = order.iter().filter(|s| **s == MainMotif).count();
        let builds = order.iter().filter(|s| **s == BuildUp).count();
        assert!(mains >= builds, "calm must breathe main: {order:?}");
    }

    #[test]
    fn pieces_run_one_after_another() {
        // Кусок конечен: тело, шов-финал, потом следующий кусок.
        // Поток бесконечный, но форма — не кольцо, а цепочка кусков.
        let mut a = Arranger::new(3);
        let mut cycles = std::collections::HashSet::new();
        let mut last = None;
        for _ in 0..200 {
            let ctx = a.advance_measure();
            if last != Some(ctx.section) {
                cycles.insert(ctx.cycle);
                last = Some(ctx.section);
            }
        }
        assert!(
            cycles.len() >= 3,
            "куски не сменяют друг друга: {:?}",
            cycles
        );
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
    fn phrases_span_two_measures_from_the_first_one() {
        // Фраза — ровно 2 такта, первая фраза начинается с первого такта.
        // Такт внутри фразы = (measure_index - 1) % 2.
        let mut a = Arranger::new(1);
        let first = a.advance_measure();
        let second = a.advance_measure();
        let third = a.advance_measure();
        let fourth = a.advance_measure();
        assert_eq!(first.measure_index, 1);
        assert_eq!(second.measure_index, 2);
        assert_eq!(third.measure_index, 3);
        assert_eq!(fourth.measure_index, 4);
        assert_eq!(first.phrase_index, second.phrase_index);
        assert_eq!(third.phrase_index, fourth.phrase_index);
        assert_ne!(first.phrase_index, third.phrase_index);
    }

    #[test]
    fn measure_index_is_global_across_sections() {
        // section_measure_count сбрасывается на стыках, а сквозной такт — нет:
        // по нему движутся прогрессия и every_n_measures.
        let mut a = Arranger::new(1);
        let mut measures = 0usize;
        let mut last_section = None;
        for _ in 0..300 {
            let ctx = a.advance_measure();
            measures += 1;
            assert_eq!(
                ctx.measure_index, measures,
                "measure index must be global and monotonic"
            );
            if let Some(prev) = last_section {
                if prev != ctx.section && ctx.measure_index == 1 {
                    panic!("measure index reset on section boundary");
                }
            }
            last_section = Some(ctx.section);
        }
    }

    #[test]
    fn phrases_never_straddle_section_boundaries() {
        // Секции четной длины и стартуют с нечетного такта: фраза
        // (2 такта) целиком лежит внутри секции, а не рвется на стыке.
        let mut a = Arranger::new(9);
        let mut last_section = None;
        let mut last_phrase = usize::MAX;
        for _ in 0..400 {
            let ctx = a.advance_measure();
            if let Some(prev) = last_section {
                if prev != ctx.section {
                    let bar = (ctx.measure_index - 1) % 2;
                    assert_eq!(bar, 0, "section starts mid-phrase: {ctx:?}");
                    assert_ne!(
                        ctx.phrase_index, last_phrase,
                        "new section must start a new phrase"
                    );
                }
            }
            last_section = Some(ctx.section);
            last_phrase = ctx.phrase_index;
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
            assert!(!first.use_alt_motif, "starts with A");
            // Размах темы — почерк сида: узкие поступенные (≤1),
            // широкие терцовые (≤2). Каданс всегда на тонике.
            let seed_hash = splitmix64(seed ^ 0x51ED);
            let lim: i8 = if splitmix64(seed_hash ^ 0x1DE5) % 2 == 0 {
                2
            } else {
                1
            };
            for (name, m) in [("motif", first.motif), ("motif_b", first.motif_b)] {
                for v in m {
                    assert!(v <= 4, "{name} out of range: {v}");
                }
                for w in m.windows(2) {
                    assert!(
                        (w[0] as i8 - w[1] as i8).abs() <= lim,
                        "{name} leaps: {m:?}"
                    );
                }
                assert_eq!(m[15], 0, "{name} without cadence: {m:?}");
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
        assert!(
            main_a && drop_b && post_b,
            "sections never complete a cycle"
        );
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
    fn starts_at_full_strength() {
        // Разогрева нет: первый такт — уже тема в полную силу,
        // все роли слышны (гейны > 0.3), heartbeat бочки стучит.
        let mut a = Arranger::new(3);
        let ctx = a.advance_measure();
        assert_eq!(ctx.section, SongSection::MainMotif);
        assert!(!ctx.use_alt_motif, "starts with A");
        for (i, g) in ctx.stage_gains[..7].iter().enumerate() {
            assert!(*g > 0.3, "role {i} muted at start: {g}");
        }
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
        let progs: std::collections::HashSet<_> = (0..16u64)
            .map(|s| Arranger::new(s).advance_measure().prog)
            .collect();
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
