//! Генератор композиции: тактовый цикл, опрашивающий стиль.
//!
//! Петля не владеет музыкальными решениями: стиль дает направляющие,
//! микшер — активные треки, темп — живую ручку BPM. Все три меняются
//! на ходу без перезапуска потока.
//!
//! Иерархия голосов одна и живет в [`crate::styles::render_style`]:
//! ведущий темы звучит в полную, остальные мелодические — поддержкой.
//! Композер не выбирает второго лидера: иначе тема и баланс партий
//! расходились бы.

use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, RwLock,
};
use std::thread;
use std::time::{Duration, Instant};

use crossbeam::queue::ArrayQueue;
use rand::rngs::StdRng;
use rand::SeedableRng;

use crate::arranger::{Arranger, SongContext};
use crate::core::{base_step_secs, normalize_tempo, snapshot, swing_step_secs, MAX_ACTIVE_TRACKS};
use crate::instruments::{InstrumentRole, NoteEvent, Slot};
use crate::mixer::MixerState;
use crate::styles::{render_style, StepContext, Style};

/// Живой темп: `None` = дефолт стиля, `Some(bpm)` = ручка фронта.
pub type LiveTempo = Arc<RwLock<Option<f32>>>;

/// Живой снимок для кругового визуалайзера: секция, энергия,
/// мотив сида и позиция в нем. Обновляется композером каждый шаг
/// (дешево: маленький Copy-набор), читается UI по опросу.
#[derive(Clone, Debug, Default)]
pub struct VisualSnap {
    pub section: u8,
    pub energy: f32,
    pub motif: [u8; 16],
    /// Позиция темы на этом шаге (индекс в motif).
    pub motif_pos: u8,
    /// Доля такта 0..1 (стрелка круга).
    pub step_phase: f32,
    /// Маска звучавших долей такта (бит = 16-я доля): точки инструментов.
    /// Сбрасывается на границе такта.
    pub bar_hits: u16,
    /// Маски по главным голосам (тот же бит = та же доля):
    /// кик, снейр+хэт, бас, тема (лид/вокал/аккорды).
    /// Колесо ставит точку голоса на его долях: барабан 6 раз
    /// за такт — 6 точек голоса по кругу.
    pub kick_hits: u16,
    pub snare_hits: u16,
    pub bass_hits: u16,
    pub theme_hits: u16,
    /// Текущая фраза (бит = доля фразы, фраза = 2 такта):
    /// рука делает оборот за фразу.
    /// 32 бита = 2×16 шагов; встроенные стили 16-шаговые.
    pub phrase_len: u8,
    pub phrase_pos: u8,
    /// Точная фаза фразы 0..1 (целые шаги + доля текущего по часам):
    /// якорь колеса без лесенки опросов.
    pub phrase_phase: f32,
    /// Фаза окна колеса 0..1 (окно = 4 такта): по ней идёт стрелка и
    /// считается «возраст» точки. Окно шире фразы, поэтому след не
    /// обрывается на границе фразы.
    pub window_phase: f32,
    /// Ноты окна по голосам (бит = доля окна): кик, снейр+хэт,
    /// бас, тема (лид/вокал/аккорды). Это настоящие ноты, а не план:
    /// колесо показывает ровно то, что звучало, поэтому показа и звука
    /// разойтись не могут.
    pub pkick: u64,
    pub psnare: u64,
    pub pbass: u64,
    pub ptheme: u64,
    /// Доли фразы, где что-то звучало (бит = доля фразы).
    /// Сброс на старте фразы.
    pub pplayed: u32,
}

/// Номер секции для снимка (порядок объявления SongSection).
pub(crate) fn section_index(section: crate::arranger::SongSection) -> u8 {
    section as u8
}

/// Окно колеса: сколько шагов помнятся точки (4 такта = 64 шага).
/// Шире музыкальной фразы (2 такта) — иначе на границе фразы маски
/// обнулялись и радар очищался в тот момент, когда стрелка приходит
/// вверх.
pub const WHEEL_WINDOW_STEPS: usize = 64;

/// Метка шага: номер шага и его длительность в микросекундах.
///
/// Движок строит по меткам сетку времени. Ноты бывают не на каждом
/// шаге (паузы формы, разреженные слои), а сетка нужна на каждом:
/// пропуски, оценённые линейно по длине текущего шага, копят ошибку
/// (при свинге длинный и короткий шаг отличаются на 20-50 мс, и через
/// пару секунд ноты «опоздали» на полсекунды — ритм сбивался, а сетка
/// пере-якоривалась). Метка — честная длина шага.
#[derive(Clone, Copy, Debug)]
pub struct StepMark {
    pub step: u64,
    pub us: u32,
}

/// Тень перехода: следующий кусок, который поет поверх грува текущего
/// в шве. Голос входит мягко (громкость растет к концу шва), грув
/// остается один — иначе стык превратится в двух голосов.
struct Shadow {
    arranger: Arranger,
    song: SongContext,
}

pub struct Composer {
    queue: Arc<ArrayQueue<NoteEvent>>,
    /// Метки шагов (длина каждого шага): движок строит по ним сетку
    /// времени. Без них сетка строится по нотам, и пропуски оцениваются
    /// линейно — тайминг уплывает (ритм сбивается).
    marks: Arc<ArrayQueue<StepMark>>,
    style: Arc<RwLock<Box<dyn Style>>>,
    mixer: Arc<RwLock<MixerState>>,
    stop: Arc<AtomicBool>,
    is_paused: Arc<AtomicBool>,
    /// Сид в shared-владении: Flutter меняет его наживую,
    /// петля подхватывает изменение без перезапуска потока.
    seed: Arc<RwLock<u64>>,
    tempo: LiveTempo,
    /// Ручка intensity фронта (дефолт 1.0): читается каждый шаг.
    intensity: Arc<RwLock<f32>>,
    /// Сюда каждый шаг пишется эффективный BPM — движок строит
    /// от него время темпового эха.
    bpm_out: Arc<RwLock<f32>>,
    /// Сюда каждый шаг пишется открытость фильтра формы (0..1) —
    /// движок свистит срезом верхов к дропу.
    filter_open: Arc<RwLock<f32>>,
    /// Счетчик отданных в очередь нот (диагностика для UI).
    pushed: Arc<AtomicU64>,
    /// Ноты, не влезшие в очередь (генератор обгоняет движок).
    /// Ненулевой счетчик = ритм дырявит: пропадают доли.
    dropped: Arc<AtomicU64>,
    /// Живой снимок для визуалайзера (делится с сессией).
    visual: Arc<RwLock<VisualSnap>>,
    /// Флаг «оборвать старые голоса»: ставится при смене сида или стиля.
    voice_reset: Arc<AtomicBool>,
    // Состояние генерации (бывшие локальные переменные run):
    rng: StdRng,
    arranger: Arranger,
    song: SongContext,
    current_seed: u64,
    step_count: usize,
    last_style_name: String,
    /// Фраза, для которой собраны маски нот (монотонный индекс) и
    /// глобальный шаг ее старта. Маски — по настоящим нотам фразы:
    /// колесо показывает ровно то, что звучит.
    last_planned: usize,
    phrase_start: usize,
    /// Маски нот фразы по голосам: кик, перкуссия, бас, мелодия.
    plan: [u64; 4],
    /// Партитура стиля (нужна тени перехода).
    score: crate::arranger::Score,
    /// Аранж стиля: кто играет в каждой секции.
    layers: crate::piece::Layers,
    /// Как стиль склеивает куски.
    transition: crate::piece::Transition,
    /// Тень: голос следующего куска, который поет поверх грува текущего
    /// в шве. `None` — вне шва.
    shadow: Option<Shadow>,
    /// Отдельный RNG тени: общий поток сместил бы ноты основного.
    shadow_rng: StdRng,
    /// Момент старта текущего шага: точная фаза внутри шага
    /// для якоря колеса (лесенки опросов больше нет).
    step_t0: Instant,
    /// Последняя нота каждого слота: (высота, шаг, на котором она
    /// перестанет звучать). Нужна лигам: повтор той же высоты, пока
    /// нота еще звучит, продлевает ее, а не переатакует.
    last_note: [Option<(u8, u64)>; Slot::COUNT],
}

/// Лига возможна, только если пришла та же высота, предыдущая нота
/// еще звучит и детерминированный бросок ниже вероятности стиля.
/// Разные высоты — не лига: моно-голос сам сольет их легато.
fn should_tie(
    tie_repeats: f32,
    last: Option<(u8, u64)>,
    pitch: u8,
    due_now: u64,
    seed_hash: u64,
    slot_idx: usize,
) -> bool {
    let Some((prev_pitch, prev_end)) = last else {
        return false;
    };
    tie_repeats > 0.0
        && prev_pitch == pitch
        && prev_end > due_now
        && crate::styles::tie_chance(seed_hash, slot_idx, due_now as usize) < tie_repeats
}

impl Composer {
    pub fn new(
        queue: Arc<ArrayQueue<NoteEvent>>,
        style: Arc<RwLock<Box<dyn Style>>>,
        mixer: Arc<RwLock<MixerState>>,
        stop: Arc<AtomicBool>,
        is_paused: Arc<AtomicBool>,
        seed: u64,
    ) -> Self {
        Self::new_shared(
            queue,
            Arc::new(ArrayQueue::new(1024)),
            style,
            mixer,
            stop,
            is_paused,
            Arc::new(RwLock::new(seed)),
            Arc::new(RwLock::new(None)),
            Arc::new(RwLock::new(1.0)),
            Arc::new(RwLock::new(120.0)),
            Arc::new(RwLock::new(0.4)),
            Arc::new(AtomicU64::new(0)),
            Arc::new(AtomicU64::new(0)),
            Arc::new(RwLock::new(VisualSnap::default())),
            Arc::new(AtomicBool::new(false)),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_shared(
        queue: Arc<ArrayQueue<NoteEvent>>,
        marks: Arc<ArrayQueue<StepMark>>,
        style: Arc<RwLock<Box<dyn Style>>>,
        mixer: Arc<RwLock<MixerState>>,
        stop: Arc<AtomicBool>,
        is_paused: Arc<AtomicBool>,
        seed: Arc<RwLock<u64>>,
        tempo: LiveTempo,
        intensity: Arc<RwLock<f32>>,
        bpm_out: Arc<RwLock<f32>>,
        filter_open: Arc<RwLock<f32>>,
        pushed: Arc<AtomicU64>,
        dropped: Arc<AtomicU64>,
        visual: Arc<RwLock<VisualSnap>>,
        voice_reset: Arc<AtomicBool>,
    ) -> Self {
        let current_seed = snapshot(&seed);
        let (score, style_name, bpm, transition, layers) = match style.read() {
            Ok(g) => (
                g.arrangement_for_seed(current_seed),
                g.name().to_string(),
                g.bpm(),
                g.transition(),
                g.arrangement_layers(),
            ),
            Err(poisoned) => {
                let g = poisoned.into_inner();
                (
                    g.arrangement_for_seed(current_seed),
                    g.name().to_string(),
                    g.bpm(),
                    g.transition(),
                    g.arrangement_layers(),
                )
            }
        };
        // Первый кусок: форма конечна (30-60 с), дальше куски идут
        // подряд, и каждый следующий пишется заранее (стык написан).
        let plan =
            crate::piece::plan_piece(current_seed, bpm, score.cycle, 0, 0, transition, layers);
        let mut arranger = Arranger::with_plan(current_seed, bpm, score, plan, transition, layers);
        let song = arranger.advance_measure();
        Self {
            queue,
            marks,
            style,
            mixer,
            stop,
            is_paused,
            seed,
            tempo,
            intensity,
            bpm_out,
            filter_open,
            pushed,
            dropped,
            visual,
            voice_reset,
            rng: StdRng::seed_from_u64(current_seed),
            arranger,
            song,
            current_seed,
            step_count: 0,
            // Имя уже текущего стиля: первый шаг не должен приниматься
            // за «смену стиля» и сбрасывать только что созданную форму.
            last_style_name: style_name,
            last_planned: usize::MAX,
            phrase_start: 0,
            plan: [0u64; 4],
            score,
            layers,
            transition,
            shadow: None,
            shadow_rng: StdRng::seed_from_u64(current_seed ^ 0x5A4D),
            step_t0: Instant::now(),
            last_note: [None; Slot::COUNT],
        }
    }

    fn reset_form(&mut self, seed: u64) {
        // Старая генерация обрывается: голоса и хвост комнаты гаснут,
        // а не доигрывают поверх новой (слушатель сам сменил сид/стиль —
        // резкий обрыв уместен).
        self.voice_reset.store(true, Ordering::Relaxed);
        // Партитура — от текущего стиля и сида: каждый сид выбирает
        // свой вариант формы, а не только тему внутри одной сетки.
        let (score, bpm, transition, layers) = match self.style.read() {
            Ok(g) => (
                g.arrangement_for_seed(seed),
                g.bpm(),
                g.transition(),
                g.arrangement_layers(),
            ),
            Err(poisoned) => {
                let g = poisoned.into_inner();
                (
                    g.arrangement_for_seed(seed),
                    g.bpm(),
                    g.transition(),
                    g.arrangement_layers(),
                )
            }
        };
        // Живая смена вступает с следующего такта: остаток текущего куска
        // перепланируется под новый сид/стиль, отыгранное — не трогаем.
        let bar = self.arranger.piece_bar();
        let piece = self.arranger.piece();
        self.current_seed = seed;
        self.rng = StdRng::seed_from_u64(seed);
        self.score = score;
        self.transition = transition;
        self.layers = layers;
        // Тень прошлого шва не переносится в новую форму.
        self.shadow = None;
        let plan = crate::piece::plan_piece(seed, bpm, score.cycle, piece, bar, transition, layers);
        self.arranger = Arranger::with_plan(seed, bpm, score, plan, transition, layers);
        self.song = self.arranger.advance_measure();
        self.step_count = 0;
        self.last_planned = usize::MAX;
        // Голоса погашены движком: запомненные ноты больше не звучат
        // и не должны давать лигу через границу стиля/сида.
        self.last_note = [None; Slot::COUNT];
    }

    /// Один шаг генерации: ноты + длительность до следующего шага.
    /// Чистая механика без сна — используется и петлей, и офлайн-рендером.
    pub fn next_step(&mut self) -> (Vec<NoteEvent>, f32) {
        // Часы шага: снимок пишется в его начале — точная фаза
        // (доля внутри шага) едет колесу вместе с позицией.
        self.step_t0 = Instant::now();

        // Живая смена сида: пересоздаем RNG и форму,
        // поток и очередь при этом не трогаем.
        let live_seed = snapshot(&self.seed);
        if live_seed != self.current_seed {
            self.reset_form(live_seed);
        }

        // Имя стиля читаем коротким гардом, чтобы сброс формы
        // (&mut self) не конфликтовал с чтением стиля.
        let style_name = match self.style.read() {
            Ok(g) => g.name().to_string(),
            Err(poisoned) => poisoned.into_inner().name().to_string(),
        };
        if self.last_style_name != style_name {
            // Смена стиля начинает новую форму с чистого листа.
            self.reset_form(self.current_seed);
            self.last_style_name = style_name;
        }

        // Микс читаем после сбросов: живой seed/style мог сменить
        // каст, и старый срез активных треков не должен попасть в
        // первый шаг новой формы.
        let mut active_tracks = [0usize; MAX_ACTIVE_TRACKS];
        let active_count = match self.mixer.read() {
            Ok(state) => state.active_ids(&mut active_tracks),
            Err(poisoned) => poisoned.into_inner().active_ids(&mut active_tracks),
        };
        // active_ids уже сортирует — порядок детерминирован.
        // Треки с громкостью 0 сюда не попадают: выключенные слоты
        // не грузят ни генерацию, ни DSP.
        let active = &active_tracks[..active_count];

        let guard = match self.style.read() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        let style = &**guard;

        let steps_per_measure = style.steps_per_measure();
        // Эффективный темп: ручка фронта перекрывает дефолт стиля.
        // Валидация и clamp — тот же путь, что у UI и статуса
        // (core::normalize_tempo): наружу не уезжает незвучащий BPM.
        let bpm = snapshot(&self.tempo)
            .and_then(normalize_tempo)
            .or_else(|| normalize_tempo(style.bpm()))
            .unwrap_or_else(|| style.bpm());
        if let Ok(mut share) = self.bpm_out.write() {
            *share = bpm;
        }
        let base_step_secs = base_step_secs(bpm, steps_per_measure);
        let step_in_measure = self.step_count % steps_per_measure;
        // Один снимок на шаг: план и живой рендер видят одну интенсивность.
        let intensity = snapshot(&self.intensity).clamp(0.0, 2.0);

        if step_in_measure == 0 && self.step_count > 0 {
            self.song = self.arranger.advance_measure();
            // Тень (следующий кусок) идет в ногу с основным: те же такты.
            if let Some(shadow) = self.shadow.as_mut() {
                shadow.song = shadow.arranger.advance_measure();
            }
        }
        // Открытость фильтра — от свежей секции: движок свистит
        // срезом верхов к дропу каждый шаг.
        if let Ok(mut share) = self.filter_open.write() {
            *share = crate::arranger::section_openness(self.song.section, self.song.progress);
        }

        // Тень перехода: в шве (финале куска) голос следующего куска
        // входит поверх грува текущего — стык не режет фразу. Тень
        // строится, когда шов начался (план следующего куска к этому
        // моменту уже написан), и поет только мелодические группы:
        // грув остается один.
        if self.shadow.is_none() && self.arranger.in_joint() {
            if let (Some(plan), crate::piece::Transition::Rolling { .. }) =
                (self.arranger.next_plan().cloned(), self.transition)
            {
                let mut shadow = self.arranger.shadow_of(plan);
                let song = shadow.build_context();
                self.shadow = Some(Shadow {
                    arranger: shadow,
                    song,
                });
                self.shadow_rng = StdRng::seed_from_u64(self.current_seed ^ 0x5A4D);
            }
        } else if !self.arranger.in_joint() {
            // Шов кончился: голос следующего куска теперь поет сам
            // арранжер — тень уходит, чтобы не было двух голосов.
            self.shadow = None;
        }

        // Окно колеса: последние 64 шага (4 такта) настоящих нот.
        // Точки на колесе — ровно то, что звучит: отдельного пре-рендера
        // больше нет, поэтому показа и звука разойтись не может.
        // Окно шире фразы (32 шага) специально: иначе на границе фразы
        // маски обнулялись — и радар очищался как раз в тот момент, когда
        // стрелка приходит в верхнюю точку.
        let window_pos = self.step_count % WHEEL_WINDOW_STEPS;
        if window_pos == 0 {
            self.plan = [0u64; 4];
        }

        let mut sctx = StepContext {
            step: self.step_count,
            step_in_measure,
            base_step_secs,
            active_tracks: active,
            song: &self.song,
            rng: &mut self.rng,
            intensity,
        };
        let mut notes = render_style(&**guard, &mut sctx);

        // Лиги на моно-слоях: повтор той же высоты, пришедший пока нота
        // еще звучит, не переатакует, а продлевает ее. Это делает длину
        // ноты слышимой: иначе следующий онсет режет бас до шага.
        // Полифония накладывает голоса сама — лига нужна только басу.
        let due_now = self.step_count as u64;
        let guides = style.slot_guides();
        for note in notes.iter_mut() {
            let Some(slot) = Slot::from_id(note.instrument_id) else {
                continue;
            };
            let Some(guide) = guides.get(slot.index()) else {
                continue;
            };
            let gate = (note.duration_secs / base_step_secs).round().max(1.0) as u64;
            let mut end = due_now + gate;
            let prev = self.last_note[slot.index()];
            if slot == Slot::Bass
                && should_tie(
                    guide.tie_repeats,
                    prev,
                    note.midi_pitch,
                    due_now,
                    self.song.seed_hash,
                    slot.index(),
                )
            {
                note.tie = true;
                if let Some((_, prev_end)) = prev {
                    end = end.max(prev_end);
                }
            }
            self.last_note[slot.index()] = Some((note.midi_pitch, end));
        }

        // Тень перехода: голос следующего куска входит поверх грува
        // текущего. Только мелодические группы (лид, вокал): грув и
        // пампа остаются одни, иначе шов превратится в двух голосов.
        // Громкость тени растёт к концу шва — вход слышен как событие,
        // а не как фон из ниоткуда. Растёт по шагам, а не по тактам:
        // голос поднимается плавно, без ступенек.
        let shadow_gain = self
            .arranger
            .joint_step_progress(steps_per_measure, step_in_measure)
            .powf(1.2)
            .clamp(0.0, 1.0);
        if shadow_gain > 0.0 {
            if let Some(shadow) = self.shadow.as_mut() {
                let mut shctx = StepContext {
                    step: self.step_count,
                    step_in_measure,
                    base_step_secs,
                    active_tracks: active,
                    song: &shadow.song,
                    rng: &mut self.shadow_rng,
                    intensity,
                };
                for mut n in render_style(&**guard, &mut shctx) {
                    let melodic = matches!(
                        Slot::from_id(n.instrument_id).map(Slot::role),
                        Some(InstrumentRole::Lead) | Some(InstrumentRole::Vocal)
                    );
                    if !melodic {
                        continue;
                    }
                    n.velocity *= shadow_gain;
                    notes.push(n);
                }
            }
        }

        // Свинг сида поверх стиля: у каждого сида свой кач.
        // Считаем ДО снимка: длительность нужна и штампам, и точной
        // фазе фразы для колеса.
        let swing = style.swing_ratio() * crate::styles::swing_var(self.song.seed_hash);
        let duration = swing_step_secs(base_step_secs, swing, step_in_measure);
        // Точная фаза фразы: целые шаги + доля текущего по часам.
        let phrase_len = (2 * steps_per_measure).min(32);
        let phrase_pos = self.step_count.saturating_sub(self.phrase_start).min(31);
        let step_frac = (Instant::now()
            .saturating_duration_since(self.step_t0)
            .as_secs_f32()
            / duration.max(0.001))
        .clamp(0.0, 1.0);
        let phrase_phase = (phrase_pos as f32 + step_frac) / phrase_len.max(1) as f32;
        // Фаза окна колеса: стрелка и «возраст» точки считаются по ней.
        let window_phase = (window_pos as f32 + step_frac) / WHEEL_WINDOW_STEPS as f32;

        // Маски окна — по настоящим нотам (включая голос тени): колесо
        // и звук берут один и тот же источник, поэтому разойтись не могут.
        if !notes.is_empty() {
            let bit = 1u64 << window_pos.min(WHEEL_WINDOW_STEPS - 1);
            for n in notes.iter() {
                match Slot::from_id(n.instrument_id).map(Slot::role) {
                    Some(InstrumentRole::Kick) => self.plan[0] |= bit,
                    Some(InstrumentRole::Snare | InstrumentRole::Hat) => self.plan[1] |= bit,
                    Some(InstrumentRole::Bass) => self.plan[2] |= bit,
                    Some(InstrumentRole::Lead | InstrumentRole::Vocal | InstrumentRole::Chords) => {
                        self.plan[3] |= bit
                    }
                    _ => {}
                }
            }
        }
        // Живой снимок для визуалайзера: доля такта + маски звучавших
        // долей по главным голосам (бит = 16-я доля, сброс на границе
        // такта) + план фразы (оборот руки — фраза, точки стоят сразу).
        // Позиция темы — та же формула, что в play_guide: первая
        // половина мотива в первом такте фразы, вторая — во втором.
        if let Ok(mut snap) = self.visual.write() {
            let motif_len = self.song.motif.len().max(1);
            let bar_in_phrase = self.song.measure_index.saturating_sub(1) % 2;
            let bar_span = (steps_per_measure / 2).max(1);
            let rot = self.song.stage % motif_len;
            let pos = (bar_in_phrase * bar_span + step_in_measure / 2 + rot) % motif_len;
            let active_motif = if self.song.use_alt_motif {
                self.song.motif_b
            } else {
                self.song.motif
            };
            let mut hits = snap.bar_hits;
            let mut kick = snap.kick_hits;
            let mut snare = snap.snare_hits;
            let mut bass = snap.bass_hits;
            let mut theme = snap.theme_hits;
            if step_in_measure == 0 {
                hits = 0;
                kick = 0;
                snare = 0;
                bass = 0;
                theme = 0;
            }
            if !notes.is_empty() {
                let bit = 1u16 << step_in_measure.min(15);
                hits |= bit;
                for n in notes.iter() {
                    match Slot::from_id(n.instrument_id).map(Slot::role) {
                        Some(InstrumentRole::Kick) => kick |= bit,
                        Some(InstrumentRole::Snare | InstrumentRole::Hat) => snare |= bit,
                        Some(InstrumentRole::Bass) => bass |= bit,
                        Some(
                            InstrumentRole::Lead | InstrumentRole::Vocal | InstrumentRole::Chords,
                        ) => theme |= bit,
                        _ => {}
                    }
                }
            }
            // Сыграно фразы: бит ставится, когда шаг отзвучал.
            // Сброс на старте фразы.
            let mut played = snap.pplayed;
            if phrase_pos == 0 {
                played = 0;
            }
            if !notes.is_empty() {
                played |= 1u32 << phrase_pos.min(31);
            }
            *snap = VisualSnap {
                section: section_index(self.song.section),
                energy: self.song.energy,
                motif: active_motif,
                motif_pos: pos as u8,
                step_phase: step_in_measure as f32 / steps_per_measure.max(1) as f32,
                bar_hits: hits,
                kick_hits: kick,
                snare_hits: snare,
                bass_hits: bass,
                theme_hits: theme,
                phrase_len: phrase_len as u8,
                phrase_pos: phrase_pos as u8,
                phrase_phase,
                window_phase,
                pkick: self.plan[0],
                psnare: self.plan[1],
                pbass: self.plan[2],
                ptheme: self.plan[3],
                pplayed: played,
            };
        }

        self.step_count += 1;
        // Due-штамп: нота знает свой шаг и его длительность —
        // насос выстрелит точно в медиакадр, когда бы ни разобрал
        // очередь (фронт больше не владеет таймингом).
        let step_us = (duration.max(0.001) * 1_000_000.0) as u32;
        let due_step = self.step_count as u64 - 1;
        for note in notes.iter_mut() {
            note.due_step = due_step;
            note.step_us = step_us;
        }
        (notes, duration)
    }

    pub fn run(mut self) {
        let mut next_deadline = Instant::now();

        while !self.stop.load(Ordering::Relaxed) {
            if self.is_paused.load(Ordering::Relaxed) {
                thread::sleep(Duration::from_millis(100));
                // Сдвигаем дедлайн, чтобы после паузы не было "догоняющей" очереди шагов.
                next_deadline = Instant::now();
                continue;
            }

            let (notes, step_duration) = self.next_step();
            // Метка шага — всегда, даже если на шаге нет нот: движок по
            // ней знает честную длину шага и строит точную сетку времени.
            // Без меток сетка строится по нотам, пропуски оцениваются
            // линейно, и тайминг уплывает (ритм сбивается).
            let step_us = (step_duration.max(0.001) * 1_000_000.0) as u32;
            let _ = self.marks.push(StepMark {
                step: self.step_count.saturating_sub(1) as u64,
                us: step_us,
            });
            // Полная очередь: дропаем новейшую ноту, старые не трогаем.
            // Считаем отдельно: молчаливый дроп — это «ритм сбился»,
            // а по счетчику видно, что именно генератор не успевает.
            let mut ok = 0u64;
            let mut lost = 0u64;
            for note in notes {
                if self.queue.push(note).is_ok() {
                    ok += 1;
                } else {
                    lost += 1;
                }
            }
            if ok > 0 {
                self.pushed.fetch_add(ok, Ordering::Relaxed);
            }
            if lost > 0 {
                self.dropped.fetch_add(lost, Ordering::Relaxed);
            }

            // Шагомер без дрейфа: спим до дедлайна, отставание не накапливаем.
            // Кламп защищает от паники Duration::from_secs_f32 при кривом
            // кастомном стиле (NaN/Inf/отрицательная длительность).
            let safe_step = step_duration.clamp(0.001, 4.0);
            next_deadline += Duration::from_secs_f32(safe_step);
            let now = Instant::now();
            if next_deadline > now {
                thread::sleep(next_deadline - now);
            } else {
                next_deadline = now;
            }
        }
    }
}

/// Совместимая обертка над [`Composer::run`].
pub fn start_composing(
    queue: Arc<ArrayQueue<NoteEvent>>,
    style_lock: Arc<RwLock<Box<dyn Style>>>,
    mixer: Arc<RwLock<MixerState>>,
    stop: Arc<AtomicBool>,
    is_paused: Arc<AtomicBool>,
    seed: u64,
) {
    Composer::new(queue, style_lock, mixer, stop, is_paused, seed).run();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instruments::Slot;
    use crate::mixer::MixerState;

    #[test]
    fn deep_bass_lengths_vary() {
        // Интеграция длин: композитор реально отдает басу разные
        // длительности, а не одну константу тембра.
        let style = Arc::new(RwLock::new(
            crate::styles::make_style("deep").expect("deep style"),
        ));
        let mixer = Arc::new(RwLock::new(MixerState::new()));
        {
            let guard = style.read().expect("style readable");
            let mut m = mixer.write().expect("mixer writable");
            crate::playback::apply_style_mix(&**guard, 424242, &mut m);
        }
        let queue = Arc::new(ArrayQueue::new(16_384));
        let mut composer = Composer::new(
            queue,
            style,
            mixer,
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
            424242,
        );
        let mut bass = 0usize;
        let mut lengths = std::collections::HashSet::new();
        for _ in 0..1600 {
            let (notes, _) = composer.next_step();
            for n in notes {
                if n.instrument_id == Slot::Bass.id() {
                    bass += 1;
                    lengths.insert((n.duration_secs * 1000.0).round() as i32);
                }
            }
        }
        assert!(bass > 100, "too few bass notes: {bass}");
        assert!(lengths.len() >= 3, "bass lengths did not vary: {lengths:?}");
    }

    #[test]
    fn tie_decision_needs_same_pitch_and_overlap() {
        let due = 100u64;
        assert!(should_tie(1.0, Some((40, 104)), 40, due, 7, 0));
        assert!(
            !should_tie(1.0, Some((41, 104)), 40, due, 7, 0),
            "other pitch is a legato, not a tie"
        );
        assert!(
            !should_tie(1.0, Some((40, 100)), 40, due, 7, 0),
            "previous note already ended"
        );
        assert!(
            !should_tie(0.0, Some((40, 104)), 40, due, 7, 0),
            "style forbids ties"
        );
        assert!(!should_tie(1.0, None, 40, due, 7, 0), "no previous note");
    }
}
