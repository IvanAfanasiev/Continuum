//! Стили как данные-направления, а не партитуры.
//!
//! Каждый стиль объявляет: лад, темп, свинг, дефолтный микс по слотам
//! и по одной направляющей [`RoleGuide`] на слот. Как именно из весов
//! родятся ноты, решает общий движок [`render_style`]: вес 1.0 —
//! структурный якорь жанра (всегда играет), веса между — вероятности,
//! масштабируемые энергией секции. Никаких номеров шагов в коде стилей.

pub mod club;
pub mod lofi;
pub mod phonk;
pub mod rock;

use rand::rngs::StdRng;
use rand::Rng;

use crate::arranger::SongContext;
use crate::core::{base_step_secs, swing_step_secs};
use crate::instruments::{InstrumentRole, NoteEvent, Slot};

/// Полка каталога стилей для сортировки фронта.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StyleGroup {
    Club,
    Rock,
    Chill,
}

impl StyleGroup {
    /// Каноническое имя для фронта и сортировки.
    pub fn name(self) -> &'static str {
        match self {
            StyleGroup::Club => "club",
            StyleGroup::Rock => "rock",
            StyleGroup::Chill => "chill",
        }
    }

    /// Порядок полок в групповой сортировке.
    pub fn order(self) -> usize {
        match self {
            StyleGroup::Club => 0,
            StyleGroup::Rock => 1,
            StyleGroup::Chill => 2,
        }
    }
}

/// Все, что нужно движку для генерации одного шага.
pub struct StepContext<'a> {
    pub step: usize,
    pub step_in_measure: usize,
    pub base_step_secs: f32,
    pub active_tracks: &'a [usize],
    pub song: &'a SongContext,
    pub rng: &'a mut StdRng,
    /// Ручка intensity фронта (дефолт 1.0): множитель энергии.
    /// Выше — плотнее и ярче, ниже — разреженнее и тише.
    pub intensity: f32,
}

/// Как озвучить срабатывание направляющей.
#[derive(Clone, Copy)]
pub enum Voicing {
    /// Ударный инструмент: фиксированный питч набора (кик 36, снейр 38...).
    Kit(u8),
    /// Питчевый инструмент: ступень лада от текущей фразы + октава.
    /// `jitter` — блуждание по соседним ступеням (мелодическая жизнь),
    /// 0 = строго держим ступень.
    Scale {
        degree: usize,
        octave: i8,
        jitter: usize,
    },
}

/// Направляющая одного слота: тенденции, а не инструкции.
///
/// Архитектура «план роли → исполнители» (см. vision):
/// `weights` — это ПЛАН РОЛИ, общий для всех ее исполнителей
/// (один и тот же `&'static` срез на несколько слотов). Слоты одной роли
/// играют один ритмический скелет, но каждый — своим голосом, своей
/// октавой и своим броском гейта (слот входит в хэш): гетерофония,
/// а не робот-унисон. Громкости взвешивают исполнителей.
/// `alt`/`poles` — индивидуальные краски поверх плана (пока).
///
/// `weights` — веса шагов такта (длина = steps_per_measure стиля):
/// 1.0 — якорь (почти всегда играет: приоритет 0.95, даунбит дышит),
/// 0.0 — никогда, между — порог фиксированного гейта сида.
/// Итоговая вероятность: `w * energy² * energy_response`.
/// Гейт НЕ кубик: шаг звучит, если хэш(сид, слот, шаг, фраза) ниже порога.
/// Тот же сид — тот же поток фраз; другой сид — другой.
/// Квадрат энергии держит куплеты разреженными, а дроп — плотным.
/// `poles` — набор из 2–4 готовых грувов (бит!): фраза выбирает один
/// полюс детерминированно от (сид, слот, фраза). Разные фразы — разный
/// бит, тот же сид — тот же бесконечный поток грувов.
/// Если полюса заданы, `weights` игнорируется.
#[derive(Clone, Copy)]
pub struct RoleGuide {
    pub weights: &'static [f32],
    pub poles: Option<&'static [&'static [f32]]>,
    pub energy_response: f32,
    /// Молчать во второй половине последнего такта (брейк-пауза).
    pub drop_before_climax: bool,
    /// Играть каждый N-й такт (пэды через такт и т.п.). 1 = всегда.
    pub every_n_measures: usize,
    /// Оффбитные 8-е звучат длиннее (открытые хэты сами собой).
    pub long_offbeats: bool,
    /// Во сколько раз длиннее (от duration_mult).
    pub long_mult: f32,
    pub duration_mult: f32,
    pub vel_base: f32,
    pub vel_energy: f32,
    pub vel_jitter: f32,
    pub voice: Voicing,
    /// Вероятность скачка на октаву вверх (басовые переливы).
    pub octave_up: f32,
    /// Потолок регистра: скачок вверх не применяется, если нота
    /// превысит его. Держит бас внизу без жестких указаний.
    pub top_pitch: u8,
    /// Приглушение высоких нот: множитель velocity на октаву выше MIDI 40.
    /// 1.0 = выключено. Бас на 0.6: неудачно высокие ноты звучат тише,
    /// низкие — в полную силу (классика с ударными).
    pub vel_high_cut: f32,
    /// Следовать мотиву сида: ступень берется из `song.motif`
    /// (мелодическое ДНК трека), а не из дрейфа по ладу.
    /// Так лид играет узнаваемую тему сида — «ремикс на известную песню».
    pub follow_motif: bool,
    /// Только последний такт секции (крэши, влеты в дроп).
    pub climax_only: bool,
    /// Интервалы аккорда в ступенях лада (пэды/клавиши берут трезвучие).
    /// None + роль Chords + не мотив = дефолтное трезвучие [0, 2, 4].
    /// Мелодия и ударные всегда одиночные.
    pub chord: Option<&'static [i8]>,
    /// Квинтовый бас: оффбитные восьмые ходят на квинту — баслайн
    /// движется, а не топчется. Только роль Bass, только по флагу.
    pub bass_fifth: bool,
    /// Арпеджио: интервалы аккорда играются по одному вкруговую
    /// (фингерпикинг), а не стеком. Источник интервалов — `chord`.
    /// Одна струна за раз вместо щелкающей каши аккордов.
    pub arp: bool,
    /// Саб-перекос 0..1: гасит струну/щелчок/презенс в пользу чистого
    /// синуса. Club-бас глубокий и плавный, рок — струнный (0.0).
    /// Читает только бас-голос.
    pub sub_bias: f32,
    /// Потолок баса: уползшее выше складывается октавой вниз.
    /// Дефолт 45 (A2, саб). Рок-бас риффует с гитарой выше (52).
    pub bass_top: u8,
    /// Emergent-пульс в долях (0.0 = выкл, сеточный путь).
    /// Слот бьет по собственной решетке с периодом из сида, а не по весам:
    /// несоизмеримость с тактом дает вечно дрейфующий грув без повторов
    /// (фэйзинг вместо паттерна). Период дышит энергией секции
    /// (в дропе чаще) и варьируется сидом ±25%. Приоритет над
    /// weights/poles/gates; структура (every_n, climax, акценты) и баланс
    /// партий действуют как обычно. Чистая функция глобального шага —
    /// состояния нет, тот же сид дает тот же поток.
    pub emergent_beats: f32,
}

impl RoleGuide {
    /// Пустая направляющая: слот молчит и не трогает RNG.
    pub const SILENT: Self = Self {
        weights: &[],
        poles: None,
        energy_response: 0.0,
        drop_before_climax: false,
        every_n_measures: 1,
        long_offbeats: false,
        long_mult: 1.0,
        duration_mult: 1.0,
        vel_base: 0.0,
        vel_energy: 0.0,
        vel_jitter: 0.0,
        voice: Voicing::Kit(0),
        octave_up: 0.0,
        top_pitch: 127,
        vel_high_cut: 1.0,
        follow_motif: false,
        climax_only: false,
        chord: None,
        bass_fifth: false,
        sub_bias: 0.0,
        bass_top: 45,
        arp: false,
        emergent_beats: 0.0,
    };

    pub fn is_silent(&self) -> bool {
        self.weights.iter().all(|&w| w <= 0.0)
    }

    /// Заготовка ударного: фиксированный питч, остальное по дефолту.
    /// Вызывается с перекрытиями (`..RoleGuide::kit(...)`).
    pub const fn kit(weights: &'static [f32], pitch: u8, vel: f32) -> Self {
        Self {
            weights,
            poles: None,
            energy_response: 1.0,
            drop_before_climax: false,
            every_n_measures: 1,
            long_offbeats: false,
            long_mult: 1.0,
            duration_mult: 1.0,
            vel_base: vel,
            vel_energy: 0.0,
            vel_jitter: 0.05,
            voice: Voicing::Kit(pitch),
            octave_up: 0.0,
            top_pitch: 127,
            vel_high_cut: 1.0,
            follow_motif: false,
            climax_only: false,
            chord: None,
            bass_fifth: false,
            sub_bias: 0.0,
            bass_top: 45,
            arp: false,
            emergent_beats: 0.0,
        }
    }

    /// Заготовка питчевого: ступень лада + октава + джиттер.
    pub const fn pitched(
        weights: &'static [f32],
        degree: usize,
        octave: i8,
        jitter: usize,
        duration_mult: f32,
        vel: f32,
    ) -> Self {
        Self {
            weights,
            poles: None,
            energy_response: 1.0,
            drop_before_climax: false,
            every_n_measures: 1,
            long_offbeats: false,
            long_mult: 1.0,
            duration_mult,
            vel_base: vel,
            vel_energy: 0.0,
            vel_jitter: 0.05,
            voice: Voicing::Scale {
                degree,
                octave,
                jitter,
            },
            octave_up: 0.0,
            top_pitch: 127,
            vel_high_cut: 1.0,
            follow_motif: false,
            climax_only: false,
            chord: None,
            bass_fifth: false,
            sub_bias: 0.0,
            bass_top: 45,
            arp: false,
            emergent_beats: 0.0,
        }
    }
}

pub trait Style: Send + Sync {
    fn name(&self) -> &str;
    /// Полка каталога для сортировки фронта: Club (хаус, техно, транс,
    /// дабстеп, драм-н-бейс, электро, рейв), Rock (рок, хард, метал),
    /// Chill (спокойные вроде lofi). Дефолт — Club.
    fn group(&self) -> StyleGroup {
        StyleGroup::Club
    }
    /// Дефолтный темп. Фронт может перекрыть его живой ручкой темпа.
    fn bpm(&self) -> f32;
    fn steps_per_measure(&self) -> usize;
    fn swing_ratio(&self) -> f32;
    fn scale(&self) -> &'static [u8];
    /// Партитура круга: активным — бесконечный разгон-кульминация,
    /// спокойным — дыхание темы с редкими подъемами. Дефолт — активная.
    fn arrangement(&self) -> crate::arranger::Score {
        crate::arranger::ENERGETIC_SCORE
    }
    /// Вайтлист инструментов стиля: кто включен при выборе стиля,
    /// остальные глушатся в ноль. Громкости — канонические, у слотов
    /// ([`Slot::base_volume`]); пресет не хардкодит цифры.
    fn ensemble(&self) -> &'static [Slot];
    /// Флекс-пул: кандидаты на гостевые места в миксе, выбор — сидом.
    /// Ядро жанра (вайтлист) стабильно, специи меняются: каждый сид —
    /// свой состав. Пусто = весь состав фиксирован.
    fn flex_pool(&self) -> &'static [Slot] {
        &[]
    }
    /// Веса предпочтений пула (параллельно flex_pool): частые гости
    /// и редкие специи. Розыгрыш взвешенный (см. pick_flex_slots):
    /// тенденции вместо равномерной ротации. Дефолт — равные.
    /// Лишние хвосты игнорируются, недостающие считаются 1.0.
    fn flex_weights(&self) -> &'static [f32] {
        &[]
    }
    /// Бюджет гостей (мин..=макс): одни сиды аскетичны, другие богаче.
    /// Число разыгрывается сидом внутри бюджета. Дефолт — ровно
    /// flex_count (старое поведение).
    fn flex_range(&self) -> (usize, usize) {
        let n = self.flex_count();
        (n, n)
    }
    /// Сколько гостевых мест разыгрывается (0 = выключено).
    fn flex_count(&self) -> usize {
        0
    }
    /// Направляющие по слотам в порядке [`Slot::ALL`].
    fn slot_guides(&self) -> [RoleGuide; Slot::COUNT];
    /// Комната реверба: доля мокрого (0 = сухо, как хит клуба на массиве).
    fn reverb_wet(&self) -> f32 {
        0.16
    }
    /// Размер комнаты 0..1 (комната → собор).
    fn reverb_size(&self) -> f32 {
        0.5
    }
    /// Срез верхов мастера, Гц. Лоуфай мутный, клуб хирургический.
    fn treble_cutoff(&self) -> f32 {
        19000.0
    }
    /// Форма энергии: отображение энергии секции в рабочую плотность.
    /// Плоская кривая (lofi) убирает взрывные билдапы/дропы жанрово,
    /// а не запретами в коде.
    fn shape_energy(&self, energy: f32) -> f32 {
        energy
    }
    /// Глубина сайдчейна 0..2 (множитель пампа): рок давит жестко,
    /// lofi едва дышит. Дефолт 1.0 — характер движка.
    fn sidechain_depth(&self) -> f32 {
        1.0
    }

    fn base_step_secs(&self) -> f32 {
        base_step_secs(self.bpm(), self.steps_per_measure())
    }

    fn swing_duration(&self, step_in_measure: usize) -> f32 {
        swing_step_secs(self.base_step_secs(), self.swing_ratio(), step_in_measure)
    }
}

/// Общий движок генерации: один цикл по слотам для всех стилей.
/// Слот звучит, только если его id в активных треках (громкость > 0) —
/// выключенные инструменты не грузят ни генерацию, ни DSP.
/// Состав влияет на генерацию: слоты одной роли делят плотность
/// (`1/sqrt(n)` на не-якорные вероятности) — выключил второй лид,
/// оставшийся играет гуще и подхватывает пространство. Якоря жанра
/// (вес 1.0: бит, фундамент) дележу не подлежат — основа не прерывается.
/// Мотив не пропадает от мьюта: если все носители темы выключены, ее
/// подхватывает первый мелодический питчевый слот (Lead→Vocal→Chords→Bass).
/// Гармония не пропадает от мьюта: если выключены все аккордовые
/// (пэды/клавиши), лиды и чопы сгущаются в диады (корень+терция) —
/// гармонизованная мелодия вместо пустоты. Бас и кик не покрываем:
/// выключенный низ — воля пользователя, а не дыра (и чужой артикуляцией
/// его не заменить — выйдет грязь, а не фундамент).
/// Детерминировано составом: тот же сид + тот же набор = та же музыка.
/// Порядок рендера: сначала кики (фундамент объявляет заливки),
/// затем остальные — бас уже знает, даунбит это или заливка.
pub fn render_style(style: &dyn Style, sctx: &mut StepContext) -> Vec<NoteEvent> {
    let guides = style.slot_guides();
    let scale = style.scale();
    let energy = (style.shape_energy(sctx.song.energy) * sctx.intensity).clamp(0.0, 1.5);
    // Сколько активных носителей у каждой роли — карта дележа плотности.
    let mut role_n = [0usize; 8];
    for id in sctx.active_tracks {
        if let Some(s) = Slot::from_id(*id) {
            role_n[s.role().index()] += 1;
        }
    }
    let is_active = |s: Slot| sctx.active_tracks.contains(&s.id());
    // Тема жива, пока жив хоть один ее носитель.
    let has_carrier = Slot::ALL
        .iter()
        .enumerate()
        .any(|(i, s)| is_active(*s) && guides[i].follow_motif && !guides[i].is_silent());
    // Гармония жива, пока жив хоть один аккордовый слот.
    let has_chords = Slot::ALL.iter().enumerate().any(|(i, s)| {
        is_active(*s) && Slot::ALL[i].role() == InstrumentRole::Chords && !guides[i].is_silent()
    });
    // Резервный носитель: первый питчевый мелодический по рангу роли.
    // Ударные тему петь не могут — только Scale-голоса.
    let fallback: Option<usize> = if has_carrier {
        None
    } else {
        Slot::ALL
            .iter()
            .enumerate()
            .filter(|(i, s)| {
                is_active(**s)
                    && !guides[*i].is_silent()
                    && matches!(guides[*i].voice, Voicing::Scale { .. })
                    && carrier_rank(s.role()).is_some()
            })
            .min_by_key(|(_, s)| (carrier_rank(s.role()).unwrap_or(99), s.id()))
            .map(|(_, s)| s.id())
    };
    let mut notes = Vec::new();
    // Фаза 1: кики. Заливка (сработал не якорь) запоминается —
    // бас во второй фазе под нее пригнется.
    let mut kick_fill = false;
    for (i, slot) in Slot::ALL.iter().enumerate() {
        if slot.role() != InstrumentRole::Kick {
            continue;
        }
        if !sctx.active_tracks.contains(&slot.id()) {
            continue;
        }
        let guide = &guides[i];
        if guide.is_silent() {
            continue;
        }
        let n = role_n[slot.role().index()].max(1) as f32;
        let slot_notes = play_guide(
            guide,
            scale,
            sctx.song,
            energy,
            sctx.step_in_measure,
            style.steps_per_measure(),
            sctx.base_step_secs,
            slot.id(),
            i,
            1.0 / n.sqrt(),
            fallback == Some(slot.id()),
            false,
            sctx.step,
            false,
            sctx.rng,
        );
        if !slot_notes.is_empty()
            && !is_anchor_step(
                guide,
                sctx.song,
                i,
                sctx.step_in_measure,
            )
        {
            kick_fill = true;
        }
        notes.extend(slot_notes);
    }
    // Фаза 2: остальные. Порядок внутри стабилен (Slot::ALL).
    for (i, slot) in Slot::ALL.iter().enumerate() {
        if slot.role() == InstrumentRole::Kick {
            continue;
        }
        if !sctx.active_tracks.contains(&slot.id()) {
            continue;
        }
        let guide = &guides[i];
        if guide.is_silent() {
            continue;
        }
        // Морфинга нет: полюс выбирает фраза, плотность — энергия.
        // Рисунок внутри секции стабилен, между фразами пересдается.
        let n = role_n[slot.role().index()].max(1) as f32;
        // Сгущение в диады — только мелодическим питчевым при пустых
        // аккордовых. Остальные играют как написано.
        let thicken = !has_chords
            && matches!(
                slot.role(),
                InstrumentRole::Lead | InstrumentRole::Vocal
            )
            && matches!(guide.voice, Voicing::Scale { .. });
        notes.extend(play_guide(
            guide,
            scale,
            sctx.song,
            energy,
            sctx.step_in_measure,
            style.steps_per_measure(),
            sctx.base_step_secs,
            slot.id(),
            i,
            1.0 / n.sqrt(),
            fallback == Some(slot.id()),
            thicken,
            sctx.step,
            kick_fill,
            sctx.rng,
        ));
    }
    notes
}

/// Ранг роли для резервного носителя темы: лид поет первым,
/// бас — последним (держит низ, а не мелодию). `None` — роль
/// тему петь не может (ударные, текстуры).
fn carrier_rank(role: InstrumentRole) -> Option<u8> {
    match role {
        InstrumentRole::Lead => Some(0),
        InstrumentRole::Vocal => Some(1),
        InstrumentRole::Chords => Some(2),
        InstrumentRole::Bass => Some(3),
        _ => None,
    }
}

/// Приоритет якоря: даже даунбит иногда дышит (1 из 20 ударов
/// пропускается гейтом сида). Лицо жанра держится статистически,
/// абсолюта «звучит всегда» больше нет нигде.
const ANCHOR_PRIOR: f32 = 0.95;
/// Соль якорного гейта: независимый бросок от остальных гейтов слота.
const ANCHOR_SALT: u64 = 0xA11CE;

/// Якорный ли шаг для гайда: вес после выбора полюса ≥ 1.0.
/// Та же формула, что в play_guide (без гейтов): бас по ней узнает,
/// даунбит это (играем вместе) или заливка (пригибаемся).
/// Emergent-слоты якорей не имеют по построению.
fn is_anchor_step(
    guide: &RoleGuide,
    song: &SongContext,
    slot_idx: usize,
    step_in_measure: usize,
) -> bool {
    if guide.emergent_beats > 0.0 {
        return false;
    }
    let mut w = guide.weights.get(step_in_measure).copied().unwrap_or(0.0);
    if let Some(poles) = guide.poles.filter(|p| !p.is_empty()) {
        let pick = crate::arranger::splitmix64(
            song.seed_hash ^ (slot_idx as u64).wrapping_mul(0x9E3779B9),
        ) % poles.len() as u64;
        w = poles[pick as usize].get(step_in_measure).copied().unwrap_or(0.0);
    }
    w >= 1.0
}

/// Затухание тонов аккорда: корень в полную силу, верх тише.
const CHORD_ATTEN: [f32; 4] = [1.0, 0.75, 0.65, 0.6];

/// Prior участия акцентных слотов в визитах секций (остальные — всегда).
/// Лазеры, гимны и чопы — эффекты: их отсутствие в части стыков
/// и есть разнообразие сидов, а не поломка.
fn accent_prior(instrument_id: usize) -> f32 {
    match Slot::from_id(instrument_id) {
        Some(Slot::Laser) => 0.7,
        Some(Slot::Festival) => 0.75,
        Some(Slot::Vocal) => 0.75,
        _ => 1.0,
    }
}

/// Хроматический сдвиг тоники от сида (−2..=+2 полутона).
/// Стиль задает лад (семейство интервалов), сид — абсолютную высоту:
/// разные сиды звучат в разных тональностях. Диапазон узкий сознательно:
/// регистры голосов рассчитаны с запасом (бас не уходит в инфраниз,
/// лиды не вылезают за D6). Детерминировано сидом полностью.
pub(crate) fn tonic_shift_for(seed_hash: u64) -> i8 {
    (crate::arranger::splitmix64(seed_hash ^ 0x701C) % 5) as i8 - 2
}

/// Свинг сида: множитель 0.95..1.05 к свингу стиля — у каждого сида
/// свой кач (слышно!), жанр цел (отклонение ±5%). Детерминирован.
pub(crate) fn swing_var(seed_hash: u64) -> f32 {
    let h = crate::arranger::splitmix64(seed_hash ^ 0x5716);
    0.95 + 0.1 * ((h >> 32) as f32 / (u32::MAX as f32))
}

/// Модальный вариант от сида: дорийская секста / фригийская секунда /
/// блюзовая нота. Та же гамма с другим характером.
fn modal_tone(scale: &[u8], idx: usize, seed_hash: u64) -> u8 {
    let root = scale[idx];
    match (scale.len(), (seed_hash % 3) as u8) {
        (7, 1) if idx == 5 => root.saturating_add(1),
        (7, 2) if idx == 1 => root.saturating_sub(1),
        (5, 1) if idx == 3 => root.saturating_add(1),
        _ => root,
    }
}
/// Emergent-пульс: бьет ли глобальный шаг `step`.
/// Решетка с периодом P шагов из сида: звучит, если в шаг попадает
/// очередной тик. P несоизмерим с тактом (дробный, свой у каждого сида),
/// поэтому рисунок дрейфует и не повторяется — фэйзинг вместо паттерна.
/// Период СТАТИЧЕН весь трек (разгона в дропе нет — бит держится);
/// варьируется только сидом ±25%.
/// Возвращает (fire, accent): акцент сильнее у четвертей.
/// Чистая функция — состояния нет, тот же сид дает тот же поток.
fn emergent_fire(
    seed_hash: u64,
    slot_idx: usize,
    step: usize,
    step_in_measure: usize,
    steps_per_measure: usize,
    beats: f32,
) -> (bool, f32) {
    let h = crate::arranger::splitmix64(
        seed_hash ^ (slot_idx as u64).wrapping_mul(0xC0FFEE) ^ 0xE9E9E9,
    );
    let span = 0.75 + 0.5 * ((h >> 32) as f32 / (u32::MAX as f32));
    let mut p = beats * span * steps_per_measure as f32 / 4.0;
    p = p.clamp(1.25, 16.0);
    let s = step as f32;
    let fire = ((s + 1.0) / p).floor() > (s / p).floor();
    let q = steps_per_measure / 4;
    let accent = if q > 0 {
        let d = (step_in_measure % q).min(q - step_in_measure % q) as f32;
        1.0 + 0.35 * (1.0 - d / (q.max(2) / 2) as f32).max(0.0)
    } else {
        1.0
    };
    (fire, accent)
}
/// Детерминированный гейт вероятности: тот же (сид, слот, шаг, соль) —
/// то же решение. Соль берем от монотонной фразы (растет каждые 2 такта),
/// поэтому грув держится фразу и пересдается дальше: повторов нет,
/// но тот же сид всегда дает тот же бесконечный поток (воспроизводимость).
/// В отличие от кубика на каждый шаг, выбранное играет точно.
fn fixed_gate(seed_hash: u64, slot_idx: usize, step: usize, salt: u64) -> f32 {
    let h = crate::arranger::splitmix64(
        seed_hash
            ^ (slot_idx as u64).wrapping_mul(0x9E3779B9)
            ^ (step as u64).wrapping_mul(0x85EBCA6B)
            ^ salt,
    );
    (h >> 32) as f32 / (u32::MAX as f32)
}

/// Детерминированное блуждание тоники по фразам: префикс-сумма шагов
/// −1/0/+1 из битов хэша (сид, фраза). Шаг не больше ступени за фразу
/// (музыкально связно), путь непериодичен (период потока 2^64 —
/// скелет не замыкается никогда). Тот же сид — тот же путь.
/// Сложность O(фраза): на суточной сессии ~микросекунды на ноту.
fn melodic_drift(seed_hash: u64, phrase: usize, span: usize) -> usize {
    let mut pos: i64 = 0;
    for p in 0..phrase {
        let h = crate::arranger::splitmix64(
            seed_hash
                ^ (p as u64).wrapping_mul(0x9E3779B97F4A7C15)
                ^ 0xD21F7,
        );
        pos += match h % 3 {
            0 => 0,
            1 => 1,
            _ => -1,
        };
    }
    pos.rem_euclid(span.max(1) as i64) as usize
}

/// Артикуляция сида: множители атаки/релиза для мягких голосов.
/// Статична на (сид, слот) — «этот сид помягче», а не болтанка внутри
/// трека. Диапазон узкий: характер инструментов цел, сиды различимы.
/// Ударные поля игнорируют (их transient — лицо жанра).
pub(crate) fn envelope_shape(seed_hash: u64, slot_idx: usize) -> (f32, f32) {
    let h = crate::arranger::splitmix64(
        seed_hash ^ (slot_idx as u64).wrapping_mul(0xC0FFEE) ^ 0xA771C04,
    );
    let hi = (h >> 32) as f32 / (u32::MAX as f32);
    let lo = (h & 0xFFFF_FFFF) as f32 / (u32::MAX as f32);
    (0.8 + 1.0 * hi, 0.8 + 0.8 * lo)
}

/// Длительность ноты от сида 0.85..1.25, статична на (сид, слот):
/// одни сиды играют протяжнее, другие отрывистее («медленнее/быстрее»).
/// Ударные короткие по построению — им почти не влияет.
pub(crate) fn duration_shape(seed_hash: u64, slot_idx: usize) -> f32 {
    let h = crate::arranger::splitmix64(
        seed_hash ^ (slot_idx as u64).wrapping_mul(0xC0FFEE) ^ 0xD0A4,
    );
    0.85 + 0.4 * ((h >> 32) as f32 / (u32::MAX as f32))
}

#[allow(clippy::too_many_arguments)]
fn play_guide(
    guide: &RoleGuide,
    scale: &[u8],
    song: &SongContext,
    energy: f32,
    step_in_measure: usize,
    steps_per_measure: usize,
    base_step_secs: f32,
    instrument_id: usize,
    slot_idx: usize,
    // Доля плотности роли (1/sqrt(n) носителей): 1.0 = единственный
    // носитель, играет в полную силу. Якоря (вес 1.0) не масштабируются.
    role_share: f32,
    // Форсированный носитель темы: состав без follow_motif — ступень
    // берется из мотива сида. Озвучка (трезвучия пэдов) и октава гайда
    // сохраняются: пэд играет аккорд от ступени темы, а не мелодию.
    force_motif: bool,
    // Сгущение в диаду: пустые аккордовые — лид поет корень+терцию.
    // Гармония без мути трезвучий и без смены голоса.
    force_chords: bool,
    // Глобальный шаг композиции (монотонный счетчик): emergent-пульс
    // строит решетку от него. Сеточный путь его не читает.
    global_step: usize,
    // Заливка кика на этом шаге (не якорь): бас пригибается вдвое,
    // чтобы не наслаиваться. Даунбит играют вместе в полную.
    kick_fill: bool,
    rng: &mut StdRng,
) -> Vec<NoteEvent> {
    // Вес шага: полюс трека или веса такта. Без морфинга и заливок:
    // плотность секции задают только энергия и структура.
    // Полюс грува: один на весь трек от (сид, слот). Бит прописан
    // и держится (уверенная основа), оживляют его только гейты фраз
    // и свинг сида. Тот же сид — тот же бит; другой сид — другой.
    let base_w = guide.weights.get(step_in_measure).copied().unwrap_or(0.0);
    let weight = if let Some(poles) = guide.poles.filter(|p| !p.is_empty()) {
        let pick = crate::arranger::splitmix64(
            song.seed_hash ^ (slot_idx as u64).wrapping_mul(0x9E3779B9),
        ) % poles.len() as u64;
        poles[pick as usize]
            .get(step_in_measure)
            .copied()
            .unwrap_or(0.0)
    } else {
        base_w
    };
    if weight <= 0.0 {
        return Vec::new();
    }
    if guide.every_n_measures > 1 && !song.measure_index.is_multiple_of(guide.every_n_measures) {
        return Vec::new();
    }
    let climax = song.is_last_measure;
    if guide.drop_before_climax && climax && step_in_measure * 2 > steps_per_measure {
        return Vec::new();
    }
    // Точечные акценты (крэш): только последний такт секции.
    if guide.climax_only && !climax {
        return Vec::new();
    }
    // Акценты звучат не в каждой фразе, а в некоторых:
    // крэш — каждый второй стык, лазеры/гимны/чопы — чаще, но тоже
    // с пропусками. Шаг — фраза (2 такта), а не визит: дырки короткие,
    // инструменты дышат, а не пропадают на всю секцию.
    // Конкретный выбор — гейтом сида от номера фразы,
    // поэтому это вариация сида, а не долбежка из раза в раз.
    // Фундамент (барабаны, бас, пэды, гитара) играет всегда.
    let prior = accent_prior(instrument_id);
    if prior < 1.0
        && fixed_gate(
            song.seed_hash,
            slot_idx,
            song.phrase_index % 64,
            0xA66E47
        ) >= prior
    {
        return Vec::new();
    }

    // Роль слота нужна рано (кик-якоря в гейте): чистая функция id.
    let role = Slot::from_id(instrument_id).map(|s| s.role());

    // Emergent-пульс вместо сетки: бьет ли глобальный шаг.
    // Веса/морфинг/гейты пропускаем; структура выше уже отработала.
    // Акцент у четвертей — барабанщик выделяет доли (грув, а не стук).
    let emergent_accent = if guide.emergent_beats > 0.0 {
        let (fire, accent) = emergent_fire(
            song.seed_hash,
            slot_idx,
            global_step,
            step_in_measure,
            steps_per_measure,
            guide.emergent_beats,
        );
        if !fire {
            return Vec::new();
        }
        accent
    } else {
        1.0
    };

    // Сеточный путь: вероятности + гейт сида. Emergent-слот его
    // пропускает (решение уже принято пульсом выше).
    // Кик-якоря — абсолюти: фундамент бьет всегда, без дыхания.
    // Остальные якоря дышат приоритетом 0.95.
    if guide.emergent_beats <= 0.0 {
        let kick_anchor =
            weight >= 1.0 && matches!(role, Some(InstrumentRole::Kick));
        let probability = if weight >= 1.0 {
            // Якорь — сильный приоритет, а не гарантия: даунбит дышит.
            // Кроме кика: там гарантия.
            ANCHOR_PRIOR
        } else {
            // Плотность сида масштабирует все вероятности: разреженные
            // и густые сиды одного стиля. Доля роли: носителей несколько —
            // каждый реже (делят пространство), один — в полную силу.
            // Якоря идут своим приоритетом и не масштабируются.
            // Заливок больше нет: кульминация — это энергия, а не буст.
            let p =
                weight * energy * energy * guide.energy_response * song.density * role_share;
            p.clamp(0.0, 1.0)
        };
        // Гейт сида: решение зависит от (сид, слот, шаг, фраза), поэтому грув
        // стоит ровно фразу и никогда не повторяется дословно. Якоря идут
        // отдельным броском (своя соль) — пропуск якоря не влечет пропуски
        // остальных шагов и наоборот. Кик-якорь гейт не проходит вовсе.
        let salt = if weight >= 1.0 {
            ANCHOR_SALT ^ (song.phrase_index as u64)
        } else {
            song.phrase_index as u64
        };
        if !kick_anchor
            && probability < 1.0
            && fixed_gate(song.seed_hash, slot_idx, step_in_measure, salt) >= probability
        {
            return Vec::new();
        }
    }

    let mut velocity = (guide.vel_base
        + energy * guide.vel_energy
        + rng.gen_range(-guide.vel_jitter..=guide.vel_jitter))
    .clamp(0.1, 1.0)
        * emergent_accent;

    // Артикуляция сида для мягких голосов: та же нота тем же голосом —
    // с разной мягкостью у разных сидов. Ударные идут как есть.
    let (attack_mul, release_mul) = envelope_shape(song.seed_hash, slot_idx);

    // Оффбитные 8-е: шаг q/2 внутри четверти (q = шагов на четверть).
    // Длительность — от сида: одни сиды протяжнее, другие отрывистее.
    let quarter = steps_per_measure / 4;
    let long = guide.long_offbeats && quarter > 0 && step_in_measure % quarter == quarter / 2;
    let duration = base_step_secs
        * guide.duration_mult
        * duration_shape(song.seed_hash, slot_idx)
        * if long { guide.long_mult } else { 1.0 };

    // Ударные: одиночная нота фиксированного питча, коротким путем.
    if let Voicing::Kit(pitch) = guide.voice {
        if guide.vel_high_cut < 1.0 {
            let octaves_above = ((pitch as f32 - 40.0) / 12.0).max(0.0);
            velocity = (velocity * guide.vel_high_cut.powf(octaves_above)).clamp(0.05, 1.0);
        }
        if let Some(r) = role {
            velocity *= song.stage_gain(r);
        }
        if velocity < 0.05 {
            return Vec::new();
        }
        return vec![NoteEvent::new(instrument_id, pitch, velocity, duration)];
    }

    let (tone_base, octaves) = match guide.voice {
        Voicing::Kit(_) => unreachable!("kit handled above"),
        Voicing::Scale {
            degree,
            octave,
            jitter,
        } => {
            if scale.is_empty() {
                return Vec::new();
            }
            // Ротация лада от сида: тот же набор звуков, другой тоновый
            // центр — сиды звучат в разных «тональностях», диапазон цел.
            let rotation = (song.seed_hash % scale.len() as u64) as usize;
            // Мотив сида: узнаваемая тема вместо блуждания по ладу.
            // Ротация применяется и к нему — тема развивается с гармонией.
            // Аккомпанемент (не мотив) движется по прогрессии сида:
            // аккорды меняются каждые 2 такта, оставаясь в ладу.
            // Форсированный носитель идет тем же путем темы, что и родной.
            let use_motif = guide.follow_motif || force_motif;
            let prog_shift = if use_motif {
                0
            } else {
                // Аккомпанемент ползет за прогрессией вдвое медленнее баса:
                // бас держит корни почти стоя (иначе гуляет по грифу
                // и ощущается то высоким, то низким). Остальные — в полную.
                let shift = song.prog[(song.measure_index / 2) % song.prog.len()] as usize;
                if matches!(role, Some(InstrumentRole::Bass)) {
                    shift / 2
                } else {
                    shift
                }
            };
            // Вопрос-ответ: нечетные фразы отвечают транспозицией темы
            // на терцию вверх — та же тема, другое дыхание.
            let answer = if use_motif && song.phrase_index % 2 == 1 {
                2
            } else {
                0
            };
            let (degree, jitter) = if use_motif {
                // Живой мотив: тема секции (A везде, B на Drop/PostChorus)
                // плюс ротация окном фразы. ДНК сида сохранена, но дословных повторов нет.
                let theme = if song.use_alt_motif {
                    &song.motif_b
                } else {
                    &song.motif
                };
                let rot = song.phrase_index % theme.len();
                let motif_step =
                    theme[(step_in_measure / 2 + rot) % theme.len()] as usize;
                (motif_step + answer, 0)
            } else {
                let mut d = (degree + prog_shift) % scale.len().max(1);
                // Квинтовый бас: оффбитные восьмые ходят на квинту —
                // баслайн движется, а не топчется на тонике.
                if guide.bass_fifth && matches!(role, Some(InstrumentRole::Bass)) {
                    let eighth = steps_per_measure / 8;
                    if eighth > 0 && step_in_measure % (eighth * 2) == eighth {
                        let fifth = if scale.len() == 7 { 4 } else { 3 };
                        d = (d + fifth) % scale.len().max(1);
                    }
                }
                (d, jitter)
            };
            let drift = if jitter > 0 {
                // Блуждание тоже фиксировано сидом и фразой: внутри фразы
                // та же нота в том же месте, дальше — пересдача.
                (crate::arranger::splitmix64(
                    song.seed_hash
                        ^ (slot_idx as u64).wrapping_mul(0xC0FFEE)
                        ^ (step_in_measure as u64).wrapping_mul(0x9E3779B9)
                        ^ (song.phrase_index as u64).wrapping_mul(0x85EBCA6B)
                        ^ 0xD1F7,
                ) % (jitter as u64 + 1)) as usize
            } else {
                0
            };
            // Тоника блуждает детерминированно (−1/0/+1 по битам хэша фраз):
            // локально связно (стоит или шаг на ступень), глобально
            // непериодично — скелет мелодии не замыкается в цикл длиной
            // в лад, как раньше (там было +1 каждую фразу: период = лад).
            let tonic = melodic_drift(song.seed_hash, song.phrase_index, scale.len());
            let base = tonic + degree + drift + rotation;
            // Октава общая для тонов аккорда; подъем проверяем по корню.
            let root_idx = base % scale.len();
            // Абсолютная высота — от сида (тон семейства + сдвиг тоники).
            let shift = tonic_shift_for(song.seed_hash);
            let root = modal_tone(scale, root_idx, song.seed_hash).saturating_add_signed(shift);
            let bump = guide.octave_up > 0.0
                // Потолок регистра лидов: выше D6 (86) — ледяной свист,
                // а не музыка. Берем минимум из гайда и колпака.
                && root.saturating_add(12) <= guide.top_pitch.min(86)
                && fixed_gate(
                    song.seed_hash,
                    slot_idx,
                    step_in_measure,
                    0x0C74 ^ (song.phrase_index as u64),
                ) < guide.octave_up;
            (base, octave + if bump { 1 } else { 0 })
        }
    };

    // Аккордовая озвучка: пэды и клавиши берут трезвучие (или интервалы
    // из гайда), мелодия — одиночные ноты. Сгущение (пустые аккордовые) —
    // диада корень+терция у лидов: гармония без мути. Все тоны в ладу.
    // Арпеджио: один интервал за удар, вкруговую (фингерпикинг).
    let intervals: &[i8] = match guide.chord {
        Some(iv) => iv,
        None if matches!(role, Some(InstrumentRole::Chords)) && !guide.follow_motif => &[0, 2, 4],
        None if force_chords => &[0, 2],
        _ => &[0],
    };
    let arp_one;
    let intervals: &[i8] = if guide.arp && intervals.len() > 1 {
        arp_one = [intervals[(step_in_measure / 2) % intervals.len()]];
        &arp_one
    } else {
        intervals
    };
    let mut out = Vec::with_capacity(intervals.len());
    for (ti, iv) in intervals.iter().enumerate() {
        let idx = (tone_base as i32 + *iv as i32).rem_euclid(scale.len() as i32) as usize;
        let mut tone = modal_tone(scale, idx, song.seed_hash)
            .saturating_add_signed(tonic_shift_for(song.seed_hash))
            .saturating_add_signed(octaves.saturating_mul(12));
        // Низ держим внизу: уползшее выше капа складываем октавой вниз.
        // Гармония та же (тот же питч-класс). Кап от гайда: саб 45,
        // рок риффует выше (52) вслед за гитарой. Питчевый кик (808)
        // ведет баслайн — тот же регистровый закон, что у баса:
        // ударные-киты (фиксированный питч) это не трогает.
        let low_fold = matches!(role, Some(InstrumentRole::Bass))
            || (matches!(role, Some(InstrumentRole::Kick))
                && matches!(guide.voice, Voicing::Scale { .. }));
        if low_fold {
            while tone > guide.bass_top {
                tone -= 12;
            }
        }
        let atten = if ti < CHORD_ATTEN.len() {
            CHORD_ATTEN[ti]
        } else {
            0.5
        };
        let mut v = velocity * atten;
        // Приглушение высоких нот — по каждому тону отдельно.
        if guide.vel_high_cut < 1.0 {
            let octaves_above = ((tone as f32 - 40.0) / 12.0).max(0.0);
            v = (v * guide.vel_high_cut.powf(octaves_above)).clamp(0.05, 1.0);
        }
        // Громкость фразы: баланс партий по ступени круга.
        // Ноль таблицы = тишина (снейра в интро нет — и не должно быть).
        if let Some(r) = role {
            v *= song.stage_gain(r);
        }
        // Оркестровка: бас знает роль кика — под заливки пригибается
        // вдвое, даунбит держит вместе. Инструменты не наслаиваются.
        if matches!(role, Some(InstrumentRole::Bass)) && kick_fill {
            v *= 0.5;
        }
        if v >= 0.05 {
            let mut note = NoteEvent::new(instrument_id, tone, v, duration)
                .shaped(attack_mul, release_mul);
            note.sub_bias = guide.sub_bias.clamp(0.0, 1.0);
            out.push(note);
        }
    }
    out
}

/// Фабрика стилей по имени (регистр и пробелы игнорируются).
/// `None` для неизвестных имен — вызывающий предупреждает пользователя
/// вместо тихого включения не того стиля.
pub fn make_style(name: &str) -> Option<Box<dyn Style>> {
    use club::ClubStyle;
    use lofi::LofiStyle;
    use phonk::PhonkStyle;
    use rock::RockStyle;
    match name.trim().to_lowercase().as_str() {
        "club" => Some(Box::new(ClubStyle::new())),
        "lofi" => Some(Box::new(LofiStyle::new())),
        "rock" => Some(Box::new(RockStyle::new())),
        "phonk" => Some(Box::new(PhonkStyle::new())),
        _ => None,
    }
}

/// Все доступные стили: имена для CLI и Flutter.
pub fn style_names() -> &'static [&'static str] {
    &["club", "rock", "lofi", "phonk"]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arranger::SongSection;

    fn test_song(progress: f32) -> SongContext {
        SongContext {
            section: SongSection::MainMotif,
            measure_index: 1,
            phrase_index: 0,
            energy: 1.0,
            progress,
            is_last_measure: false,
            stage: 1,
            cycle: 0,
            stage_gains: [1.0; 8],
            seed_hash: 42,
            density: 1.0,
            motif: [0, 2, 4, 2, 0, 3, 1, 4],
            motif_b: [1, 4, 0, 3, 2, 4, 2, 0],
            use_alt_motif: false,
            prog: [0, 0, 0, 0],
        }
    }

    #[test]
    fn energy_shapes_are_sane() {
        // Кривая каждого стиля: монотонна, в разумных пределах,
        // дроп (1.0) не тише интро (0.0).
        for name in style_names() {
            let style = make_style(name).expect("known style");
            let lo = style.shape_energy(0.0);
            let hi = style.shape_energy(1.0);
            let mid = style.shape_energy(0.5);
            assert!(lo.is_finite() && hi.is_finite(), "{name}: non-finite");
            assert!(
                (0.0..=1.2).contains(&lo) && (0.0..=1.2).contains(&hi),
                "{name}: out of range ({lo}, {hi})"
            );
            assert!(hi >= lo, "{name}: inverted curve");
            assert!(
                mid >= lo && mid <= hi,
                "{name}: non-monotonic ({lo}, {mid}, {hi})"
            );
        }
    }

    #[test]
    fn guide_data_matches_grid() {
        // Веса обязаны совпадать с сеткой стиля,
        // иначе шаги молча гаснут (.get → None → 0.0).
        // Вайтлист — только из известных слотов.
        for name in style_names() {
            let style = make_style(name).expect("known style");
            let spm = style.steps_per_measure();
            for slot in style.ensemble() {
                assert!(Slot::ALL.contains(slot), "{name}: unknown slot in ensemble");
            }
            for (i, guide) in style.slot_guides().iter().enumerate() {
                assert_eq!(guide.weights.len(), spm, "{name} slot {i}: weights len");
                if let Some(poles) = guide.poles {
                    assert!(!poles.is_empty(), "{name} slot {i}: empty poles");
                    for (j, pole) in poles.iter().enumerate() {
                        assert_eq!(pole.len(), spm, "{name} slot {i} pole {j}: len");
                    }
                }
            }
        }
    }

    fn motif_pitches(motif: [u8; 8]) -> Vec<u8> {
        let guide = RoleGuide {
            weights: &[1.0; 16],
            follow_motif: true,
            ..RoleGuide::pitched(&[1.0; 16], 0, 2, 0, 1.0, 0.8)
        };
        let scale = [48, 50, 52, 53, 55, 57, 59];
        let mut song = test_song(0.5);
        song.motif = motif;
        song.stage_gains = [1.0; 8];
        let mut rng = rand::SeedableRng::seed_from_u64(3);
        (0..8)
            .filter_map(|step| {
                play_guide(
                    &guide,
                    &scale,
                    &song,
                    1.0,
                    step * 2,
                    16,
                    0.1,
                    6,
                    5,
                    1.0,
                    false,
                    false,
                    0, false, &mut rng,
                )
                .into_iter()
                .next()
                .map(|n| n.midi_pitch)
            })
            .collect()
    }

    #[test]
    fn alt_motif_switches_theme() {
        // Тот же лид, разные темы A/B — разные мелодии.
        // use_alt_motif переключает массив, остальное одинаково.
        let a = motif_pitches([0, 2, 4, 2, 0, 3, 1, 4]);
        let mut song_b = test_song(0.5);
        song_b.motif = [0, 2, 4, 2, 0, 3, 1, 4];
        song_b.motif_b = [4, 4, 0, 0, 2, 2, 4, 4];
        song_b.use_alt_motif = true;
        let guide = RoleGuide {
            weights: &[1.0; 16],
            follow_motif: true,
            ..RoleGuide::pitched(&[1.0; 16], 0, 2, 0, 1.0, 0.8)
        };
        let scale = [48, 50, 52, 53, 55, 57, 59];
        let mut rng = rand::SeedableRng::seed_from_u64(3);
        let b: Vec<u8> = (0..8)
            .filter_map(|step| {
                play_guide(
                    &guide,
                    &scale,
                    &song_b,
                    1.0,
                    step * 2,
                    16,
                    0.1,
                    6,
                    5,
                    1.0,
                    false,
                    false,
                    0, false, &mut rng,
                )
                .into_iter()
                .next()
                .map(|n| n.midi_pitch)
            })
            .collect();
        assert_eq!(a.len(), 8);
        assert_eq!(b.len(), 8);
        assert_ne!(a, b, "alt motif ignored");
    }

    #[test]
    fn motif_shapes_melody() {
        // Один и тот же лид с разными мотивами дает разные мелодии —
        // мотив сида слышно.
        let a = motif_pitches([0, 2, 4, 2, 0, 3, 1, 4]);
        let b = motif_pitches([4, 4, 4, 4, 4, 4, 4, 4]);
        assert_eq!(a.len(), 8);
        assert_ne!(a, b, "motif ignored");
        // Детерминизм: тот же мотив — те же ноты.
        assert_eq!(a, motif_pitches([0, 2, 4, 2, 0, 3, 1, 4]));
    }

    const POLE_A: &[f32] = &[1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0];
    const POLE_B: &[f32] = &[0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0];
    const POLES: &[&[f32]] = &[POLE_A, POLE_B];

    fn pole_steps(seed_hash: u64, slot_idx: usize) -> Vec<usize> {
        let guide = RoleGuide {
            weights: &[0.0; 8],
            poles: Some(POLES),
            ..RoleGuide::kit(&[0.0; 8], 36, 0.9)
        };
        let mut song = test_song(0.0);
        song.seed_hash = seed_hash;
        let mut rng = rand::SeedableRng::seed_from_u64(1);
        (0..8)
            .filter(|step| {
                !play_guide(
                    &guide, &[48], &song, 1.0, *step, 8, 0.1, 2, slot_idx, 1.0, false, false, 0, false, &mut rng,
                )
                .is_empty()
            })
            .collect()
    }

    #[test]
    fn pads_play_triads() {
        // Пэд с ролью Chords звучит трезвучием, а не одиночной нотой.
        let guide = RoleGuide {
            weights: &[1.0; 8],
            ..RoleGuide::pitched(&[1.0; 8], 0, 0, 0, 4.0, 0.6)
        };
        let scale = [48, 50, 52, 53, 55, 57, 59];
        let song = test_song(0.5);
        let mut rng = rand::SeedableRng::seed_from_u64(9);
        // id 3 = Pad (роль Chords), индекс слота 2.
        let notes = play_guide(&guide, &scale, &song, 1.0, 0, 8, 0.1, 3, 2, 1.0, false, false, 0, false, &mut rng);
        assert_eq!(notes.len(), 3, "pad must voice a triad");
        let mut pitches: Vec<u8> = notes.iter().map(|n| n.midi_pitch).collect();
        pitches.sort_unstable();
        // Терция и квинта выше корня, все в пределах октавы.
        assert!(pitches[1] > pitches[0] && pitches[2] > pitches[1]);
        assert!(
            pitches[2] - pitches[0] <= 12,
            "triad too wide: {pitches:?}"
        );
    }

    #[test]
    fn accents_skip_some_visits() {
        // Акценты с prior звучат не в каждой фразе, но и не никогда:
        // вариация сида, а не долбежка и не тишина. Шаг — фраза.
        let guide = RoleGuide {
            climax_only: true,
            ..RoleGuide::kit(&[1.0; 8], 74, 0.8)
        };
        let mut hits = 0;
        for phrase in 0..17usize {
            let mut song = test_song(0.9);
            song.is_last_measure = true;
            song.phrase_index = phrase;
            let mut rng = rand::SeedableRng::seed_from_u64(5);
            // id 9 = Laser (prior 0.7), индекс слота 8 (см. Slot::ALL).
            if !play_guide(&guide, &[48], &song, 1.0, 0, 8, 0.1, 9, 8, 1.0, false, false, 0, false, &mut rng)
                .is_empty()
            {
                hits += 1;
            }
        }
        assert!(hits > 0 && hits < 17, "laser phrases: {hits}/17");
    }

    #[test]
    fn leads_never_exceed_d6() {
        // Ледяной свист запрещен: лиды и вокал всех стилей ≤ 86 (D6).
        use crate::instruments::{InstrumentRole, Slot};
        for name in style_names() {
            let style = make_style(name).expect("known style");
            let ensemble = style.ensemble();
            let active: Vec<usize> = Slot::ALL
                .iter()
                .filter(|s| ensemble.contains(s))
                .map(|s| s.id())
                .collect();
            let mut rng = rand::SeedableRng::seed_from_u64(424242);
            let mut arranger = crate::arranger::Arranger::new(424242);
            let mut song = arranger.advance_measure();
            for step in 0..128 {
                let spm = style.steps_per_measure();
                let sim = step % spm;
                if sim == 0 && step > 0 {
                    song = arranger.advance_measure();
                }
                let mut sctx = StepContext {
                    step,
                    step_in_measure: sim,
                    base_step_secs: style.base_step_secs(),
                    active_tracks: &active,
                    song: &song,
                    rng: &mut rng,
                    intensity: 1.0,
                };
                for note in render_style(&*style, &mut sctx) {
                    let role = Slot::from_id(note.instrument_id).map(|s| s.role());
                    if matches!(
                        role,
                        Some(InstrumentRole::Lead | InstrumentRole::Vocal)
                    ) {
                        assert!(
                            note.midi_pitch <= 86,
                            "{name}: lead too high: {}",
                            note.midi_pitch
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn bass_stays_deep() {
        // Саб внизу: ноты баса в регистре капа стиля (саб 45,
        // рок риффует выше — 52). Уползшие складываются октавой вниз.
        use crate::instruments::{InstrumentRole, Slot};
        for (name, cap) in [("club", 45), ("rock", 52), ("lofi", 45)] {
            let style = make_style(name).expect("known style");
            let ensemble = style.ensemble();
            let active: Vec<usize> = Slot::ALL
                .iter()
                .filter(|s| ensemble.contains(s))
                .map(|s| s.id())
                .collect();
            let mut rng = rand::SeedableRng::seed_from_u64(424242);
            let mut arranger = crate::arranger::Arranger::new(424242);
            let mut song = arranger.advance_measure();
            let mut heard = 0;
            for step in 0..256 {
                let spm = style.steps_per_measure();
                let sim = step % spm;
                if sim == 0 && step > 0 {
                    song = arranger.advance_measure();
                }
                let mut sctx = StepContext {
                    step,
                    step_in_measure: sim,
                    base_step_secs: style.base_step_secs(),
                    active_tracks: &active,
                    song: &song,
                    rng: &mut rng,
                    intensity: 1.0,
                };
                for note in render_style(&*style, &mut sctx) {
                    let role = Slot::from_id(note.instrument_id).map(|s| s.role());
                    if matches!(role, Some(InstrumentRole::Bass)) {
                        assert!(
                            (24..=cap).contains(&note.midi_pitch),
                            "{name}: bass out of range: {}",
                            note.midi_pitch
                        );
                        heard += 1;
                    }
                }
            }
            assert!(heard > 0, "{name}: bass never plays");
        }
    }

    #[test]
    fn poles_pick_groove_by_seed() {
        // Один полюс на весь трек: шаги только из POLE_A
        // или только из POLE_B. Бит прописан и держится.
        for seed_hash in [0u64, 1, 7, 42, 1000] {
            let steps = pole_steps(seed_hash, 1);
            let in_a = steps.iter().all(|s| POLE_A[*s] > 0.0);
            let in_b = steps.iter().all(|s| POLE_B[*s] > 0.0);
            assert!(in_a || in_b, "seed {seed_hash}: mixed poles {steps:?}");
        }
        // Разные сиды выбирают разные полюса.
        let grooves: std::collections::HashSet<Vec<usize>> =
            (0..8u64).map(|s| pole_steps(s, 1)).collect();
        assert!(grooves.len() > 1, "pole never varies");
        // Тот же сид = тот же грув (повторяемость, бит прописан).
        assert_eq!(pole_steps(42, 1), pole_steps(42, 1));
    }

    #[test]
    fn tonic_walk_is_bounded_and_never_loops() {
        // Тоника: старт в нуле, детерминизм, шаг ≤ ступень между фразами.
        assert_eq!(melodic_drift(42, 0, 7), 0);
        assert_eq!(melodic_drift(42, 5, 7), melodic_drift(42, 5, 7));
        for seed in [0u64, 1, 42, 777] {
            for ph in 0..200usize {
                let a = melodic_drift(seed, ph, 7) as i64;
                let b = melodic_drift(seed, ph + 1, 7) as i64;
                let d = (b - a).rem_euclid(7).min((a - b).rem_euclid(7));
                assert!(d <= 1, "seed {seed} phrase {ph}: jump {d}");
            }
        }
        // Непериодичность: на окне 200 фраз нет периода ≤ 32
        // (линейное +1/фраза дало бы период 7 — цикл длиной в лад).
        for seed in [5u64, 12345] {
            let path: Vec<usize> = (0..200).map(|ph| melodic_drift(seed, ph, 7)).collect();
            let loops = (1..=32)
                .any(|t| path.iter().zip(path.iter().skip(t)).all(|(a, b)| a == b));
            assert!(!loops, "seed {seed}: walk loops");
        }
    }

    #[test]
    fn motif_skeleton_outgrows_rotation_templates() {
        // Линия мотива: та же фраза — та же линия; соседние фразы разные;
        // за 64 фразы различных линий больше 16 (потолок ротации 8 ×
        // вопрос-ответ 2) — блуждание тоники рвет шаблонный цикл.
        fn phrase_pitches(phrase: usize) -> Vec<u8> {
            let guide = RoleGuide {
                weights: &[1.0; 16],
                follow_motif: true,
                ..RoleGuide::pitched(&[1.0; 16], 0, 2, 0, 1.0, 0.8)
            };
            let scale = [48, 50, 52, 53, 55, 57, 59];
            let mut song = test_song(0.5);
            song.phrase_index = phrase;
            let mut rng = rand::SeedableRng::seed_from_u64(3);
            (0..8)
                .filter_map(|step| {
                    play_guide(
                        &guide, &scale, &song, 1.0, step * 2, 16, 0.1, 6, 5, 1.0, false,
                        false, 0, false, &mut rng,
                    )
                    .into_iter()
                    .next()
                    .map(|n| n.midi_pitch)
                })
                .collect()
        }
        let p0 = phrase_pitches(0);
        let p1 = phrase_pitches(1);
        assert_eq!(p0.len(), 8);
        assert_ne!(p0, p1, "motif stuck across phrases");
        assert_eq!(p0, phrase_pitches(0), "same phrase must repeat");
        // Блуждание рвет шаблон: различных линий за 64 фразы больше,
        // чем может дать ротация без него.
        let distinct: std::collections::HashSet<Vec<u8>> =
            (0..64).map(phrase_pitches).collect();
        assert!(
            distinct.len() > 16,
            "skeleton loops: {} distinct",
            distinct.len()
        );
    }

    #[test]
    fn motif_fallback_hands_theme_to_first_melodic_slot() {
        // Мьют всех носителей темы не убивает мотив: клавиши (не носитель)
        // с подхватом поют ту же тему, что гитара, пошагово.
        use crate::instruments::Slot;
        let style = make_style("lofi").expect("known style");
        let guides = style.slot_guides();
        let scale = style.scale();
        let spm = style.steps_per_measure();
        let ki = Slot::ALL.iter().position(|s| *s == Slot::Keys).unwrap();
        let song = test_song(0.5);
        // Эталонная линия мотива: якорный носитель звучит на каждом шаге.
        let anchor_motif = RoleGuide {
            weights: &[1.0; 16],
            follow_motif: true,
            ..RoleGuide::pitched(&[1.0; 16], 0, 1, 0, 1.0, 0.8)
        };
        let mut rng = rand::SeedableRng::seed_from_u64(21);
        let full_line: Vec<u8> = (0..16)
            .map(|step| {
                play_guide(
                    &anchor_motif, scale, &song, 1.0, step, spm, 0.1, 6, 5, 1.0,
                    false, false, 0, false, &mut rng,
                )
                .into_iter()
                .next()
                .map(|n| n.midi_pitch)
                .unwrap()
            })
            .collect();
        // Клавиши без подхвата и с подхватом — разные линии.
        let line = |force: bool| -> Vec<(usize, u8)> {
            let mut rng = rand::SeedableRng::seed_from_u64(22);
            (0..16)
                .filter_map(|step| {
                    play_guide(
                        &guides[ki], scale, &song, 1.0, step, spm, 0.1,
                        Slot::Keys.id(), ki, 1.0, force, false, 0, false, &mut rng,
                    )
                    .into_iter()
                    .next()
                    .map(|n| (step, n.midi_pitch))
                })
                .collect()
        };
        let plain = line(false);
        let forced = line(true);
        assert!(!forced.is_empty(), "fallback silent");
        assert_ne!(plain, forced, "fallback ignored");
        for (step, pitch) in &forced {
            assert_eq!(*pitch, full_line[*step], "step {step}: not the motif");
        }
        // Уровень движка: активны только клавиши — подхват включается сам,
        // корень аккорда на каждом шаге лежит на линии мотива
        // (верхние терции трезвучия — гармонизация темы, а не уход с нее).
        let active = vec![Slot::Keys.id()];
        let mut rng = rand::SeedableRng::seed_from_u64(23);
        let mut sctx = StepContext {
            step: 0,
            step_in_measure: 0,
            base_step_secs: style.base_step_secs(),
            active_tracks: &active,
            song: &song,
            rng: &mut rng,
            intensity: 1.0,
        };
        let mut heard = 0;
        for (sim, expected) in full_line.iter().enumerate() {
            sctx.step = sim;
            sctx.step_in_measure = sim;
            let mut step_pitches = Vec::new();
            for note in render_style(&*style, &mut sctx) {
                assert_eq!(note.instrument_id, Slot::Keys.id());
                step_pitches.push(note.midi_pitch);
            }
            if !step_pitches.is_empty() {
                // Первая нота озвучки — корень: тема на месте.
                assert_eq!(
                    step_pitches[0], *expected,
                    "render step {sim}: motif lost"
                );
                heard += step_pitches.len();
            }
        }
        assert!(heard > 0, "keys alone play nothing");
    }

    #[test]
    fn empty_chords_thicken_leads_into_dyads() {
        // Пустые аккордовые: плак соло поет диадами (корень+терция),
        // тот же плак при живом пэде — одиночными. Те же шаги звучат:
        // состав меняет озвучку, а не ритм. (Гитара теперь всегда
        // трезвучиями по своему гайду — ее фолбэк не касается.)
        use crate::instruments::Slot;
        let style = make_style("lofi").expect("known style");
        let gid = Slot::Pluck.id();
        // По фразам, как живой движок: якоря иногда дышат мимо.
        let render = |active: &Vec<usize>| -> Vec<(usize, Vec<u8>)> {
            let mut rng = rand::SeedableRng::seed_from_u64(31);
            let mut out = Vec::new();
            for phrase in 0..4usize {
                let mut song = test_song(0.5);
                song.phrase_index = phrase;
                let mut sctx = StepContext {
                    step: 0,
                    step_in_measure: 0,
                    base_step_secs: style.base_step_secs(),
                    active_tracks: active,
                    song: &song,
                    rng: &mut rng,
                    // Погромче: тихий плак проверяем на озвучку, не на гейты
                    // (они от intensity не зависят — сравнение честное).
                    intensity: 2.0,
                };
                for sim in 0..16 {
                    sctx.step = sim;
                    sctx.step_in_measure = sim;
                    let pitches: Vec<u8> = render_style(&*style, &mut sctx)
                        .into_iter()
                        .filter(|n| n.instrument_id == gid)
                        .map(|n| n.midi_pitch)
                        .collect();
                    out.push((phrase * 16 + sim, pitches));
                }
            }
            out
        };
        let solo = render(&vec![gid]);
        let with_pad = render(&vec![gid, Slot::Pad.id()]);
        let solo_sounding: Vec<_> = solo.iter().filter(|(_, p)| !p.is_empty()).collect();
        assert!(!solo_sounding.is_empty(), "pluck alone silent");
        for (sim, pitches) in &solo_sounding {
            assert_eq!(
                pitches.len(),
                2,
                "solo step {sim}: expected dyad, got {pitches:?}"
            );
            assert_ne!(pitches[0], pitches[1], "dyad in unison");
        }
        // Тот же сид, те же шаги — при пэде одиночные ноты.
        for ((sim, dyad), (_, single)) in solo.iter().zip(with_pad.iter()) {
            if dyad.is_empty() {
                assert!(single.is_empty(), "step {sim}: gate moved with mix");
            } else {
                assert_eq!(single.len(), 1, "step {sim}: no thinning with pad");
                assert_eq!(single[0], dyad[0], "step {sim}: root moved");
            }
        }
        assert_eq!(solo, render(&vec![gid]), "must repeat");
    }

    #[test]
    fn lofi_kick_stays_sparse() {
        // Бум-бэп: кик на 1 + пара синкоп, не пулемет. Не больше 4 ударов
        // кика и не больше 6 низовых атак (кик+808+бас) на такт — в любых
        // фразах, сидах и при госте-808 (удвоение паттерна запрещено).
        // Худший случай: полный дефолтный микс сида + пик энергии формы.
        use crate::instruments::Slot;
        let style = make_style("lofi").expect("known style");
        for seed in [11u64, 12, 42, 777, 424242] {
            let mut mixer = crate::mixer::MixerState::new();
            crate::playback::apply_style_mix(&*style, &mut mixer, seed);
            let mut buf = [0usize; crate::core::MAX_ACTIVE_TRACKS];
            let n = mixer.active_ids(&mut buf);
            let active = buf[..n].to_vec();
            let mut rng = rand::SeedableRng::seed_from_u64(seed);
            let mut arranger = crate::arranger::Arranger::new(seed);
            let mut song = arranger.advance_measure();
            for _bar in 0..24 {
                let (mut kicks, mut lowend) = (0, 0);
                {
                    let mut sctx = StepContext {
                        step: 0,
                        step_in_measure: 0,
                        base_step_secs: style.base_step_secs(),
                        active_tracks: &active,
                        song: &song,
                        rng: &mut rng,
                        intensity: 1.0,
                    };
                    for sim in 0..16 {
                        sctx.step = sim;
                        sctx.step_in_measure = sim;
                        for note in render_style(&*style, &mut sctx) {
                            if note.instrument_id == Slot::Kick.id() {
                                kicks += 1;
                                lowend += 1;
                            } else if note.instrument_id == Slot::Kick808.id()
                                || note.instrument_id == Slot::Bass.id()
                            {
                                lowend += 1;
                            }
                        }
                    }
                }
                assert!(kicks <= 4, "seed {seed}: {kicks} kicks/bar");
                assert!(lowend <= 6, "seed {seed}: {lowend} low-end/bar");
                song = arranger.advance_measure();
            }
        }
    }

    #[test]
    fn seed_articulation_varies_and_sticks_to_notes() {
        // Артикуляция: в диапазоне, детерминирована (сид, слот),
        // различается между сидами и цепляется к нотам гайда.
        for seed in [0u64, 1, 42, 777, 424242] {
            for slot in 0..8usize {
                let (a, r) = envelope_shape(seed, slot);
                assert!((0.8..=1.8).contains(&a), "attack out of range: {a}");
                assert!((0.8..=1.6).contains(&r), "release out of range: {r}");
                assert_eq!((a, r), envelope_shape(seed, slot), "not deterministic");
            }
        }
        let shapes: std::collections::HashSet<(u32, u32)> = (0..16u64)
            .map(|s| {
                let (a, r) = envelope_shape(s, 2);
                ((a * 1000.0) as u32, (r * 1000.0) as u32)
            })
            .collect();
        assert!(shapes.len() > 8, "seeds sound alike");
        // Пин: сид 83 — самый мягкий бас из первых 300 (атака ~1.8).
        // Хэш-путь честный: seed_hash = splitmix(seed ^ 0x51ED), как в Arranger.
        // Если путь поменяется, пин упадет.
        let h83 = crate::arranger::splitmix64(83 ^ 0x51ED);
        let (soft_a, _) = envelope_shape(h83, 0);
        assert!(soft_a > 1.7, "soft-seed pin moved: {soft_a}");
        // Гайд цепляет shaping сида к ноте.
        let guide = RoleGuide {
            weights: &[1.0; 8],
            ..RoleGuide::pitched(&[1.0; 8], 0, 0, 0, 4.0, 0.6)
        };
        let song = test_song(0.5);
        let mut rng = rand::SeedableRng::seed_from_u64(9);
        let notes = play_guide(
            &guide, &[48, 50, 52, 53, 55, 57, 59], &song, 1.0, 0, 8, 0.1, 3,
            2, 1.0, false, false, 0, false, &mut rng,
        );
        assert!(!notes.is_empty());
        let (a, r) = envelope_shape(42, 2);
        assert_eq!((notes[0].attack_mul, notes[0].release_mul), (a, r));
    }

    #[test]
    fn sub_bias_rides_from_guide_to_note() {
        // Перекос стиля едет в ноту (club-бас 0.8), дефолт — 0.0.
        use crate::instruments::Slot;
        use club::ClubStyle;
        let guides = ClubStyle::new().slot_guides();
        let bi = Slot::ALL.iter().position(|s| *s == Slot::Bass).unwrap();
        assert_eq!(guides[bi].sub_bias, 0.8);
        let mut guide = RoleGuide {
            weights: &[1.0; 8],
            sub_bias: 0.8,
            ..RoleGuide::pitched(&[1.0; 8], 0, 0, 0, 1.0, 0.8)
        };
        let song = test_song(0.5);
        let mut rng = rand::SeedableRng::seed_from_u64(9);
        let notes = play_guide(
            &guide, &[48, 50, 52, 53, 55, 57, 59], &song, 1.0, 0, 8, 0.1, 1,
            0, 1.0, false, false, 0, false, &mut rng,
        );
        assert!(!notes.is_empty());
        assert_eq!(notes[0].sub_bias, 0.8);
        guide.sub_bias = 0.0;
        let mut rng = rand::SeedableRng::seed_from_u64(9);
        let notes = play_guide(
            &guide, &[48, 50, 52, 53, 55, 57, 59], &song, 1.0, 0, 8, 0.1, 1,
            0, 1.0, false, false, 0, false, &mut rng,
        );
        assert_eq!(notes[0].sub_bias, 0.0);
    }

    #[test]
    fn rock_guitar_cuts_through_mix() {
        // Рифф слышно: гитара дает не меньше 24 нот за 24 такта дефолта.
        // Было: в 40 раз тише баса (замолчена фаззом, октавой и уровнем).
        use crate::instruments::Slot;
        let style = make_style("rock").expect("rock");
        let seed = 5u64;
        let mut mixer = crate::mixer::MixerState::new();
        crate::playback::apply_style_mix(&*style, &mut mixer, seed);
        let mut buf = [0usize; crate::core::MAX_ACTIVE_TRACKS];
        let n = mixer.active_ids(&mut buf);
        let active = buf[..n].to_vec();
        let mut rng = rand::SeedableRng::seed_from_u64(seed);
        let mut arranger = crate::arranger::Arranger::new(seed);
        let mut song = arranger.advance_measure();
        let mut heard = 0;
        for _bar in 0..24 {
            let mut sctx = StepContext {
                step: 0,
                step_in_measure: 0,
                base_step_secs: style.base_step_secs(),
                active_tracks: &active,
                song: &song,
                rng: &mut rng,
                intensity: 1.0,
            };
            for sim in 0..16 {
                sctx.step = sim;
                sctx.step_in_measure = sim;
                heard += render_style(&*style, &mut sctx)
                    .into_iter()
                    .filter(|nn| nn.instrument_id == Slot::Guitar.id())
                    .count();
            }
            song = arranger.advance_measure();
        }
        assert!(heard >= 24, "riff inaudible: {heard}/24 bars");
    }

    #[test]
    fn style_contract_holds_across_styles() {
        // Контракт фазы 1: лицо каждого стиля как измеримые свойства
        // дефолтного микса за 24 такта. Свобода реализации внутри —
        // любой рефактор обязан держать эти числа (с запасом ~2x вниз
        // от замера: ловят поломку, а не вкус).
        // Замеры: club kickmax 6, rock 5, lofi 2–3; роли и тема — в PROBE.
        use crate::instruments::Slot;
        use std::collections::HashMap;
        // (стиль, потолок кика на такт, полы ролей за 24 такта, пол темы)
        // Потолки — по самому плотному намеренному полюсу + запас:
        // club drive-восьмые дают до 9 ударов, это лицо, а не баг.
        // Club держит тему плаком (сквозная мелодия битмейкера).
        type Floors<'a> = &'a [(&'a str, usize)];
        let contract: &[(&str, usize, Floors, usize)] = &[
            (
                "club",
                10,
                &[("kick", 48), ("snare", 24), ("hat", 48), ("bass", 48)],
                24,
            ),
            (
                "rock",
                6,
                &[
                    ("kick", 48),
                    ("snare", 24),
                    ("hat", 48),
                    ("bass", 48),
                    ("lead", 48),
                ],
                48,
            ),
            (
                "lofi",
                4,
                &[
                    ("kick", 12),
                    ("snare", 24),
                    ("hat", 16),
                    ("bass", 12),
                    ("chords", 48),
                    ("lead", 8),
                    ("texture", 24),
                ],
                8,
            ),
        ];
        for (name, kick_cap, role_floors, motif_floor) in contract {
            let style = make_style(name).expect("style");
            for seed in [11u64, 42, 777] {
                let mut mixer = crate::mixer::MixerState::new();
                crate::playback::apply_style_mix(&*style, &mut mixer, seed);
                let mut buf = [0usize; crate::core::MAX_ACTIVE_TRACKS];
                let n = mixer.active_ids(&mut buf);
                let active = buf[..n].to_vec();
                let mut rng = rand::SeedableRng::seed_from_u64(seed);
                let mut arranger = crate::arranger::Arranger::new(seed);
                let mut song = arranger.advance_measure();
                let mut kick_max = 0;
                let mut roles: HashMap<&str, usize> = HashMap::new();
                let mut motif_n = 0;
                let guides = style.slot_guides();
                for _bar in 0..24 {
                    let mut kicks = 0;
                    let mut sctx = StepContext {
                        step: 0,
                        step_in_measure: 0,
                        base_step_secs: style.base_step_secs(),
                        active_tracks: &active,
                        song: &song,
                        rng: &mut rng,
                        intensity: 1.0,
                    };
                    for sim in 0..16 {
                        sctx.step = sim;
                        sctx.step_in_measure = sim;
                        for note in render_style(&*style, &mut sctx) {
                            let slot = Slot::from_id(note.instrument_id).unwrap();
                            *roles.entry(slot.role().name()).or_insert(0) += 1;
                            if note.instrument_id == Slot::Kick.id() {
                                kicks += 1;
                            }
                            let gi =
                                Slot::ALL.iter().position(|s| *s == slot).unwrap();
                            if guides[gi].follow_motif {
                                motif_n += 1;
                            }
                        }
                    }
                    kick_max = kick_max.max(kicks);
                    song = arranger.advance_measure();
                }
                assert!(
                    kick_max <= *kick_cap,
                    "{name} seed {seed}: {kick_max} kicks/bar"
                );
                for (role, floor) in *role_floors {
                    let got = roles.get(role).copied().unwrap_or(0);
                    assert!(
                        got >= *floor,
                        "{name} seed {seed}: role {role} lost ({got} < {floor})"
                    );
                }
                assert!(
                    motif_n >= *motif_floor,
                    "{name} seed {seed}: theme gone ({motif_n})"
                );
            }
        }
    }

    #[test]
    fn tonic_shift_spans_five_semitones() {
        // Тоника: диапазон ±2, детерминизм, все 5 значений встречаются.
        for h in [0u64, 1, 42, 777, 424242, u64::MAX] {
            let s = tonic_shift_for(h);
            assert!((-2..=2).contains(&s), "shift out of range: {s}");
            assert_eq!(s, tonic_shift_for(h), "not deterministic");
        }
        let keys: std::collections::HashSet<i8> =
            (0..64u64).map(tonic_shift_for).collect();
        assert_eq!(keys.len(), 5, "tonics missing: {keys:?}");
    }

    #[test]
    fn tonic_shift_transposes_line_intact() {
        // Сдвиг транспонирует линию целиком: контур тот же, высота — нет.
        // Два хэша из одного %21-класса: ротация (%7) и модус (%3)
        // совпадают, отличается только сдвиг. Гейты разные — сравниваем
        // только шаги, звучащие в обеих линиях.
        let guide = RoleGuide {
            weights: &[1.0; 16],
            follow_motif: true,
            ..RoleGuide::pitched(&[1.0; 16], 0, 1, 0, 1.0, 0.8)
        };
        let scale = [48, 50, 52, 53, 55, 57, 59];
        let pitch = |hash: u64, step: usize| -> Option<u8> {
            let mut song = test_song(0.5);
            song.seed_hash = hash;
            let mut rng = rand::SeedableRng::seed_from_u64(3);
            play_guide(
                &guide, &scale, &song, 1.0, step, 16, 0.1, 6, 5, 1.0,
                false, false, 0, false, &mut rng,
            )
            .into_iter()
            .next()
            .map(|n| n.midi_pitch)
        };
        let mut pair = None;
        'search: for h1 in 0..200u64 {
            for h2 in (h1 + 1)..200u64 {
                if h1 % 21 != h2 % 21 {
                    continue;
                }
                if tonic_shift_for(h1) == tonic_shift_for(h2) {
                    continue;
                }
                pair = Some((h1, h2));
                break 'search;
            }
        }
        let (h1, h2) = pair.expect("no pair in range");
        let delta = tonic_shift_for(h2) as i16 - tonic_shift_for(h1) as i16;
        let mut checked = 0;
        for step in (0..16).step_by(2) {
            if let (Some(x), Some(y)) = (pitch(h1, step), pitch(h2, step)) {
                assert_eq!(y as i16 - x as i16, delta, "contour broken");
                checked += 1;
            }
        }
        assert!(checked >= 3, "too few common steps");
    }

    #[test]
    fn lofi_guitar_fingerpicks_arp() {
        // Фигнерпикинг: один интервал за удар, вкруговую по трезвучию
        // (sounding step = 1 нота). Бой-аккорды стеком давали щелкающую
        // кашу вместо струны.
        use crate::instruments::Slot;
        let style = lofi::LofiStyle::new();
        assert!((1.4..=1.8).contains(&style.swing_ratio()), "swing out of dusty band");
        let guides = style.slot_guides();
        let gi = Slot::ALL.iter().position(|s| *s == Slot::Guitar).unwrap();
        assert!(guides[gi].arp, "guitar must fingerpick");
        let iv = guides[gi].chord.expect("arp source");
        assert_eq!(iv.to_vec(), vec![0i8, 2, 4]);
        let active = vec![Slot::Guitar.id()];
        let mut rng = rand::SeedableRng::seed_from_u64(17);
        let mut heard: Vec<u8> = Vec::new();
        for phrase in 0..8usize {
            let mut song = test_song(0.5);
            song.phrase_index = phrase;
            let mut sctx = StepContext {
                step: 0,
                step_in_measure: 0,
                base_step_secs: style.base_step_secs(),
                active_tracks: &active,
                song: &song,
                rng: &mut rng,
                intensity: 1.0,
            };
            for sim in 0..16 {
                sctx.step = sim;
                sctx.step_in_measure = sim;
                let notes = render_style(&style, &mut sctx);
                // Один интервал за удар, не стек.
                assert!(notes.len() <= 1, "phrase {phrase} step {sim}: stack");
                heard.extend(notes.iter().map(|n| n.midi_pitch));
            }
        }
        // Перебор гуляет по струнам, а не топчется на одной ноте.
        let distinct: std::collections::HashSet<u8> = heard.iter().copied().collect();
        assert!(heard.len() >= 24, "strumming gone: {}/8 bars", heard.len());
        assert!(distinct.len() > 2, "arp stuck: {distinct:?}");
    }

    #[test]
    fn bass_line_stays_near_roots() {
        // Бас почти стоит: по первым долям прог-цикла разброс ≤ 3 полутонов.
        // Было: прогрессия таскала бас на ±ступени (то высоко, то низко).
        let guide = RoleGuide {
            weights: &[1.0; 8],
            ..RoleGuide::pitched(&[1.0; 8], 0, -1, 0, 1.0, 0.8)
        };
        let scale = [48, 51, 53, 55, 58];
        let mut pitches = Vec::new();
        for measure in 0..8usize {
            let mut song = test_song(0.5);
            song.measure_index = measure;
            song.prog = [0, 3, 1, 2];
            let mut rng = rand::SeedableRng::seed_from_u64(3);
            if let Some(n) = play_guide(
                &guide, &scale, &song, 1.0, 0, 8, 0.1, 1, 0, 1.0, false,
                false, 0, false, &mut rng,
            )
            .into_iter()
            .next()
            {
                pitches.push(n.midi_pitch);
            }
        }
        assert!(pitches.len() >= 6, "bass gone: {pitches:?}");
        let spread = pitches.iter().max().unwrap() - pitches.iter().min().unwrap();
        assert!(spread <= 3, "bass wanders: {pitches:?}");
    }

    #[test]
    fn emergent_pulse_phases_without_repeating() {
        // Пульс: детерминирован, варьируется сидом, темп ровный
        // (разгона в дропе нет — бит держится), такты не клонируются
        // (фэйзинг: ≥3 разных рисунка на 8 тактов).
        let ticks = |seed: u64| -> Vec<Vec<usize>> {
            (0..8)
                .map(|bar| {
                    (0..16)
                        .filter(|sim| {
                            let s = *sim;
                            emergent_fire(seed, 4, bar * 16 + s, s, 16, 0.45).0
                        })
                        .collect()
                })
                .collect()
        };
        let a = ticks(11);
        assert_eq!(a, ticks(11), "must repeat");
        assert!(a.iter().any(|b| !b.is_empty()), "pulse silent");
        let bars: std::collections::HashSet<Vec<usize>> = a.into_iter().collect();
        assert!(bars.len() >= 3, "bars cloned: {bars:?}");
        // Другой сид — другой грув.
        let b = ticks(42);
        assert_ne!(ticks(11).concat(), b.concat(), "seeds share groove");
    }

    #[test]
    fn role_plans_are_shared_by_performers() {
        // План роли — один срез на всех исполнителей (ptr-равенство,
        // не совпадение значений): lofi плак+гитара делят бой,
        // rock фазз+гитара делят рифф и его полюса.
        use crate::instruments::Slot;
        let lofi = make_style("lofi").expect("lofi");
        let guides = lofi.slot_guides();
        let at = |s: Slot| {
            Slot::ALL.iter().position(|x| *x == s).map(|i| &guides[i]).unwrap()
        };
        assert!(std::ptr::eq(
            at(Slot::Pluck).weights.as_ptr(),
            at(Slot::Guitar).weights.as_ptr()
        ));
        let rock = make_style("rock").expect("rock");
        let guides = rock.slot_guides();
        let at = |s: Slot| {
            Slot::ALL.iter().position(|x| *x == s).map(|i| &guides[i]).unwrap()
        };
        assert!(std::ptr::eq(
            at(Slot::Fuzz).weights.as_ptr(),
            at(Slot::Guitar).weights.as_ptr()
        ));
        assert!(std::ptr::eq(
            at(Slot::Fuzz).poles.unwrap().as_ptr(),
            at(Slot::Guitar).poles.unwrap().as_ptr()
        ));
    }

    #[test]
    fn seed_divergence_stays_high() {
        // Сиды обязаны различаться: питч-наборы пар (шаг%64, слот,
        // питч-класс) далеки (Жаккар ≥ 0.4), ритм-наборы не совпадают.
        // Замеры: pitch min 0.50–0.62, rhythm min 0.26–0.32.
        use std::collections::HashSet;
        type PitchSig = HashSet<(usize, usize, u8)>;
        type RhythmSig = HashSet<(usize, usize)>;
        fn signature(style_name: &str, seed: u64) -> (PitchSig, RhythmSig) {
            let style = make_style(style_name).expect("style");
            let mut mixer = crate::mixer::MixerState::new();
            crate::playback::apply_style_mix(&*style, &mut mixer, seed);
            let mut buf = [0usize; crate::core::MAX_ACTIVE_TRACKS];
            let n = mixer.active_ids(&mut buf);
            let active = buf[..n].to_vec();
            let mut rng = rand::SeedableRng::seed_from_u64(seed);
            let mut arranger = crate::arranger::Arranger::new(seed);
            let mut song = arranger.advance_measure();
            let mut pitches = HashSet::new();
            let mut rhythm = HashSet::new();
            for step in 0..256 {
                let spm = style.steps_per_measure();
                let sim = step % spm;
                if sim == 0 && step > 0 {
                    song = arranger.advance_measure();
                }
                let mut sctx = StepContext {
                    step,
                    step_in_measure: sim,
                    base_step_secs: style.base_step_secs(),
                    active_tracks: &active,
                    song: &song,
                    rng: &mut rng,
                    intensity: 1.0,
                };
                for note in render_style(&*style, &mut sctx) {
                    pitches.insert((note.instrument_id, step % 64, note.midi_pitch % 12));
                    rhythm.insert((note.instrument_id, step % 64));
                }
            }
            (pitches, rhythm)
        }
        for name in ["club", "lofi"] {
            let sigs: Vec<_> = (0..8u64).map(|s| signature(name, s)).collect();
            for i in 0..8 {
                for j in (i + 1)..8 {
                    let jac = |a: &PitchSig, b: &PitchSig| {
                        1.0 - a.intersection(b).count() as f32 / a.union(b).count().max(1) as f32
                    };
                    let rjac = |a: &RhythmSig, b: &RhythmSig| {
                        1.0 - a.intersection(b).count() as f32 / a.union(b).count().max(1) as f32
                    };
                    let pd = jac(&sigs[i].0, &sigs[j].0);
                    let rd = rjac(&sigs[i].1, &sigs[j].1);
                    assert!(pd >= 0.4, "{name}: seeds {i}/{j} share pitch ({pd:.2})");
                    assert!(rd > 0.0, "{name}: seeds {i}/{j} share rhythm");
                }
            }
        }
    }

    #[test]
    fn bass_ducks_kick_fills_not_downbeats() {
        // Бас знает роль кика: даунбит вместе в полную, под заливку —
        // вдвое тише (та же нота, тот же velocity-джиттер).
        let guide = RoleGuide {
            weights: &[1.0; 8],
            ..RoleGuide::pitched(&[1.0; 8], 0, -1, 0, 1.0, 0.8)
        };
        let scale = [48, 51, 53, 55, 58];
        let song = test_song(0.5);
        let call = |fill: bool| {
            let mut rng = rand::SeedableRng::seed_from_u64(3);
            play_guide(
                &guide, &scale, &song, 1.0, 0, 8, 0.1, 1, 0, 1.0, false,
                false, 0, fill, &mut rng,
            )
            .into_iter()
            .next()
            .map(|n| n.velocity)
            .unwrap()
        };
        let full = call(false);
        assert_eq!(call(true), full * 0.5, "bass ignores kick fill");
    }

    #[test]
    fn anchor_detection_mirrors_weights() {
        // Детектор якорей повторяет формулу весов: якорь, склон, тишина.
        let song = test_song(0.5);
        let guide = RoleGuide {
            weights: &[1.0, 0.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            ..RoleGuide::kit(&[1.0, 0.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0], 36, 0.9)
        };
        assert!(is_anchor_step(&guide, &song, 0, 0));
        assert!(!is_anchor_step(&guide, &song, 0, 1));
        assert!(!is_anchor_step(&guide, &song, 0, 2));
    }

    #[test]
    fn swing_varies_by_seed_within_band() {
        // Свинг сида: 0.95..1.05, детерминирован, реально варьируется
        // (min ниже 0.98, max выше 1.02) — у сидов разный кач.
        // Club свингован (deep house 54–58%), а не прямой.
        for h in [0u64, 1, 42, 777, 424242] {
            let v = swing_var(h);
            assert!((0.95..=1.05).contains(&v), "swing out of band: {v}");
            assert_eq!(v, swing_var(h), "not deterministic");
        }
        let vals: Vec<f32> = (0..64u64).map(swing_var).collect();
        assert!(
            vals.iter().cloned().fold(1.0f32, f32::min) < 0.98
                && vals.iter().cloned().fold(0.0f32, f32::max) > 1.02,
            "swing never varies"
        );
    }

    #[test]
    fn club_swings_for_deep_house() {
        // Deep house кач: свинг club в 1.15..1.25 (54–58%), не прямой.
        let swing = make_style("club").expect("club").swing_ratio();
        assert!((1.15..=1.25).contains(&swing), "club too straight: {swing}");
    }

    #[test]
    fn kick_vocabularies_differ_by_seed() {
        // Словарь битов: concatenated kick-рисунки 8 тактов различаются
        // у разных сидов (у каждого сида свой бит, а не один на стиль).
        use crate::instruments::Slot;
        for name in style_names() {
            let style = make_style(name).expect("style");
            let mut pats = std::collections::HashSet::new();
            for seed in 0..12u64 {
                let mut mixer = crate::mixer::MixerState::new();
                crate::playback::apply_style_mix(&*style, &mut mixer, seed);
                let mut buf = [0usize; crate::core::MAX_ACTIVE_TRACKS];
                let n = mixer.active_ids(&mut buf);
                let active = buf[..n].to_vec();
                let mut rng = rand::SeedableRng::seed_from_u64(seed);
                let mut arranger = crate::arranger::Arranger::new(seed);
                let mut song = arranger.advance_measure();
                let mut pat = Vec::new();
                for _bar in 0..8 {
                    let mut sctx = StepContext {
                        step: 0,
                        step_in_measure: 0,
                        base_step_secs: style.base_step_secs(),
                        active_tracks: &active,
                        song: &song,
                        rng: &mut rng,
                        intensity: 1.0,
                    };
                    for sim in 0..16 {
                        sctx.step = sim;
                        sctx.step_in_measure = sim;
                        if render_style(&*style, &mut sctx)
                            .into_iter()
                            .any(|nn| nn.instrument_id == Slot::Kick.id())
                        {
                            pat.push(sim);
                        }
                    }
                    song = arranger.advance_measure();
                }
                pats.insert(pat);
            }
            assert!(pats.len() >= 6, "{name}: one beat per style ({})", pats.len());
        }
    }

    #[test]
    fn kick_leaves_breath_before_downbeat() {
        // Фламы на стыке такта запрещены: 15-й шаг киковых направляющих
        // тише 0.3 во всех стилях (связка 14-15-0 ломала бит очередью).
        // Проверяются веса и все полюса кик-слотов.
        use crate::instruments::Slot;
        for name in style_names() {
            let style = make_style(name).expect("known style");
            let guides = style.slot_guides();
            for slot in [Slot::Kick, Slot::Kick808] {
                let gi = Slot::ALL.iter().position(|s| *s == slot).unwrap();
                let g = &guides[gi];
                let mut arrs: Vec<&[f32]> = vec![g.weights];
                if let Some(poles) = g.poles {
                    arrs.extend(poles.iter().copied());
                }
                for (ai, arr) in arrs.iter().enumerate() {
                    if arr.len() == style.steps_per_measure() {
                        assert!(
                            arr[15] < 0.3,
                            "{name} {slot:?} arr {ai}: pickup {v} before downbeat",
                            v = arr[15]
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn duration_shape_stretches_notes_by_seed() {
        // Длительность от сида 0.85..1.25: детерминирована, варьируется,
        // цепляется к ноте (доля базового шага × mult гайда × shape).
        for h in [0u64, 1, 42, 777] {
            let d = duration_shape(h, 3);
            assert!((0.85..=1.25).contains(&d), "out of range: {d}");
            assert_eq!(d, duration_shape(h, 3), "not deterministic");
        }
        let set: std::collections::HashSet<u32> = (0..32u64)
            .map(|h| (duration_shape(h, 3) * 1000.0) as u32)
            .collect();
        assert!(set.len() > 8, "seeds share lengths");
        // Прилипание: длительность ноты = база × mult × shape.
        let guide = RoleGuide {
            weights: &[1.0; 8],
            ..RoleGuide::pitched(&[1.0; 8], 0, 0, 0, 2.0, 0.8)
        };
        let mut song = test_song(0.5);
        song.seed_hash = 99;
        let mut rng = rand::SeedableRng::seed_from_u64(3);
        let notes = play_guide(
            &guide, &[48, 50, 52, 53, 55, 57, 59], &song, 1.0, 0, 8, 0.5, 3, 2,
            1.0, false, false, 0, false, &mut rng,
        );
        assert!(!notes.is_empty());
        let expected = 0.5 * 2.0 * duration_shape(99, 2);
        assert!(
            (notes[0].duration_secs - expected).abs() < 1e-4,
            "shape ignored: {} vs {expected}",
            notes[0].duration_secs
        );
    }

    #[test]
    fn progression_moves_accompaniment_not_motif() {
        // Аккомпанемент следует прогрессии (разные такты — разные ступени),
        // мотив при той же прогрессии стабилен.
        fn degree_at(measure: usize, follow_motif: bool) -> u8 {
            let guide = RoleGuide {
                weights: &[1.0; 8],
                follow_motif,
                ..RoleGuide::pitched(&[1.0; 8], 0, 1, 0, 1.0, 0.8)
            };
            let scale = [48, 50, 52, 53, 55, 57, 59];
            let mut song = test_song(0.5);
            song.measure_index = measure;
            song.prog = [0, 2, 0, 0];
            let mut rng = rand::SeedableRng::seed_from_u64(3);
            play_guide(&guide, &scale, &song, 1.0, 0, 8, 0.1, 7, 6, 1.0, false, false, 0, false, &mut rng)
                .into_iter()
                .next()
                .map(|n| n.midi_pitch)
                .unwrap()
        }
        // Такты 0–1 (прог 0) и 2–3 (прог 2): аккомпанемент сдвинут.
        assert_ne!(degree_at(1, false), degree_at(2, false));
        // Мотив от такта не зависит.
        assert_eq!(degree_at(1, true), degree_at(2, true));
    }

    fn shared_hits(share: f32) -> usize {
        // Не-якорный лид (вес 0.5): считаем срабатывания по сетке
        // 16 шагов × 8 фраз. Доля 1.0 = единственный носитель роли.
        let guide = RoleGuide {
            weights: &[0.5; 16],
            ..RoleGuide::kit(&[0.5; 16], 60, 0.8)
        };
        let mut total = 0;
        for phrase in 0..8usize {
            let mut song = test_song(0.5);
            song.phrase_index = phrase;
            let mut rng = rand::SeedableRng::seed_from_u64(11);
            total += (0..16)
                .filter(|step| {
                    !play_guide(
                        &guide, &[48], &song, 1.0, *step, 16, 0.1, 6, 5, share,
                        false, false, 0, false, &mut rng,
                    )
                    .is_empty()
                })
                .count();
        }
        total
    }

    #[test]
    fn role_sharing_thins_crowded_roles_but_spares_anchors() {
        // Два носителя роли (share 1/sqrt(2)) — каждый реже одиночки,
        // вместе покрывают пространство без каши. Детерминировано.
        let alone = shared_hits(1.0);
        let shared = shared_hits(1.0 / 2.0f32.sqrt());
        assert!(alone > 0, "lead never plays");
        assert!(
            shared < alone,
            "sharing must thin: shared={shared} alone={alone}"
        );
        assert_eq!(shared, shared_hits(1.0 / 2.0f32.sqrt()), "must repeat");
        // Якоря (вес 1.0) дележу не подлежат, но дышат приоритетом 0.95:
        // рисунок ударов не зависит от доли, пропуски редки и те же
        // при любой доле (гейт не знает о дележе).
        let anchor = RoleGuide {
            weights: &[1.0; 8],
            ..RoleGuide::kit(&[1.0; 8], 36, 0.9)
        };
        let pattern = |share: f32| -> Vec<bool> {
            let mut song = test_song(0.5);
            let mut out = Vec::new();
            for phrase in 0..16usize {
                song.phrase_index = phrase;
                let mut rng = rand::SeedableRng::seed_from_u64(4);
                for step in 0..8 {
                    out.push(
                        !play_guide(
                            &anchor, &[48], &song, 1.0, step, 8, 0.1, 2, 0,
                            share, false, false, 0, false, &mut rng,
                        )
                        .is_empty(),
                    );
                }
            }
            out
        };
        let full = pattern(1.0);
        let hits = full.iter().filter(|h| **h).count();
        // Приоритет 0.95: бьет почти всегда (потери в пределах ~1/10).
        assert!(
            hits * 10 >= full.len() * 9,
            "anchors lost face: {hits}/{}",
            full.len()
        );
        // Дележ на якоря не действует: рисунок тот же при любой доле.
        assert_eq!(full, pattern(1.0 / 2.0f32.sqrt()), "share moved anchors");
        assert_eq!(full, pattern(0.2), "share moved anchors");
    }
}
