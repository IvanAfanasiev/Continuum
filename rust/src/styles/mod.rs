//! Стили как данные-направления, а не партитуры.
//!
//! Каждый стиль объявляет: лад, темп, свинг, дефолтный микс по слотам
//! и по одной направляющей [`RoleGuide`] на слот. Как именно из весов
//! родятся ноты, решает общий движок [`render_style`]: вес 1.0 —
//! структурный якорь жанра (почти всегда играет, приоритет 0.95;
//! кик-якорь — всегда), веса между — вероятности,
//! масштабируемые энергией секции. Никаких номеров шагов в коде стилей.
//!
//! Принцип правил: их умеренное количество, а не минимум. Фиксированная
//! структура (форма циклом, якоря жанра, регистровые капы) — разрешена
//! и желательна: она держит музыку музыкой. Свобода сидов — в содержании:
//! мотив, грув, гости, тембровые выборы. Правила задают русло,
//! сиды — воду. Бесконечность от этого не страдает: циклы длинные,
//! короткие слышимые повторы запрещены контрактами.
//!
//! Худые миксы: дефолтный ансамбль — 5–9 слотов, каждый с уникальной
//! работой. Дубль текстуры/второго лида «на всякий случай» — в ручные
//! слоты, а не в дефолт: плотность доказывается замером, не ощущением.

pub mod club;
pub mod lofi;
pub mod phonk;
pub mod protoclub;
pub mod rock;

use rand::rngs::StdRng;
use rand::Rng;

use crate::arranger::{SongContext, SongSection};
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
    /// Молчать в последней четверти последнего такта BuildUp:
    /// короткий вдох ровно перед дропом, а не сбивка в конце каждого
    /// раздела формы.
    pub drop_before_climax: bool,
    /// Играть каждый N-й такт от старта сессии (пэды через такт и т.п.).
    /// Сквозной такт, а не локальный в секции: фаза не сбрасывается
    /// на стыках формы. 1 = всегда.
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
    /// Отдых фразами 0..1: доля фраз, которые носитель темы молчит
    /// целиком (вопрос-ответ фразами: один поет — второй дышит).
    /// Бросок независимый на слот. 0.0 = всегда играет.
    pub phrase_rest: f32,
    /// Ролл в дроп: последний такт BuildUp слот играет каждую 16-ю
    /// с нарастанием (снейр-ролл в дроп — лицо жанра). Веса/гейты
    /// на этом такте игнорируются: заливка всегда доводит.
    /// Только энергичным (club/rock/phonk/proto), не lofi.
    pub build_roll: bool,
    /// Забивка на стыке фразы: в последней доле фразы слот играет
    /// каждую 16-ю с нарастанием. Сгенерированный переход (не кроссфейд):
    /// стык слышно как ход в следующую фразу, а не как обрыв.
    /// Ставится стилем: в спокойном лофай забивки нет.
    pub phrase_fill: bool,
    /// Полюса по секциям: грув слота пересдаётся на каждую секцию
    /// (Main свой бит, Drop свой), а не стоит один на весь трек.
    /// Якоря формы: дроп каждый раз узнаваем, куплет — свой.
    /// Выбор детерминирован (сид, слот, секция). Без полюсов флаг
    /// ничего не делает.
    pub section_poles: bool,
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
        chord: None,
        bass_fifth: false,
        sub_bias: 0.0,
        bass_top: 45,
        arp: false,
        phrase_rest: 0.0,
        build_roll: false,
        phrase_fill: false,
        section_poles: false,
    };

    pub fn is_silent(&self) -> bool {
        // Молчит только если не звучит ни веса такта, ни единый полюс:
        // полюса — это грув, а не декорация, и пустые веса с полюсами
        // обязаны играть (иначе роль пропадает из ансамбля молча).
        let weights_silent = self.weights.iter().all(|&w| w <= 0.0);
        let poles_silent = self
            .poles
            .is_none_or(|poles| poles.iter().all(|p| p.iter().all(|&w| w <= 0.0)));
        weights_silent && poles_silent
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
            chord: None,
            bass_fifth: false,
            sub_bias: 0.0,
            bass_top: 45,
            arp: false,
            phrase_rest: 0.0,
            build_roll: false,
            phrase_fill: false,
            section_poles: false,
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
            chord: None,
            bass_fifth: false,
            sub_bias: 0.0,
            bass_top: 45,
            arp: false,
            phrase_rest: 0.0,
            build_roll: false,
            phrase_fill: false,
            section_poles: false,
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
    /// Как стиль склеивает куски: плавный внахлёст (голос следующего
    /// входит поверх грува) или резкий стык на границе такта. Энергичные
    /// стили катят грув через стык, спокойные — закрывают тихо.
    fn transition(&self) -> crate::piece::Transition {
        crate::piece::Transition::Rolling { bars: 2 }
    }
    /// Вайтлист инструментов стиля: кто включен при выборе стиля,
    /// остальные глушатся в ноль. Громкости — канонические, у слотов
    /// ([`Slot::base_volume`]); пресет не хардкодит цифры.
    /// Состав меняет слушатель в миксе; сид на него не влияет.
    fn ensemble(&self) -> &'static [Slot];
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
    // Сколько реально звучащих носителей у каждой роли — карта дележа
    // плотности. Молчащие гайды (нулевые веса и полюса) не считаются:
    // иначе живые исполнители делят плотность с пустотой.
    let mut role_n = [0usize; 8];
    for (i, s) in Slot::ALL.iter().enumerate() {
        if sctx.active_tracks.contains(&s.id()) && !guides[i].is_silent() {
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
    // Носители темы на этом шаге: активные followers + подхват.
    // Лидер ведет фразу в полную, остальные — эхом (см. play_guide):
    // впервые у темы есть первый голос, а не хор солистов.
    let mut carriers: Vec<usize> = Slot::ALL
        .iter()
        .enumerate()
        .filter(|(i, s)| is_active(**s) && guides[*i].follow_motif && !guides[*i].is_silent())
        .map(|(_, s)| s.id())
        .collect();
    if carriers.is_empty() {
        if let Some(fb) = fallback {
            carriers.push(fb);
        }
    }
    let leader = theme_leader(&carriers, sctx.song.seed_hash, sctx.song.stage);
    // Единый пул мелодических голосов для громкостного баланса:
    // активные и реально звучащие (не SILENT).
    let active_melodic: Vec<usize> = Slot::ALL
        .iter()
        .enumerate()
        .filter(|(i, s)| is_active(**s) && !guides[*i].is_silent() && is_melodic_role(s.role()))
        .map(|(_, s)| s.id())
        .collect();
    // Фаза 1: кики. Фундамент — первым, чтобы DSP-дак бочки был готов
    // к моменту нот остальных слоев (порядок важен движку, не генерации).
    for (i, slot) in Slot::ALL.iter().enumerate() {
        if slot.role() != InstrumentRole::Kick {
            continue;
        }
        if !sctx.active_tracks.contains(&slot.id()) {
            continue;
        }
        // Аранж по слоям: план куска пишет, кто играет в этой секции.
        // Молчащая группа — не «тише», а молчит: точки на колесе и звук
        // совпадают, потому что решение одно и то же.
        if sctx.song.layers & (1 << slot.group().index()) == 0 {
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
            // Кики тему не несут: лидерство им не нужно.
            false,
            sctx.rng,
        );
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
        // Аранж по слоям: план куска пишет, кто играет в этой секции.
        if sctx.song.layers & (1 << slot.group().index()) == 0 {
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
            && matches!(slot.role(), InstrumentRole::Lead | InstrumentRole::Vocal)
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
            // Лидер фразы — в полную, остальные носители — эхом.
            leader == Some(slot.id()),
            sctx.rng,
        ));
    }
    // Единый баланс партий: лидер темы (а если тему несет не мелодический
    // голос — выбранный мелодический) чуть громче, остальные мелодические
    // — поддержка. Второго, независимого лидера в композере больше нет:
    // иерархия одна и та же для эха темы и для громкостного баланса.
    apply_leader_balance(
        &mut notes,
        balance_leader(&active_melodic, leader, sctx.song.stage),
    );
    notes
}

/// Мелодические роли — те, что звучат в верхнем регистре: лид, вокал,
/// аккорды. Бас и барабаны держат фундамент и в балансе не участвуют.
fn is_melodic_role(role: InstrumentRole) -> bool {
    matches!(
        role,
        InstrumentRole::Lead | InstrumentRole::Chords | InstrumentRole::Vocal
    )
}

/// Лидер баланса: ведущий темы, если он мелодический; иначе —
/// детерминированная ротация по активным мелодическим голосам.
/// Меньше двух голосов — балансировать нечего (одного не давим).
fn balance_leader(
    active_melodic: &[usize],
    theme_lead: Option<usize>,
    stage: usize,
) -> Option<usize> {
    if active_melodic.len() < 2 {
        return None;
    }
    theme_lead
        .filter(|l| active_melodic.contains(l))
        .or_else(|| Some(active_melodic[stage % active_melodic.len()]))
}

/// Намеренно сдержанный баланс: генерация не спорит с ручками
/// пользователя, лишь слегка подсвечивает ведущий голос.
const LEADER_BOOST: f32 = 1.08;
const SUPPORT_DIP: f32 = 0.95;

fn apply_leader_balance(notes: &mut [NoteEvent], leader: Option<usize>) {
    let Some(lead_id) = leader else { return };
    for note in notes.iter_mut() {
        let Some(role) = Slot::from_id(note.instrument_id).map(|s| s.role()) else {
            continue;
        };
        if !is_melodic_role(role) {
            continue;
        }
        if note.instrument_id == lead_id {
            note.velocity = (note.velocity * LEADER_BOOST).min(1.0);
        } else {
            note.velocity *= SUPPORT_DIP;
        }
    }
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

/// Позиция доли внутри музыкальной фразы (фраза = два такта).
fn phrase_position(song: &SongContext, step_in_measure: usize, steps_per_measure: usize) -> usize {
    let bar_in_phrase = song.measure_index.saturating_sub(1) % 2;
    bar_in_phrase * steps_per_measure + step_in_measure
}

/// Зона забивки: последняя доля фразы (четверть такта) — здесь переход
/// из фразы во фразу.
fn phrase_fill_zone(steps_per_measure: usize, song: &SongContext, step_in_measure: usize) -> bool {
    let zone_start = 2 * steps_per_measure - steps_per_measure / 4;
    phrase_position(song, step_in_measure, steps_per_measure) >= zone_start
}

/// Приоритет якоря: даже даунбит иногда дышит (1 из 20 ударов
/// пропускается гейтом сида). Лицо жанра держится статистически,
/// абсолюта «звучит всегда» больше нет нигде.
const ANCHOR_PRIOR: f32 = 0.95;
/// Соль якорного гейта: независимый бросок от остальных гейтов слота.
const ANCHOR_SALT: u64 = 0xA11CE;

/// Вес шага с учётом полюса трека: бит прописан выбором полюса,
/// без него — весами такта. Флаг `section_poles` подмешивает секцию
/// в соль выбора: грув пересдаётся на секцию (у дропа свой бит).
fn pole_step_weight(
    guide: &RoleGuide,
    song: &SongContext,
    key: usize,
    step_in_measure: usize,
) -> f32 {
    let base_w = guide.weights.get(step_in_measure).copied().unwrap_or(0.0);
    if let Some(poles) = guide.poles.filter(|p| !p.is_empty()) {
        let mut salt = song.seed_hash ^ (key as u64).wrapping_mul(0x9E3779B9);
        if guide.section_poles {
            salt ^= (song.section as u64).wrapping_mul(0x5EED);
        }
        let pick = crate::arranger::splitmix64(salt) % poles.len() as u64;
        poles[pick as usize]
            .get(step_in_measure)
            .copied()
            .unwrap_or(0.0)
    } else {
        base_w
    }
}

/// Затухание тонов аккорда: корень в полную силу, верх тише.
const CHORD_ATTEN: [f32; 4] = [1.0, 0.75, 0.65, 0.6];

/// Prior участия акцентных слотов в визитах секций (остальные — всегда).
/// Вокал — эффект: его отсутствие в части стыков и есть разнообразие
/// сидов, а не поломка.
fn accent_prior(instrument_id: usize) -> f32 {
    match Slot::from_id(instrument_id) {
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

/// Октава темы от сида (−1/0/+1 на весь трек): один сид поет
/// высоко, другой — тенором. Едет со всей темой (мотив и подхват
/// вместе), а не с отдельным слотом. Детерминирована полностью.
pub(crate) fn motif_octave(seed_hash: u64) -> i8 {
    (crate::arranger::splitmix64(seed_hash ^ 0x0C7A) % 3) as i8 - 1
}

/// Лидер темы на визит секции: один носитель ведет в полную,
/// остальные вторят эхом. Ротация детерминирована (сид, визит).
/// Заморожено на секцию: первый голос держится весь раздел формы
/// и меняется только на границе (состав не тасуется внутри песни).
/// Пусто — вести некому (все молчат как написано).
pub(crate) fn theme_leader(carriers: &[usize], seed_hash: u64, stage: usize) -> Option<usize> {
    if carriers.is_empty() {
        return None;
    }
    let h =
        crate::arranger::splitmix64(seed_hash ^ 0x1EAD ^ (stage as u64).wrapping_mul(0x9E3779B9));
    Some(carriers[(h % carriers.len() as u64) as usize])
}

/// Доля эха: столько своих шагов вторящий оставляет себе.
/// Общий бросок на всех вторящих — эхо звучит вместе.
/// Высоко (0.65): вторящий почти всегда рядом с лидером —
/// параллельная игра линий, а не разовые удары по очереди.
pub(crate) const LEAD_ECHO: f32 = 0.65;

/// Свинг сида: множитель 0.93..1.07 к свингу стиля — у каждого сида
/// свой кач (слышно!), грув не разваливается (±10% давали пьяные
/// шестнадцатые). Детерминирован.
pub(crate) fn swing_var(seed_hash: u64) -> f32 {
    let h = crate::arranger::splitmix64(seed_hash ^ 0x5716);
    0.93 + 0.14 * ((h >> 32) as f32 / (u32::MAX as f32))
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
    let h =
        crate::arranger::splitmix64(seed_hash ^ (slot_idx as u64).wrapping_mul(0xC0FFEE) ^ 0xD0A4);
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
    // Лидер темы на этой фразе: ведет в полную. Остальные носители —
    // эхом (урезанная плотность, те же шаги). Тесты зовут с true:
    // голос в изоляции звучит как написано.
    theme_lead: bool,
    rng: &mut StdRng,
) -> Vec<NoteEvent> {
    // Вес шага: полюс трека или веса такта. Без морфинга и заливок:
    // плотность секции задают только энергия и структура.
    // Роль слота нужна рано (общий план и кик-якоря): чистая функция id.
    let role = Slot::from_id(instrument_id).map(|s| s.role());
    // Общий план (общая фраза): все носители темы кидают один гейт —
    // разреженные играют подмножество густых (те же шаги, не свои).
    // Каждый слой уходит в свою краску (голос, октава, велосити),
    // но бьет вместе: гетерофония вместо хора солистов.
    // Ударные, бас и аккомпанемент кидают свой: у каждой работы
    // свой рисунок, иначе грув слипнется в унисон.
    let groove_key = if guide.follow_motif || force_motif {
        10_077
    } else {
        slot_idx
    };
    // Структурные фильтры действуют и на ролл: слот, который по плану
    // молчит этот такт (every_n) или делает вдох перед дропом, не
    // получает заливку. Иначе редкая партия превращалась в «всегда
    // в последнем такте билда» вопреки собственной структуре.
    if guide.every_n_measures > 1
        && !(song.measure_index.saturating_sub(1)).is_multiple_of(guide.every_n_measures)
    {
        return Vec::new();
    }
    let climax = song.is_last_measure;
    // Пауза перед кульминацией — только перед дропом, в последней
    // четверти последнего такта BuildUp. Раньше флаг срабатывал на
    // последнем такте Drop и вырезал четверть уже после удара — это
    // читалось как сбивка, а не как вдох перед дропом.
    let before_drop = matches!(song.section, SongSection::BuildUp);
    if guide.drop_before_climax
        && climax
        && before_drop
        && step_in_measure * 4 >= steps_per_measure * 3
    {
        return Vec::new();
    }

    // Отдых фразами: носитель темы иногда молчит фразу целиком.
    // Вопрос-ответ фразами вместо хора солистов: лидер фразы
    // не отдыхает никогда (иначе тема перебивается паузой),
    // остальные дышат по независимому броску. Подхват (ключи
    // без темы) не отдыхает: одинокий носитель всегда ведет.
    // Проверка стоит до ролла: отдых фразы — общее правило формы,
    // а не строка грува, которую заливка вправе переписать.
    if guide.follow_motif
        && !theme_lead
        && guide.phrase_rest > 0.0
        && fixed_gate(song.seed_hash, slot_idx, song.phrase_index, 0x2E57) < guide.phrase_rest
    {
        return Vec::new();
    }

    // Ролл в дроп: последний такт BuildUp ударный слот с флагом
    // играет каждую 16-ю с нарастанием 0.55→1.0. Это структурная
    // заливка, а не строка грува: веса шага, полюса и вероятностный
    // гейт осознанно не применяются. Общие правила формы (every_n,
    // вдох перед дропом, отдых фразы) выше уже отработали.
    // Ролл доводит до последней 16-й такта: заливка вливается в
    // даунбит дропа, а не обрывается за шаг до него.
    // Только Kit-голоса (снейр): питчевым флаг не ставится.
    //
    // Переход на стыке фраз — тот же прием: сгенерированная забивка в
    // последней доле фразы. Не кроссфейд и не тишина: это ноты, которые
    // ведут в следующую фразу, поэтому стык слышно как ход, а не как
    // обрыв. Флаг ставит стиль: в спокойном лофай забивки нет.
    let drop_roll =
        guide.build_roll && matches!(song.section, SongSection::BuildUp) && song.is_last_measure;
    let fill = phrase_fill_zone(steps_per_measure, song, step_in_measure);
    // Забивка не у всех сидов одинаковая: у одного плотнее, у другого
    // реже. Иначе все сиды получили бы один и тот же хвост — и
    // различие сидов по вокалу/ритму просело бы.
    let fill_skip = guide.phrase_fill
        && fill
        && fixed_gate(song.seed_hash, slot_idx, step_in_measure, 0xF11E) >= 0.28;
    if drop_roll || (guide.phrase_fill && fill && !fill_skip) {
        let Voicing::Kit(pitch) = guide.voice else {
            return Vec::new();
        };
        let rise = if guide.phrase_fill && fill {
            // Нарастание внутри забивки: от первой 16-й к последней.
            let zone_start = 2 * steps_per_measure - steps_per_measure / 4;
            let done =
                (phrase_position(song, step_in_measure, steps_per_measure) - zone_start) as f32;
            0.55 + 0.45 * done / (steps_per_measure / 4).max(1) as f32
        } else {
            0.55 + 0.45 * step_in_measure as f32 / steps_per_measure.max(1) as f32
        };
        let mut velocity = (guide.vel_base * rise
            + energy * guide.vel_energy
            + rng.gen_range(-guide.vel_jitter..=guide.vel_jitter))
        .clamp(0.1, 1.0);
        if guide.vel_high_cut < 1.0 {
            let octaves_above = ((pitch as f32 - 40.0) / 12.0).max(0.0);
            velocity = (velocity * guide.vel_high_cut.powf(octaves_above)).clamp(0.05, 1.0);
        }
        if let Some(r) = role {
            velocity *= song.stage_gain(r);
        }
        velocity *= beat_accent(step_in_measure, steps_per_measure);
        if velocity < 0.05 {
            return Vec::new();
        }
        return vec![NoteEvent::new(
            instrument_id,
            pitch,
            velocity,
            base_step_secs * 0.5,
        )];
    }
    // Полюс грува: бит прописан выбором полюса (сид, план; при
    // section_poles — ещё и секция: у дропа свой бит) и держится.
    // Оживляют его только гейты фраз и свинг сида. Тот же сид —
    // тот же бит; другой сид — другой.
    let weight = pole_step_weight(guide, song, groove_key, step_in_measure);
    if weight <= 0.0 {
        return Vec::new();
    }
    // Точечные акценты — аранжировка, а не мигание: присутствуют
    // целыми секциями (гейт от визита), а не фразами.
    // Сквозной stage без `% 64`: короткий период давал дословные
    // повторы эффектов каждые 64 визита.
    let prior = accent_prior(instrument_id);
    if prior < 1.0 && fixed_gate(song.seed_hash, groove_key, song.stage, 0xA66E47) >= prior {
        return Vec::new();
    }

    // Сеточный путь: вероятности + гейт сида.
    // Кик-якоря — абсолюти: фундамент бьет всегда, без дыхания.
    // Остальные якоря дышат приоритетом 0.95.
    let kick_anchor = weight >= 1.0 && matches!(role, Some(InstrumentRole::Kick));
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
        let p = weight * energy * energy * guide.energy_response * song.density * role_share;
        p.clamp(0.0, 1.0)
    };
    // Гейт сида: решение зависит от (сид, план, такт, визит секции).
    //
    // Раньше соль брала фразу: рисунок ударных пересдавался каждые две
    // доли, и на стыке фразы удары то появлялись, то исчезали — на
    // слух это «скачок» (замер потока: у лофай-кика 831 против 32
    // внутри фразы). Теперь соль держит ВИЗИТ секции: внутри раздела
    // грув один, меняется он на границе раздела — как у живой группы.
    // Якоря (weight >= 1.0) держат общий скелет: их соль не включает
    // такт, и дыхание даунбита (5% пропусков) не должно «моргать» на
    // каждой фразе.
    // Носители темы делят бросок: разреженный входит в шаги густого.
    // Якоря идут отдельным броском (своя соль) — пропуск якоря
    // не влечет пропуски остальных шагов и наоборот.
    // Кик-якорь гейт не проходит вовсе.
    let bar_in_phrase = song.measure_index.saturating_sub(1) % 2;
    let salt = if weight >= 1.0 {
        // Якоря (даунбит) дышат по фразе: 5% пропусков — живой пульс.
        ANCHOR_SALT ^ (song.phrase_index as u64)
    } else {
        // Прочие ноты (синкопы, штрихи) — своя соль на фразу: рисунок
        // развивается, а не дублирует предыдущую фразу дословно.
        (song.phrase_index as u64)
            .wrapping_mul(2)
            .wrapping_add(bar_in_phrase as u64)
    };
    if !kick_anchor
        && probability < 1.0
        && fixed_gate(song.seed_hash, groove_key, step_in_measure, salt) >= probability
    {
        return Vec::new();
    }
    // Иерархия темы: ведет один (лидер секции), остальные — эхом.
    // Общий бросок на всех вторящих: эхо звучит вместе, в те же шаги.
    // Соль — визит секции, в ногу с лидерством: состав эха стоит,
    // пока стоит лидер.
    // Вложенность базовых гейтов цела: разреженный и так подмножество.
    if (guide.follow_motif || force_motif)
        && !theme_lead
        && fixed_gate(song.seed_hash, 7777, step_in_measure, song.stage as u64) >= LEAD_ECHO
    {
        return Vec::new();
    }

    let mut velocity = (guide.vel_base
        + energy * guide.vel_energy
        + rng.gen_range(-guide.vel_jitter..=guide.vel_jitter))
    .clamp(0.1, 1.0);

    // Артикуляция сида для мягких голосов: та же нота тем же голосом —
    // с разной мягкостью у разных сидов. Ударные идут как есть.
    let (attack_mul, release_mul) = envelope_shape(song.seed_hash, slot_idx);

    // Оффбитные 8-е: шаг q/2 внутри четверти (q = шагов на четверть).
    // Длительность — от сида: одни сиды протяжнее, другие отрывистее.
    let quarter = steps_per_measure / 4;
    let long = guide.long_offbeats && quarter > 0 && step_in_measure % quarter == quarter / 2;
    // Фразировка темы: сильные доли поют (длинные ноты), слабые —
    // коротко. Общая для всех носителей (чистая функция шага):
    // унисон держит и ритм. Мелодия дышит длинно-коротко, а не стучит
    // 8-ми одинаковой длины (замер: 1–2 длительности на стиль).
    let theme_phrase = if guide.follow_motif || force_motif {
        if step_in_measure == 0 {
            3.0
        } else if step_in_measure == steps_per_measure / 2 {
            2.6
        } else {
            match step_in_measure % 4 {
                0 => 2.0,
                2 => 0.9,
                _ => 1.3,
            }
        }
    } else {
        1.0
    };
    let duration = base_step_secs
        * guide.duration_mult
        * duration_shape(song.seed_hash, slot_idx)
        * theme_phrase
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
        // Иерархия долей и в ударных: даунбит — полный голос (×1.25),
        // третья — чуть тише, слабые — как написано. Бит качает
        // динамикой, а не ровной стеной одинаковых ударов.
        velocity *= beat_accent(step_in_measure, steps_per_measure);
        if velocity < 0.05 {
            return Vec::new();
        }
        return vec![NoteEvent::new(instrument_id, pitch, velocity, duration)];
    }

    // `line_fold` — на сколько октав линия темы уходит вниз целиком,
    // чтобы потолок регистра не резал отдельные ноты (иначе линия
    // рвётся скачком на октаву посреди фразы).
    let (tone_base, degree, octaves, line_fold) = match guide.voice {
        Voicing::Kit(_) => unreachable!("kit handled above"),
        Voicing::Scale {
            degree: voice_degree,
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
                // Аккомпанемент идет за прогрессией: бас — корнями
                // вместе со всеми (дрона на одной ноте больше нет),
                // остальные — в полную. Гармонический ритм — от сида:
                // одни сиды меняют аккорд каждые 2 такта (движение),
                // другие держат по 8 (фундамент). Почерк сида, а не
                // один темп гармонии на всех.
                // Регистр баса держит фолд, блуждание — джиттер гайда.
                // Прогрессия идет по сквозному такту сессии: сброс на
                // каждом стыке секций превращал движение в топтание
                // на первом аккорде в любой короткой секции.
                song.prog[(song.measure_index.saturating_sub(1) / prog_bar_len(song.seed_hash))
                    % song.prog.len()] as usize
            };
            let (degree, jitter, contour_max) = if use_motif {
                // Живой мотив: тема секции (A везде, B на Drop/PostChorus)
                // плюс окно визита (секции идут подряд — окно едет на шаг,
                // тема развивается, а не пересдается и не стоит на месте).
                // ДНК сида сохранена, но дословных повторов нет.
                // Без транспозиции ответа: терция вверх на нечетных фразах
                // рассинхронизировала слои (полутоновые стычки до 37% шагов).
                // Каданс: последний шаг раздела — тоника (ступень 0).
                // Раздел закрывается (развитие слышно: фраза куда-то
                // пришла), следующий открывается своей темой.
                let theme = if song.use_alt_motif {
                    &song.motif_b
                } else {
                    &song.motif
                };
                // Мотив — 16 ступеней на две доли такта: первая половина
                // звучит в первом такте фразы, вторая — во втором.
                // Индекс считается от фазы такта во фразе, а не от
                // step_in_measure: иначе обе половины фразы пели один
                // и тот же такт и «длинная тема» не развивалась.
                let bar_in_phrase = song.measure_index.saturating_sub(1) % 2;
                let bar_span = (steps_per_measure / 2).max(1);
                let rot = song.stage % theme.len();
                let motif_step = if song.is_last_measure && step_in_measure == steps_per_measure - 1
                {
                    0
                } else {
                    theme[(bar_in_phrase * bar_span + step_in_measure / 2 + rot) % theme.len()]
                        as usize
                };
                // Сдвиг голоса — счетная линия: гайд задает свою ступень
                // (соло-гитара — терцией выше риффа), тема задает
                // контур. Раньше сдвиг гайда терялся, и все носители
                // пели в унисон: вместо двух голосов — один.
                let contour = motif_step + voice_degree;
                // Верх контура в этой фразе: линия должна поместиться
                // в лад целиком (см. расчет tone_base ниже).
                let window_len = (2 * bar_span).min(theme.len());
                let mut contour_max = voice_degree;
                for k in 0..window_len {
                    let t = theme[(rot + k) % theme.len()] as usize;
                    contour_max = contour_max.max(t + voice_degree);
                }
                (contour, 0, contour_max)
            } else {
                let mut d = (voice_degree + prog_shift) % scale.len().max(1);
                // Квинтовый бас: оффбитные восьмые ходят на квинту —
                // баслайн движется, а не топчется на тонике.
                if guide.bass_fifth && matches!(role, Some(InstrumentRole::Bass)) {
                    let eighth = steps_per_measure / 8;
                    if eighth > 0 && step_in_measure % (eighth * 2) == eighth {
                        let fifth = if scale.len() == 7 { 4 } else { 3 };
                        d = (d + fifth) % scale.len().max(1);
                    }
                }
                (d, jitter, 0)
            };
            let drift = if jitter > 0 {
                // Сильные доли держат гармонию: джиттер схлопывается —
                // звучит написанная ступень (аккордовый тон скелета).
                // Слабые доли — проходящие (блуждание как раньше).
                // Та же идея, что чёт/нечет в марковском семплинге v1:
                // метр skeleton, ornament — flesh. Детерминировано.
                let q = (steps_per_measure / 4).max(1);
                if step_in_measure % q == 0 {
                    0
                } else {
                    // Блуждание тоже фиксировано сидом и визитом секции:
                    // внутри раздела та же нота в том же месте, дальше —
                    // пересдача на границе (а не посреди песни).
                    (crate::arranger::splitmix64(
                        song.seed_hash
                            ^ (slot_idx as u64).wrapping_mul(0xC0FFEE)
                            ^ (step_in_measure as u64).wrapping_mul(0x9E3779B9)
                            ^ (song.stage as u64).wrapping_mul(0x85EBCA6B)
                            ^ 0xD1F7,
                    ) % (jitter as u64 + 1)) as usize
                }
            } else {
                0
            };
            // Тоника готова в контексте (инкрементальный walk аранжировщика):
            // модуль лада берём здесь. Визит, а не фраза: центр не
            // перебивает тему внутри раздела формы.
            let tonic = song.tonic_walk.rem_euclid(scale.len().max(1) as i64) as usize;
            let len = scale.len();
            // Центр гармонии — тоника плюс ротация сида. Это КЛАСС
            // звука, а не высота: контур темы ложится поверх и не
            // заворачивается по модулю лада. Раньше сумма бралась по
            // модулю целиком, и как только она переваливала верх лада,
            // контур переворачивался назад: линия рвалась скачком на
            // септиму и обратно посреди фразы (арки не было — «горб»).
            let base_class = (tonic + rotation + drift) % len;
            // База линии — без контура: контур (ступень темы или
            // аккорда) добавляется при выборе ступени, иначе линия
            // теряет саму тему.
            let tone_base: i32 = if use_motif {
                // Контур должен влезть в лад целиком: если не влезает —
                // вся линия уходит вниз на ступень (регистр ровный,
                // форма темы цела).
                let overflow = base_class + contour_max + 1;
                let shift_down = overflow.saturating_sub(len);
                (base_class - shift_down) as i32
            } else {
                (tonic + drift + rotation) as i32
            };
            // Октава общая для тонов аккорда; подъем проверяем по корню.
            let root_idx = (tone_base + degree as i32).rem_euclid(len as i32) as usize;
            // Абсолютная высота — от сида (тон семейства + сдвиг тоники).
            let shift = tonic_shift_for(song.seed_hash);
            let root = modal_tone(scale, root_idx, song.seed_hash).saturating_add_signed(shift);
            let bump = guide.octave_up > 0.0
                // Потолок регистра лидов: выше D6 (86) — ледяной свист,
                // а не музыка. Берем минимум из гайда и колпака.
                // Бросок общий на план: подъемы унисоном, а не вразнобой.
                // Визит секции — в ногу с тоникой: регистр стоит весь раздел.
                && root.saturating_add(12) <= guide.top_pitch.min(86)
                && fixed_gate(
                    song.seed_hash,
                    groove_key,
                    step_in_measure,
                    0x0C74 ^ (song.stage as u64),
                ) < guide.octave_up;
            // Октава темы от сида: мотив и подхват едут вместе.
            // Контраст парами секций: четные пары — низко, нечетные —
            // октавой выше. Куплет/припев регистрами, а не только
            // громкостью. Граница — граница раздела, не середина песни.
            // Верх страхуют колпаки ниже.
            let seed_octave = if use_motif {
                motif_octave(song.seed_hash)
            } else {
                0
            };
            let window_octave = if use_motif {
                let w = ((song.stage / 2) % 2) as i8;
                // Направление — от регистра голоса: фолд 86 съедает сдвиг
                // вверх у высоких голосов целиком (74+12→86→74: разделы
                // звучат одинаково), поэтому высокие уводим вниз.
                // Правило статично (гайд+сид): унисон не расслаивается.
                if octave + seed_octave >= 2 {
                    -w
                } else {
                    w
                }
            } else {
                0
            };
            // Потолок регистра: линия темы уходит вниз ЦЕЛИКОМ, а не
            // отдельными нотами. Иначе верх ноты срезается на октаву
            // вниз, а нижние остаются — линия рвется посреди фразы.
            let octaves_line = octave + seed_octave + window_octave;
            let line_fold = if use_motif {
                // Верх линии — перебором ступеней, а не по максимальной
                // ступени: модальный вариант (фригийская вторая) понижает
                // ступень, и «верхняя ступень» не значит «верхняя нота».
                // Считали бы по ступени — верхняя нота осталась бы выше
                // капа и резалась по одной: линия рвалась на октаву.
                let mut top = 0i32;
                for k in 0..=contour_max {
                    let idx = (tone_base + k as i32).rem_euclid(len as i32) as usize;
                    let p = modal_tone(scale, idx, song.seed_hash) as i32;
                    if p > top {
                        top = p;
                    }
                }
                let top = top + shift as i32 + 12 * octaves_line as i32;
                let cap = guide.top_pitch.min(86) as i32;
                if top > cap {
                    (top - cap) / 12 + 1
                } else {
                    0
                }
            } else {
                0
            };
            (
                tone_base,
                degree,
                octaves_line + if bump { 1 } else { 0 },
                line_fold,
            )
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
        let idx = (tone_base + degree as i32 + *iv as i32).rem_euclid(scale.len() as i32) as usize;
        // Сдвиг линии (line_fold) — общий на фразу: потолок регистра
        // не режет отдельные ноты, иначе линия рвётся скачком на октаву.
        let mut tone = (modal_tone(scale, idx, song.seed_hash) as i32
            + tonic_shift_for(song.seed_hash) as i32
            + 12 * octaves as i32
            - 12 * line_fold)
            .clamp(0, 255) as u8;
        // Потолок темы: сид-октава могла вынести выше гайда —
        // складываем октавой вниз (класс тот же, свиста нет).
        if guide.follow_motif || force_motif {
            while tone > guide.top_pitch.min(86) {
                tone = tone.saturating_sub(12);
            }
        }
        // Низ держим внизу: уползшее выше капа складываем октавой вниз.
        // Гармония та же (тот же питч-класс). Кап от гайда: саб 45,
        // рок риффует выше (52) вслед за гитарой. Питчевый кик (808)
        // сидит в том же суб-регистре, что бас, — общий регистровый
        // закон низа; ударные-киты (фиксированный питч) это не трогает.
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
        // Акценты долей: даунбит говорит громче всех, дальше по убывающей.
        // Без иерархии громкости бит тонет в ровной стене (все стучат
        // равно) — метр слышен, даже когда рисунок синкопирован.
        v *= beat_accent(step_in_measure, steps_per_measure);
        // Дыхание такта: носители темы чуть swell к середине (±10%).
        // Арка v1 внутри фразы: пик развития — не на границе, фраза
        // дышит, а не стучит ровными ударами. Только тема (ударные
        // и бас держат метр без кача громкости).
        if guide.follow_motif || force_motif {
            let pos = step_in_measure as f32 / steps_per_measure.max(1) as f32;
            v *= 1.0 + 0.10 * (std::f32::consts::PI * pos).sin();
        }
        // Громкость фразы: баланс партий по ступени круга.
        // Ноль таблицы = тишина (снейра в интро нет — и не должно быть).
        if let Some(r) = role {
            v *= song.stage_gain(r);
        }
        // Пригибание баса под заливки — только движковым даком
        // (плавная огибающая от кика). Генератор второй ямы не делает:
        // это давало двойной провал на каждом синкопированном кике.
        if v >= 0.05 {
            let mut note =
                NoteEvent::new(instrument_id, tone, v, duration).shaped(attack_mul, release_mul);
            note.sub_bias = guide.sub_bias.clamp(0.0, 1.0);
            out.push(note);
        }
    }
    out
}

/// Гармонический ритм сида (тактов на аккорд): 2 — движение,
/// 4 — середина, 8 — фундамент. Выбор детерминирован сидом:
/// одни сиды гармонически живые, другие статичные. Чистая функция.
pub(crate) fn prog_bar_len(seed_hash: u64) -> usize {
    const LENS: [usize; 3] = [2, 4, 8];
    LENS[(crate::arranger::splitmix64(seed_hash ^ 0x6A12) % 3) as usize]
}

/// Акцент доли 1.0..1.25: даунбит — полный голос, третья доля —
/// чуть тише, слабые — как написано. Чистая функция шага.
pub(crate) fn beat_accent(step_in_measure: usize, steps_per_measure: usize) -> f32 {
    let q = (steps_per_measure / 4).max(1);
    match (step_in_measure / q) % 4 {
        0 => 1.25,
        2 => 1.12,
        _ => 1.0,
    }
}

/// Фабрика стилей по имени (регистр и пробелы игнорируются).
/// `None` для неизвестных имен — вызывающий предупреждает пользователя
/// вместо тихого включения не того стиля.
pub fn make_style(name: &str) -> Option<Box<dyn Style>> {
    use club::ClubStyle;
    use lofi::LofiStyle;
    use phonk::PhonkStyle;
    use protoclub::ProtoclubStyle;
    use rock::RockStyle;
    match name.trim().to_lowercase().as_str() {
        "club" => Some(Box::new(ClubStyle::new())),
        "lofi" => Some(Box::new(LofiStyle::new())),
        "rock" => Some(Box::new(RockStyle::new())),
        "phonk" => Some(Box::new(PhonkStyle::new())),
        "proto" => Some(Box::new(ProtoclubStyle::new())),
        _ => None,
    }
}

/// Все доступные стили: имена для CLI и Flutter.
pub fn style_names() -> &'static [&'static str] {
    &["club", "rock", "lofi", "phonk", "proto"]
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
            layers: crate::piece::layers_for(SongSection::MainMotif) | crate::piece::L_VOCAL,
            seed_hash: 42,
            density: 1.0,
            motif: [0, 2, 4, 2, 0, 3, 1, 4, 4, 3, 1, 4, 2, 0, 1, 3],
            motif_b: [1, 4, 0, 3, 2, 4, 2, 0, 4, 0, 1, 3, 3, 2, 0, 4],
            use_alt_motif: false,
            prog: [0, 0, 0, 0],
            tonic_walk: 0,
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

    #[test]
    fn phrase_end_has_a_generated_fill() {
        // Переход на стыке фразы — сгенерированная забивка: в последней
        // доле фразы снейр играет 16-е с нарастанием. Это ноты, которые
        // ведут в следующую фразу, а не кроссфейд.
        use crate::instruments::{InstrumentRole, Slot};
        for name in ["club", "rock", "phonk", "proto"] {
            let style = make_style(name).expect("known style");
            let mut mixer = crate::mixer::MixerState::new();
            crate::playback::apply_style_mix(&*style, &mut mixer);
            let mut buf = [0usize; crate::core::MAX_ACTIVE_TRACKS];
            let n = mixer.active_ids(&mut buf);
            let active = buf[..n].to_vec();
            let mut rng = rand::SeedableRng::seed_from_u64(424242);
            let mut arranger = crate::arranger::Arranger::new(424242);
            let mut song = arranger.advance_measure();
            let mut fill_notes = 0usize;
            let mut other_notes = 0usize;
            for step in 0..32usize {
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
                let mut notes = render_style(&*style, &mut sctx);
                notes.retain(|n| {
                    Slot::from_id(n.instrument_id).map(|s| s.role()) == Some(InstrumentRole::Snare)
                });
                // Последняя доля фразы (шаги 28..31 при 16 шагов на такт).
                let in_fill = step >= 32 - spm / 4;
                if in_fill {
                    fill_notes += notes.len();
                } else {
                    other_notes += notes.len();
                }
            }
            assert!(
                fill_notes > 0,
                "{name}: на стыке фразы нет забивки (fill={fill_notes})"
            );
            assert!(
                fill_notes >= other_notes / 8,
                "{name}: забивка не отличается от обычной игры: fill={fill_notes} other={other_notes}"
            );
        }
    }

    #[test]
    fn arrangement_layers_mute_the_rest() {
        // Аранж по слоям: план пишет, кто играет. Молчащая группа молчит
        // по-настоящему (не «тише»), иначе точки на колесе и звук снова
        // разойдутся.
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
            // Только грув: ни темы, ни пампы, ни вокала.
            song.layers = crate::piece::L_GROOVE;
            let mut heard_lead = 0;
            let mut heard_groove = 0;
            for step in 0..64 {
                let spm = style.steps_per_measure();
                let sim = step % spm;
                if sim == 0 && step > 0 {
                    song = arranger.advance_measure();
                    song.layers = crate::piece::L_GROOVE;
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
                    match role {
                        Some(
                            InstrumentRole::Lead | InstrumentRole::Vocal | InstrumentRole::Chords,
                        ) => heard_lead += 1,
                        Some(
                            InstrumentRole::Kick | InstrumentRole::Snare | InstrumentRole::Hat,
                        ) => heard_groove += 1,
                        _ => {}
                    }
                }
            }
            assert_eq!(heard_lead, 0, "{name}: верх играет при маске грува");
            assert!(heard_groove > 0, "{name}: грув молчит при своей маске");
        }
    }

    fn motif_pitches(motif: [u8; 16]) -> Vec<u8> {
        // Проба скелета высоты через якорь-абсолют кик-роли (id 2):
        // гейтов нет — в кадре чистая высота без дыхательных пропусков.
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
                    2,
                    5,
                    1.0,
                    false,
                    false,
                    true,
                    &mut rng,
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
        let a = motif_pitches([0, 2, 4, 2, 0, 3, 1, 4, 4, 3, 1, 4, 2, 0, 1, 3]);
        let mut song_b = test_song(0.5);
        song_b.motif = [0, 2, 4, 2, 0, 3, 1, 4, 4, 3, 1, 4, 2, 0, 1, 3];
        song_b.motif_b = [4, 4, 0, 0, 2, 2, 4, 4, 1, 3, 3, 0, 0, 4, 2, 1];
        song_b.use_alt_motif = true;
        let guide = RoleGuide {
            weights: &[1.0; 16],
            follow_motif: true,
            ..RoleGuide::pitched(&[1.0; 16], 0, 2, 0, 1.0, 0.8)
        };
        let scale = [48, 50, 52, 53, 55, 57, 59];
        let mut rng = rand::SeedableRng::seed_from_u64(3);
        // Проба через якорь-абсолют (id 2): высота без пропусков.
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
                    2,
                    5,
                    1.0,
                    false,
                    false,
                    true,
                    &mut rng,
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
        let a = motif_pitches([0, 2, 4, 2, 0, 3, 1, 4, 4, 3, 1, 4, 2, 0, 1, 3]);
        let b = motif_pitches([4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4]);
        assert_eq!(a.len(), 8);
        assert_ne!(a, b, "motif ignored");
        // Детерминизм: тот же мотив — те же ноты.
        assert_eq!(
            a,
            motif_pitches([0, 2, 4, 2, 0, 3, 1, 4, 4, 3, 1, 4, 2, 0, 1, 3])
        );
    }

    #[test]
    fn build_roll_fires_into_drop() {
        // Ролл: последний такт BuildUp — каждая 16-я с нарастанием,
        // веса и гейты игнорируются (даже нулевые веса не глушат).
        // Вне заливки — тишина (обычный путь по весам).
        fn roll_hits(section: crate::arranger::SongSection, last: bool) -> Vec<f32> {
            let guide = RoleGuide {
                weights: &[0.0; 8],
                build_roll: true,
                ..RoleGuide::kit(&[0.0; 8], 38, 0.9)
            };
            let mut song = test_song(0.5);
            song.section = section;
            song.is_last_measure = last;
            let mut rng = rand::SeedableRng::seed_from_u64(3);
            (0..8)
                .filter_map(|sim| {
                    play_guide(
                        &guide,
                        &[48],
                        &song,
                        1.0,
                        sim,
                        8,
                        0.1,
                        4,
                        3,
                        1.0,
                        false,
                        false,
                        true,
                        &mut rng,
                    )
                    .into_iter()
                    .next()
                    .map(|n| n.velocity)
                })
                .collect()
        }
        use crate::arranger::SongSection::*;
        let roll = roll_hits(BuildUp, true);
        assert_eq!(roll.len(), 8, "roll must fire every 16th");
        assert!(
            roll.last().unwrap() > roll.first().unwrap(),
            "roll must rise: {roll:?}"
        );
        assert!(roll_hits(BuildUp, false).is_empty(), "roll too early");
        assert!(
            roll_hits(MainMotif, true).is_empty(),
            "roll outside buildup"
        );
        assert!(roll_hits(Drop, true).is_empty(), "roll inside drop");
        // Без флага те же нулевые веса молчат и в заливке.
        let guide = RoleGuide {
            weights: &[0.0; 8],
            ..RoleGuide::kit(&[0.0; 8], 38, 0.9)
        };
        let mut song = test_song(0.5);
        song.section = BuildUp;
        song.is_last_measure = true;
        let mut rng = rand::SeedableRng::seed_from_u64(3);
        let silent: Vec<_> = (0..8)
            .filter_map(|sim| {
                play_guide(
                    &guide,
                    &[48],
                    &song,
                    1.0,
                    sim,
                    8,
                    0.1,
                    4,
                    3,
                    1.0,
                    false,
                    false,
                    true,
                    &mut rng,
                )
                .into_iter()
                .next()
            })
            .collect();
        assert!(silent.is_empty(), "flag off must stay silent");
    }

    #[test]
    fn motif_uses_second_half_in_second_bar() {
        // 16-ступенчатая тема: первый такт фразы поет первые 8 ступеней,
        // второй — последние 8. Раньше обе половины фразы пели один и
        // тот же такт темы, и «двухтактный мотив» был фикцией.
        let motif: [u8; 16] = [0, 0, 0, 0, 0, 0, 0, 0, 4, 4, 4, 4, 4, 4, 4, 4];
        let guide = RoleGuide {
            weights: &[1.0; 16],
            follow_motif: true,
            ..RoleGuide::pitched(&[1.0; 16], 0, 2, 0, 1.0, 0.8)
        };
        let scale = [48, 50, 52, 53, 55, 57, 59];
        let probe = |measure: usize| -> Vec<u8> {
            let mut song = test_song(0.5);
            song.stage = 0; // rot = 0: индекс темы без сдвига окна
            song.measure_index = measure;
            song.motif = motif;
            let mut rng = rand::SeedableRng::seed_from_u64(3);
            (0..8)
                .filter_map(|step| {
                    // id 2 = Kick: якорь-абсолют, гейт не дышит.
                    play_guide(
                        &guide,
                        &scale,
                        &song,
                        1.0,
                        step * 2,
                        16,
                        0.1,
                        2,
                        5,
                        1.0,
                        false,
                        false,
                        true,
                        &mut rng,
                    )
                    .into_iter()
                    .next()
                    .map(|n| n.midi_pitch)
                })
                .collect()
        };
        let bar1 = probe(1);
        let bar2 = probe(2);
        assert_eq!(bar1.len(), 8, "first bar must voice all 8 theme slots");
        assert_eq!(bar2.len(), 8, "second bar must voice all 8 theme slots");
        assert!(
            bar1.iter().all(|p| *p == bar1[0]),
            "first bar must sing theme[0..8]: {bar1:?}"
        );
        assert!(
            bar2.iter().all(|p| *p == bar2[0]),
            "second bar must sing theme[8..16]: {bar2:?}"
        );
        assert_ne!(bar1[0], bar2[0], "both bars sang the same half");
    }

    #[test]
    fn theme_contour_survives_the_scale_edge() {
        // Контур темы не переворачивается на верхней ступени лада.
        // Раньше сумма ступеней бралась по модулю длины лада, и как
        // только она переваливала верх, линия прыгала на септиму вниз
        // и обратно посреди фразы: арки не было, был «горб».
        let style = make_style("club").expect("club");
        let scale = style.scale();
        let score = style.arrangement();
        // Направляющая линии: якорь (играет всегда), мотив, высокий
        // пол, чтобы низ не сложило октавой вниз.
        let guide = RoleGuide {
            weights: &[1.0; 16],
            follow_motif: true,
            bass_top: 100,
            ..RoleGuide::pitched(&[1.0; 16], 0, 2, 0, 1.0, 0.8)
        };
        for seed in [
            42u64, 1000, 1007, 1014, 1021, 1028, 1035, 1042, 1049, 2001, 2002,
        ] {
            let mut arranger = crate::arranger::Arranger::with_score(seed, score);
            let song = arranger.advance_measure();
            let mut rng = rand::SeedableRng::seed_from_u64(seed);
            let mut line: Vec<u8> = Vec::new();
            for step in 0..16 {
                // id 2 = Kick: якорь-абсолют, гейт не дышит.
                let notes = play_guide(
                    &guide, scale, &song, 1.0, step, 16, 0.1, 2, 2, 1.0, false, false, true,
                    &mut rng,
                );
                if let Some(n) = notes.first() {
                    line.push(n.midi_pitch);
                }
            }
            assert_eq!(line.len(), 16, "seed {seed}: линия должна быть сплошной");
            for w in line.windows(2) {
                let d = (w[1] as i32 - w[0] as i32).abs();
                assert!(
                    d < 7,
                    "seed {seed}: линия рвется скачком на {d} полутонов: {line:?}"
                );
            }
            // Линия идет по форме темы: соседние ступени темы — разные
            // высоты, а не одна нота на весь такт.
            let distinct: std::collections::BTreeSet<u8> = line.iter().copied().collect();
            assert!(
                distinct.len() >= 3,
                "seed {seed}: тема не слышна (одна нота): {line:?}"
            );
        }
    }

    #[test]
    fn non_anchor_gates_develop_across_phrase_bars() {
        // Второй такт фразы получает свою соль гейта: рисунок
        // развивается, а не дублирует первый такт дословно.
        // Якоря — исключение (скелет держится фразу), здесь веса 0.5.
        let guide = RoleGuide {
            weights: &[0.5; 16],
            ..RoleGuide::pitched(&[0.5; 16], 0, 1, 0, 1.0, 0.7)
        };
        let scale = [48, 50, 52, 53, 55, 57, 59];
        let mask = |seed: u64, measure: usize| -> Vec<bool> {
            let mut song = test_song(0.5);
            song.seed_hash = seed;
            song.measure_index = measure;
            song.phrase_index = 0;
            song.stage = 0;
            let mut rng = rand::SeedableRng::seed_from_u64(1);
            (0..16)
                .map(|step| {
                    !play_guide(
                        &guide, &scale, &song, 1.0, step, 16, 0.1, 4, 3, 1.0, false, false, true,
                        &mut rng,
                    )
                    .is_empty()
                })
                .collect()
        };
        let differing = (0..64u64).filter(|s| mask(*s, 1) != mask(*s, 2)).count();
        assert!(
            differing >= 16,
            "second bar cloned the first: {differing}/64 seeds differ"
        );
    }

    #[test]
    fn drop_break_is_only_before_drop() {
        // Вдох перед дропом: последняя четверть последнего такта BuildUp.
        // Не весь Drop-такт (это сбивка после удара) и не каждый BuildUp.
        let guide = RoleGuide {
            weights: &[1.0; 8],
            drop_before_climax: true,
            ..RoleGuide::kit(&[1.0; 8], 36, 0.9)
        };
        let probe = |section, last: bool, step: usize| -> bool {
            let mut song = test_song(0.5);
            song.section = section;
            song.is_last_measure = last;
            let mut rng = rand::SeedableRng::seed_from_u64(3);
            // id 2 = Kick: якорь-абсолют, тишину дает только брейк.
            !play_guide(
                &guide,
                &[48],
                &song,
                1.0,
                step,
                8,
                0.1,
                2,
                1,
                1.0,
                false,
                false,
                true,
                &mut rng,
            )
            .is_empty()
        };
        use crate::arranger::SongSection::*;
        for step in 0..6 {
            assert!(probe(BuildUp, true, step), "too early break at {step}");
        }
        for step in 6..8 {
            assert!(!probe(BuildUp, true, step), "no breath at {step}");
        }
        for step in 0..8 {
            assert!(probe(BuildUp, false, step), "break in non-last BuildUp");
            assert!(probe(Drop, true, step), "break inside Drop");
        }
    }

    #[test]
    fn section_poles_vary_groove_by_section() {
        // Полюса по секциям: один сид — разные биты в разных секциях
        // (у дропа свой), та же секция — тот же бит (якорь формы).
        use crate::arranger::SongSection;
        fn pole_for(seed: u64, section: SongSection) -> Vec<usize> {
            let guide = RoleGuide {
                weights: &[0.0; 8],
                poles: Some(POLES),
                section_poles: true,
                ..RoleGuide::kit(&[0.0; 8], 36, 0.9)
            };
            let mut song = test_song(0.0);
            song.seed_hash = seed;
            song.section = section;
            let mut rng = rand::SeedableRng::seed_from_u64(1);
            (0..8)
                .filter(|sim| {
                    !play_guide(
                        &guide,
                        &[48],
                        &song,
                        1.0,
                        *sim,
                        8,
                        0.1,
                        2,
                        1,
                        1.0,
                        false,
                        false,
                        true,
                        &mut rng,
                    )
                    .is_empty()
                })
                .collect()
        }
        use crate::arranger::SongSection::*;
        // Детерминизм: та же секция — тот же грув.
        assert_eq!(
            pole_for(11, Drop),
            pole_for(11, Drop),
            "section groove not stable"
        );
        // Хотя бы у одного сида из первых 16 секции дают разные биты.
        let varied = (0..16u64).any(|seed| {
            let a = pole_for(seed, MainMotif);
            pole_for(seed, Drop) != a
                || pole_for(seed, BuildUp) != a
                || pole_for(seed, PostChorus) != a
        });
        assert!(varied, "section poles never vary the groove");
    }

    #[test]
    fn prog_rhythm_varies_by_seed() {
        // Гармонический ритм — почерк сида: встречаются 2, 4 и 8,
        // тот же сид всегда даёт тот же ритм.
        let mut seen = std::collections::HashSet::new();
        for seed in 0..32u64 {
            let h = crate::arranger::splitmix64(seed ^ 0x51ED);
            let len = prog_bar_len(h);
            assert!([2, 4, 8].contains(&len), "bad rhythm: {len}");
            assert_eq!(len, prog_bar_len(h), "not deterministic");
            seen.insert(len);
        }
        assert_eq!(seen.len(), 3, "rhythm never varies: {seen:?}");
    }

    #[test]
    fn strong_beats_hold_chord_tones() {
        // Сильные доли (четверти) поют написанную ступень без джиттера
        // (скелет гармонии), слабые — с блужданием (проходящие).
        // Тот же механизм, что чёт/нечет в марковском семплинге v1,
        // но детерминированный и без отдельного кода под инструмент.
        fn pitch(jitter: usize, sim: usize) -> u8 {
            // Проба кик-якорем (id 2): якоря бочки гейт не проходят —
            // в кадре чистая высота без дыхательных пропусков.
            let guide = RoleGuide {
                weights: &[1.0; 8],
                ..RoleGuide::pitched(&[1.0; 8], 1, 0, jitter, 1.0, 0.8)
            };
            let scale = [48, 50, 52, 53, 55, 57, 59];
            let song = test_song(0.5);
            let mut rng = rand::SeedableRng::seed_from_u64(3);
            play_guide(
                &guide, &scale, &song, 1.0, sim, 8, 0.1, 2, 0, 1.0, false, false, true, &mut rng,
            )
            .into_iter()
            .next()
            .map(|n| n.midi_pitch)
            .expect("strong beat silent")
        }
        // Четверти (0 и 4 при сетке 8): джиттер схлопнут — питч тот же,
        // что у гайда без джиттера.
        assert_eq!(pitch(2, 0), pitch(0, 0), "downbeat drifts");
        assert_eq!(pitch(2, 4), pitch(0, 4), "third beat drifts");
        // Хотя бы одна слабая доля (нечётные при сетке 8) блуждает —
        // иначе ornament мёртв. Детерминировано: либо всегда, либо никогда.
        let moved = (1..8).any(|sim| sim % 2 == 1 && pitch(2, sim) != pitch(0, sim));
        assert!(moved, "passing tones never wander");
    }

    #[test]
    fn section_end_closes_on_tonic() {
        // Каданс: последний шаг раздела — тоника независимо от мотива.
        // Раздел закрывается (фраза куда-то пришла), следующий
        // открывается своей темой. Проба якорным носителем (id 2):
        // гейтов нет, в кадре чистая высота.
        fn last_step_pitch(motif: [u8; 16], last_measure: bool) -> u8 {
            let guide = RoleGuide {
                weights: &[1.0; 16],
                follow_motif: true,
                ..RoleGuide::pitched(&[1.0; 16], 0, 2, 0, 1.0, 0.8)
            };
            let scale = [48, 50, 52, 53, 55, 57, 59];
            let mut song = test_song(0.5);
            song.motif = motif;
            song.is_last_measure = last_measure;
            let mut rng = rand::SeedableRng::seed_from_u64(3);
            play_guide(
                &guide, &scale, &song, 1.0, 15, 16, 0.1, 2, 5, 1.0, false, false, true, &mut rng,
            )
            .into_iter()
            .next()
            .map(|n| n.midi_pitch)
            .expect("cadence step silent")
        }
        // Высокий мотив в конце раздела падает на ту же тонику,
        // что низкий мотив (оба — ступень 0).
        assert_eq!(
            last_step_pitch([4; 16], true),
            last_step_pitch([0; 16], true),
            "cadence misses tonic"
        );
        // А не в конце раздела каданса нет: высокий мотив высок.
        assert_ne!(
            last_step_pitch([4; 16], true),
            last_step_pitch([4; 16], false),
            "cadence fires mid-section"
        );
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
                    &guide,
                    &[48],
                    &song,
                    1.0,
                    *step,
                    8,
                    0.1,
                    2,
                    slot_idx,
                    1.0,
                    false,
                    false,
                    true,
                    &mut rng,
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
        let notes = play_guide(
            &guide, &scale, &song, 1.0, 0, 8, 0.1, 3, 2, 1.0, false, false, true, &mut rng,
        );
        assert_eq!(notes.len(), 3, "pad must voice a triad");
        let mut pitches: Vec<u8> = notes.iter().map(|n| n.midi_pitch).collect();
        pitches.sort_unstable();
        // Терция и квинта выше корня, все в пределах октавы.
        assert!(pitches[1] > pitches[0] && pitches[2] > pitches[1]);
        assert!(pitches[2] - pitches[0] <= 12, "triad too wide: {pitches:?}");
    }

    #[test]
    fn accents_skip_some_visits() {
        // Акценты с prior звучат не в каждой секции, но и не никогда:
        // аранжировка сида, а не долбежка и не тишина. Шаг — визит.
        let guide = RoleGuide {
            ..RoleGuide::kit(&[1.0; 8], 74, 0.8)
        };
        let mut hits = 0;
        for stage in 0..17usize {
            let mut song = test_song(0.9);
            song.is_last_measure = true;
            song.stage = stage;
            let mut rng = rand::SeedableRng::seed_from_u64(5);
            // id 11 = Vocal (prior 0.75), индекс слота 9 (см. Slot::ALL).
            if !play_guide(
                &guide,
                &[48],
                &song,
                1.0,
                0,
                8,
                0.1,
                11,
                9,
                1.0,
                false,
                false,
                true,
                &mut rng,
            )
            .is_empty()
            {
                hits += 1;
            }
        }
        assert!(hits > 0 && hits < 17, "vocal sections: {hits}/17");
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
                    if matches!(role, Some(InstrumentRole::Lead | InstrumentRole::Vocal)) {
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
        // Тоника: старт в нуле, детерминизм, шаг ≤ ступень между парами.
        // Прямая свёртка шагов (эталон O(n), только для тестов).
        fn walk(seed_hash: u64, pair: usize, span: usize) -> usize {
            let mut pos: i64 = 0;
            for p in 0..pair {
                pos += crate::arranger::drift_step(seed_hash, p);
            }
            pos.rem_euclid(span.max(1) as i64) as usize
        }
        assert_eq!(walk(42, 0, 7), 0);
        assert_eq!(walk(42, 5, 7), walk(42, 5, 7));
        for seed in [0u64, 1, 42, 777] {
            for ph in 0..200usize {
                let a = walk(seed, ph, 7) as i64;
                let b = walk(seed, ph + 1, 7) as i64;
                let d = (b - a).rem_euclid(7).min((a - b).rem_euclid(7));
                assert!(d <= 1, "seed {seed} phrase {ph}: jump {d}");
            }
        }
        // Непериодичность: на окне 200 пар нет периода ≤ 32
        // (линейное +1/пара дало бы период 7 — цикл длиной в лад).
        for seed in [5u64, 12345] {
            let path: Vec<usize> = (0..200).map(|ph| walk(seed, ph, 7)).collect();
            let loops = (1..=32).any(|t| path.iter().zip(path.iter().skip(t)).all(|(a, b)| a == b));
            assert!(!loops, "seed {seed}: walk loops");
        }
        // Инкрементальный walk аранжировщика совпадает с прямой
        // свёрткой на длинном прогоне (600 тактов ≈ 75+ пар визитов):
        // O(1) на такт без смены музыки.
        for seed in [0u64, 7, 424242] {
            let seed_hash = crate::arranger::splitmix64(seed ^ 0x51ED);
            let mut arranger = crate::arranger::Arranger::new(seed);
            for _ in 0..600 {
                let ctx = arranger.advance_measure();
                let mut pos: i64 = 0;
                for p in 0..ctx.stage / 2 {
                    pos += crate::arranger::drift_step(seed_hash, p);
                }
                assert_eq!(
                    ctx.tonic_walk, pos,
                    "seed {seed} stage {}: incremental drift diverged",
                    ctx.stage
                );
            }
        }
    }

    #[test]
    fn motif_skeleton_holds_window_develops_across_windows() {
        // Общая фраза: фразы одного раздела делят скелет (окно темы —
        // визит секции, тоника — пара визитов) — мелодия цельная,
        // а не пересдается. Развитие — разделами: фраза 0 vs 8
        // звучат по-разному, за 64 фразы в один шаблон не замыкается.
        // Кик-роль (id 2): якорь-абсолют, гейтов нет — в кадре
        // чистый скелет высоты без дыхательных пропусков.
        // Тестовый мир: раздел длится 4 фразы (stage = phrase / 4).
        fn phrase_pitches(phrase: usize) -> Vec<u8> {
            let guide = RoleGuide {
                weights: &[1.0; 16],
                follow_motif: true,
                ..RoleGuide::pitched(&[1.0; 16], 0, 2, 0, 1.0, 0.8)
            };
            let scale = [48, 50, 52, 53, 55, 57, 59];
            let mut song = test_song(0.5);
            song.phrase_index = phrase;
            song.stage = phrase / 4;
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
                        2,
                        5,
                        1.0,
                        false,
                        false,
                        true,
                        &mut rng,
                    )
                    .into_iter()
                    .next()
                    .map(|n| n.midi_pitch)
                })
                .collect()
        }
        let p0 = phrase_pitches(0);
        assert_eq!(p0.len(), 8);
        assert_eq!(p0, phrase_pitches(0), "same phrase must repeat");
        assert_eq!(p0, phrase_pitches(1), "same section shares skeleton");
        assert_ne!(p0, phrase_pitches(8), "sections must develop");
        // Развитие без зацикливания: линий за 64 фразы (16 разделов)
        // больше, чем окон ротации-одиночки — блуждание тоники
        // рвет шаблон.
        let distinct: std::collections::HashSet<Vec<u8>> = (0..64).map(phrase_pitches).collect();
        assert!(
            distinct.len() > 8,
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
        // Эталон снимаем с допусками: якорь дышит (0.95) — пара шагов
        // из 16 может молчать, сверка идет по звучавшим.
        let full_line: Vec<(usize, u8)> = (0..16)
            .filter_map(|step| {
                play_guide(
                    &anchor_motif,
                    scale,
                    &song,
                    1.0,
                    step,
                    spm,
                    0.1,
                    6,
                    5,
                    1.0,
                    false,
                    false,
                    true,
                    &mut rng,
                )
                .into_iter()
                .next()
                .map(|n| (step, n.midi_pitch))
            })
            .collect();
        assert!(
            full_line.len() >= 12,
            "anchor lost face: {}/16",
            full_line.len()
        );
        // Клавиши без подхвата и с подхватом — разные линии.
        let line = |force: bool| -> Vec<(usize, u8)> {
            let mut rng = rand::SeedableRng::seed_from_u64(22);
            (0..16)
                .filter_map(|step| {
                    play_guide(
                        &guides[ki],
                        scale,
                        &song,
                        1.0,
                        step,
                        spm,
                        0.1,
                        Slot::Keys.id(),
                        ki,
                        1.0,
                        force,
                        false,
                        true,
                        &mut rng,
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
            if let Some((_, ref_pitch)) = full_line.iter().find(|(s, _)| s == step) {
                assert_eq!(*pitch, *ref_pitch, "step {step}: not the motif");
            }
        }
        // Уровень движка: активны только клавиши — подхват включается сам,
        // корень аккорда на каждом шаге лежит на линии мотива
        // (верхние терции трезвучия — гармонизация темы, а не уход с нее).
        let active = vec![Slot::Keys.id()];
        // Несколько фраз: гейт фразы может пропустить якорь (даунбит
        // дышит) — контракт «ключи одни держат тему», а не «в этом
        // такте». Высота сверяется только на фразе 0 (там снят эталон).
        let mut song_m = song.clone();
        let mut rng = rand::SeedableRng::seed_from_u64(23);
        let mut heard = 0;
        for phrase in 0..4usize {
            song_m.phrase_index = phrase;
            let mut sctx = StepContext {
                step: 0,
                step_in_measure: 0,
                base_step_secs: style.base_step_secs(),
                active_tracks: &active,
                song: &song_m,
                rng: &mut rng,
                intensity: 1.0,
            };
            for (sim, expected) in full_line.iter() {
                sctx.step = *sim;
                sctx.step_in_measure = *sim;
                let mut step_pitches = Vec::new();
                for note in render_style(&*style, &mut sctx) {
                    assert_eq!(note.instrument_id, Slot::Keys.id());
                    step_pitches.push(note.midi_pitch);
                }
                if phrase == 0 && !step_pitches.is_empty() {
                    // Первая нота озвучки — корень: тема на месте.
                    assert_eq!(step_pitches[0], *expected, "render step {sim}: motif lost");
                }
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
            crate::playback::apply_style_mix(&*style, &mut mixer);
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
            &guide,
            &[48, 50, 52, 53, 55, 57, 59],
            &song,
            1.0,
            0,
            8,
            0.1,
            3,
            2,
            1.0,
            false,
            false,
            true,
            &mut rng,
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
            &guide,
            &[48, 50, 52, 53, 55, 57, 59],
            &song,
            1.0,
            0,
            8,
            0.1,
            1,
            0,
            1.0,
            false,
            false,
            true,
            &mut rng,
        );
        assert!(!notes.is_empty());
        assert_eq!(notes[0].sub_bias, 0.8);
        guide.sub_bias = 0.0;
        let mut rng = rand::SeedableRng::seed_from_u64(9);
        let notes = play_guide(
            &guide,
            &[48, 50, 52, 53, 55, 57, 59],
            &song,
            1.0,
            0,
            8,
            0.1,
            1,
            0,
            1.0,
            false,
            false,
            true,
            &mut rng,
        );
        assert_eq!(notes[0].sub_bias, 0.0);
    }

    #[test]
    fn rock_guitar_cuts_through_mix() {
        // Рифф слышно: гитара дает не меньше 24 нот за 24 такта дефолта.
        use crate::instruments::Slot;
        let style = make_style("rock").expect("rock");
        let seed = 5u64;
        let mut mixer = crate::mixer::MixerState::new();
        crate::playback::apply_style_mix(&*style, &mut mixer);
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
                crate::playback::apply_style_mix(&*style, &mut mixer);
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
                            let gi = Slot::ALL.iter().position(|s| *s == slot).unwrap();
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
        let keys: std::collections::HashSet<i8> = (0..64u64).map(tonic_shift_for).collect();
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
                &guide, &scale, &song, 1.0, step, 16, 0.1, 6, 5, 1.0, false, false, true, &mut rng,
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
                // Октава темы — та же: меряем сдвиг тоники, а не октаву.
                if motif_octave(h1) != motif_octave(h2) {
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
        assert!(
            (1.4..=1.8).contains(&style.swing_ratio()),
            "swing out of dusty band"
        );
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
        // Бас ходит корнями за прогрессией, но гармонический ритм —
        // 8 тактов: корни стоят и держат фразу, а не едут каждые 2 такта.
        let guide = RoleGuide {
            weights: &[1.0; 8],
            ..RoleGuide::pitched(&[1.0; 8], 0, -1, 0, 1.0, 0.8)
        };
        let scale = [48, 51, 53, 55, 58];
        let mut pitches = Vec::new();
        for measure in (0..32usize).step_by(4) {
            let mut song = test_song(0.5);
            song.measure_index = measure;
            song.prog = [0, 3, 1, 2];
            let mut rng = rand::SeedableRng::seed_from_u64(3);
            if let Some(n) = play_guide(
                &guide, &scale, &song, 1.0, 0, 8, 0.1, 1, 0, 1.0, false, false, true, &mut rng,
            )
            .into_iter()
            .next()
            {
                pitches.push(n.midi_pitch);
            }
        }
        assert!(pitches.len() >= 6, "bass gone: {pitches:?}");
        // Бас ходит корнями за прогрессией (разброс есть — не дрон),
        // но сидит в регистре (фолд держит ≤ bass_top) и не гуляет
        // по грифу: разброс в пределах сексты.
        let spread = pitches.iter().max().unwrap() - pitches.iter().min().unwrap();
        assert!(
            (4..=10).contains(&spread),
            "bass drones or wanders: {pitches:?}"
        );
        assert!(
            pitches.iter().all(|p| *p <= 45),
            "bass escaped register: {pitches:?}"
        );
    }

    #[test]
    fn role_plans_are_shared_by_performers() {
        // План роли — один срез на всех исполнителей (ptr-равенство,
        // не совпадение значений): club плак+гитара делят бой.
        use crate::instruments::Slot;
        let lofi = make_style("lofi").expect("lofi");
        let guides = lofi.slot_guides();
        let at = |s: Slot| {
            Slot::ALL
                .iter()
                .position(|x| *x == s)
                .map(|i| &guides[i])
                .unwrap()
        };
        assert!(std::ptr::eq(
            at(Slot::Pluck).weights.as_ptr(),
            at(Slot::Guitar).weights.as_ptr()
        ));
        let rock = make_style("rock").expect("rock");
        let guides = rock.slot_guides();
        let at = |s: Slot| {
            Slot::ALL
                .iter()
                .position(|x| *x == s)
                .map(|i| &guides[i])
                .unwrap()
        };
        // Рифф и соло-гитара в роке — разные партии: соло играет свой
        // план (свой ритм), а не дубль риффа.
        assert!(!std::ptr::eq(
            at(Slot::GuitarLead).weights.as_ptr(),
            at(Slot::Guitar).weights.as_ptr()
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
            crate::playback::apply_style_mix(&*style, &mut mixer);
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
        for name in ["club", "rock", "lofi", "phonk", "proto"] {
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
    fn leader_balance_boosts_lead_and_dips_support() {
        // Единый баланс: лидер мелодии громче, остальные мелодические
        // тише; барабаны и бас не трогаются. Меньше двух мелодических —
        // балансировать нечего.
        let lead = crate::instruments::Slot::Pluck.id();
        let pad = crate::instruments::Slot::Pad.id();
        let kick = crate::instruments::Slot::Kick.id();
        let mut notes = vec![
            NoteEvent::new(lead, 60, 0.8, 0.1),
            NoteEvent::new(pad, 60, 0.8, 0.1),
            NoteEvent::new(kick, 36, 0.8, 0.1),
        ];
        let leader = balance_leader(&[lead, pad], Some(lead), 0);
        assert_eq!(leader, Some(lead));
        apply_leader_balance(&mut notes, leader);
        assert!(notes[0].velocity > 0.8, "leader not boosted");
        assert!(notes[1].velocity < 0.8, "support not dipped");
        assert_eq!(notes[2].velocity, 0.8, "drums must stay untouched");
        assert_eq!(balance_leader(&[pad], Some(pad), 0), None);
    }

    #[test]
    fn motif_octave_covers_all_three() {
        // Октава темы от сида: −1/0/+1 все выпадают (иначе фича мертва),
        // детерминирована хэшем.
        use std::collections::HashSet;
        let seen: HashSet<i8> = (0..64u64)
            .map(|s| motif_octave(crate::arranger::splitmix64(s ^ 0x51ED)))
            .collect();
        assert_eq!(seen, HashSet::from([-1, 0, 1]), "octave stuck: {seen:?}");
        assert_eq!(motif_octave(42), motif_octave(42), "not deterministic");
    }

    #[test]
    fn theme_leader_rotates_deterministically() {
        // Лидерство: ротация по визитам секций (не залипает на одном),
        // детерминировано, пусто = некому вести, один = всегда он.
        // Внутри раздела лидер стоит — состав не тасуется.
        use std::collections::HashSet;
        let carriers = vec![6usize, 18];
        let leaders: HashSet<usize> = (0..16usize)
            .map(|s| theme_leader(&carriers, 42, s).unwrap())
            .collect();
        assert!(leaders.len() > 1, "leadership stuck: {leaders:?}");
        for l in &leaders {
            assert!(carriers.contains(l), "stranger leads: {l}");
        }
        assert_eq!(
            theme_leader(&carriers, 42, 3),
            theme_leader(&carriers, 42, 3),
            "not deterministic"
        );
        assert_eq!(theme_leader(&[], 42, 0), None);
        assert_eq!(theme_leader(&[6], 42, 9), Some(6));
    }

    #[test]
    fn window_octave_shifts_theme_by_octave() {
        // Контраст парами секций: та же линия темы, сдвинутая ровно
        // на октаву — куплет/припев регистрами. Плоский мотив: ротация
        // нейтральна; сдвиг равномерный (форма цела), величина октава
        // с допуском на дрейф тоники. Разделы 0 vs 2 (разные пары).
        // Направление — от регистра (вниз у высоких): фолд 86 иначе
        // съедает сдвиг вверх целиком и разделы звучат одинаково.
        let guide = RoleGuide {
            weights: &[1.0; 16],
            follow_motif: true,
            ..RoleGuide::pitched(&[1.0; 16], 0, 2, 0, 1.0, 0.8)
        };
        let scale = [48, 50, 52, 53, 55, 57, 59];
        let pitches = |stage: usize| -> Vec<(usize, u8)> {
            let mut song = test_song(0.5);
            song.stage = stage;
            song.motif = [1; 16];
            let mut rng = rand::SeedableRng::seed_from_u64(3);
            (0..16)
                .filter_map(|sim| {
                    play_guide(
                        &guide, &scale, &song, 1.0, sim, 16, 0.1, 6, 5, 1.0, false, false, true,
                        &mut rng,
                    )
                    .into_iter()
                    .next()
                    .map(|n| (sim, n.midi_pitch))
                })
                .collect()
        };
        let a = pitches(0);
        let b = pitches(2);
        assert!(!a.is_empty() && !b.is_empty(), "theme gone");
        // Сверка по общим звучавшим долям (якорь дышит — пропуски
        // возможны, но их мало и они не рвут линию).
        let shared: Vec<(u8, u8)> = a
            .iter()
            .filter_map(|(s, p)| b.iter().find(|(t, _)| t == s).map(|(_, q)| (*p, *q)))
            .collect();
        assert!(
            shared.len() >= 12,
            "window lost notes: {} shared",
            shared.len()
        );
        let shift = shared[0].1 as i16 - shared[0].0 as i16;
        // Величина — октава ± дрейф тоники (2 шага блуждания — не больше
        // ±2 ступеней ≈ ±4 полутона). Для сида 42 выходит −9.
        assert!(
            (8..=16).contains(&shift.abs()),
            "window must move ~an octave, got {shift}: {a:?} -> {b:?}"
        );
        for (x, y) in &shared {
            assert_eq!(*y as i16 - *x as i16, shift, "shape bent");
        }
    }

    #[test]
    fn beat_accent_orders_downbeat_first() {
        // Иерархия долей: даунбит громче всех (вся первая четверть),
        // третья доля — следом, слабые — как написано.
        // Бит читается даже в синкопе.
        assert_eq!(beat_accent(0, 16), 1.25);
        assert_eq!(beat_accent(3, 16), 1.25);
        assert_eq!(beat_accent(4, 16), 1.0);
        assert!(beat_accent(8, 16) > beat_accent(4, 16));
        assert_eq!(beat_accent(12, 16), 1.0);
    }

    #[test]
    fn swing_varies_by_seed_within_band() {
        // Свинг сида: 0.93..1.07, детерминирован, реально варьируется
        // (min ниже 0.96, max выше 1.04) — у сидов разный кач.
        // Club свингован (deep house 54–58%), а не прямой.
        for h in [0u64, 1, 42, 777, 424242] {
            let v = swing_var(h);
            assert!((0.93..=1.07).contains(&v), "swing out of band: {v}");
            assert_eq!(v, swing_var(h), "not deterministic");
        }
        let vals: Vec<f32> = (0..64u64).map(swing_var).collect();
        assert!(
            vals.iter().cloned().fold(1.0f32, f32::min) < 0.96
                && vals.iter().cloned().fold(0.0f32, f32::max) > 1.04,
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
            // Proto держит один steady-бит весь трек (как видео-реф):
            // словарь бита там не нужен, сиды различаются остальным.
            if *name == "proto" {
                continue;
            }
            let style = make_style(name).expect("style");
            let mut pats = std::collections::HashSet::new();
            for seed in 0..12u64 {
                let mut mixer = crate::mixer::MixerState::new();
                crate::playback::apply_style_mix(&*style, &mut mixer);
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
            assert!(
                pats.len() >= 6,
                "{name}: one beat per style ({})",
                pats.len()
            );
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
            &guide,
            &[48, 50, 52, 53, 55, 57, 59],
            &song,
            1.0,
            0,
            8,
            0.5,
            3,
            2,
            1.0,
            false,
            false,
            true,
            &mut rng,
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
        // Аккомпанемент следует прогрессии (разные окна — разные ступени),
        // мотив при той же прогрессии стабилен. Гармонический ритм —
        // от сида (2/4/8): пара тактов 1 vs 11 даёт разные окна при любом
        // ритме, прогрессия [0,2,1,3] даёт разные ступени в этих окнах.
        // Смотрятся все шаги: якорь фразы может пропустить даунбит
        // (даунбит дышит) — контракт про свойство, а не про нулевой шаг.
        fn degree_at(measure: usize, follow_motif: bool, step: usize) -> Option<u8> {
            let guide = RoleGuide {
                weights: &[1.0; 8],
                follow_motif,
                ..RoleGuide::pitched(&[1.0; 8], 0, 1, 0, 1.0, 0.8)
            };
            let scale = [48, 50, 52, 53, 55, 57, 59];
            let mut song = test_song(0.5);
            song.measure_index = measure;
            song.prog = [0, 2, 1, 3];
            let mut rng = rand::SeedableRng::seed_from_u64(3);
            play_guide(
                &guide, &scale, &song, 1.0, step, 8, 0.1, 7, 6, 1.0, false, false, true, &mut rng,
            )
            .into_iter()
            .next()
            .map(|n| n.midi_pitch)
        }
        // Такты 1 и 11: окна прогрессии разные при ритме 2, 4 и 8.
        let mut shifted = false;
        // Мотив от такта не зависит.
        let mut stable = false;
        for step in 0..8 {
            if let (Some(a), Some(b)) = (degree_at(1, false, step), degree_at(11, false, step)) {
                assert_ne!(a, b, "step {step}: accompaniment ignores progression");
                shifted = true;
            }
            if let (Some(a), Some(b)) = (degree_at(1, true, step), degree_at(11, true, step)) {
                assert_eq!(a, b, "step {step}: motif follows progression");
                stable = true;
            }
        }
        assert!(shifted && stable, "too few common steps");
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
                        &guide,
                        &[48],
                        &song,
                        1.0,
                        *step,
                        16,
                        0.1,
                        6,
                        5,
                        share,
                        false,
                        false,
                        true,
                        &mut rng,
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
                            &anchor,
                            &[48],
                            &song,
                            1.0,
                            step,
                            8,
                            0.1,
                            2,
                            0,
                            share,
                            false,
                            false,
                            true,
                            &mut rng,
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
