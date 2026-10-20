//! Аудиодвижок: drain очереди нот -> триггер голосов -> сайдчейн ->
//! стерео-микс -> пространство -> мастер.
//!
//! Современный клубный тракт в миниатюре:
//! - сайдчейн-памп: огибающая кика пригибает бас/пэды (глубина и кач);
//! - стерео: низ по центру, полифония разложена по сторонам пулами;
//! - мастер-пространство (темный реверб) + мягкий клиппер вместо clamp.

use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex, RwLock,
};
use std::time::{Duration, Instant};

use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    SampleFormat,
};
use crossbeam::queue::ArrayQueue;

use crate::core::{snapshot, MASTER_DRIVE, MAX_ACTIVE_TRACKS};
use crate::instruments::dsp::MasterLimiter;
use crate::instruments::{dsp::Svf, Instrument, InstrumentRole, NoteEvent};
use crate::mixer::MixerState;
use crate::playback::EngineCounters;
use crate::space::SpaceBus;

/// Затухание огибающей кика: восстановление ~180 мс (памп в темпе хауса).
const DUCK_RELEASE_SECS: f32 = 0.18;
/// Масштаб огибающей к диапазону 0..1 (пик кика после громкости ~0.8).
const DUCK_ENV_SCALE: f32 = 1.25;

/// Мастер-эффекты от стиля: комната реверба и срез верхов.
/// Контроллер перезаписывает при смене стиля, движок читает каждый блок.
#[derive(Debug, Clone, Copy)]
pub struct MasterFx {
    pub reverb_wet: f32,
    pub reverb_size: f32,
    pub treble_hz: f32,
    /// Множитель глубины сайдчейна (1.0 — характер движка).
    pub sidechain: f32,
}

impl Default for MasterFx {
    fn default() -> Self {
        Self {
            reverb_wet: 0.16,
            reverb_size: 0.5,
            treble_hz: 19000.0,
            sidechain: 1.0,
        }
    }
}

pub type SharedFx = Arc<RwLock<MasterFx>>;

/// Состояние рендера без cpal: голоса, сайдчейн, комната, верх.
/// Темпового дилэя в тракте нет (выключен: ситуативен, мылит
/// транзиенты; сам `Echo` живёт в space.rs и вернётся отдельной
/// фичей с посылами из микшера, а не захардкоженными по ролям).
/// Живой движок и офлайн-прогон (soak) используют один и тот же код —
/// расхождений между «слышу» и «проверяю» нет по построению.
pub(crate) struct RenderState {
    instruments: Vec<Box<dyn Instrument>>,
    index_by_id: HashMap<usize, usize>,
    is_kick: Vec<bool>,
    space: SpaceBus,
    treble_l: Svf,
    treble_r: Svf,
    kick_env: f32,
    duck_release: f32,
    /// Быстрая атака огибающей (2 мс): памп успевает за киком,
    /// но не трещит широкополосным хэшем по фронту удара.
    duck_attack: f32,
    duck_mult: f32,
    /// Мастер-HP 28 Гц: режет инфраниз и постоянку (хрип динамиков
    /// телефона), музыкального контента не трогает (минимум 32 Гц).
    hp_l: Svf,
    hp_r: Svf,
    sample_rate: f32,
    /// Сглаженный срез верхов: цель прыгает на границах секций
    /// (свип формы), фильтр едет за ней ~120 мс вместо ступеньки —
    /// ступенька коэффициента SVF и была редким «всхлипом».
    treble_smooth: f32,
    /// Пиковый лимитер мастера: мейкап + потолок после всей цепочки.
    limiter: MasterLimiter,
    /// Коэффициент сглаживания среза верхов (~120 мс): константа
    /// частоты дискретизации, считается в new/retune, а не на сэмпл.
    smooth_k: f32,
}

impl RenderState {
    pub(crate) fn new(mut instruments: Vec<Box<dyn Instrument>>, sample_rate: f32) -> Self {
        let sample_rate = if sample_rate.is_finite() && sample_rate > 0.0 {
            sample_rate
        } else {
            44_100.0
        };
        for inst in &mut instruments {
            inst.set_sample_rate(sample_rate);
        }
        let index_by_id = instruments
            .iter()
            .enumerate()
            .map(|(pos, inst)| (inst.id(), pos))
            .collect();
        // Маска киков для сайдчейна: их рендерим первыми в каждом семпле.
        let is_kick: Vec<bool> = instruments
            .iter()
            .map(|inst| inst.role() == InstrumentRole::Kick)
            .collect();
        Self {
            instruments,
            index_by_id,
            is_kick,
            space: SpaceBus::new(sample_rate),
            treble_l: Svf::default(),
            treble_r: Svf::default(),
            kick_env: 0.0,
            duck_release: (-1.0 / (sample_rate * DUCK_RELEASE_SECS)).exp(),
            duck_attack: (-1.0 / (sample_rate * 0.002)).exp(),
            duck_mult: 1.0,
            hp_l: Svf::default(),
            hp_r: Svf::default(),
            sample_rate,
            treble_smooth: 0.0,
            limiter: MasterLimiter::default(),
            smooth_k: 1.0 - (-1.0 / (sample_rate * 0.12)).exp(),
        }
    }

    /// Перенастройка на новую частоту (смена маршрута вывода):
    /// голоса и их состояние сохраняются, сбрасываются только
    /// частотно-зависимые узлы (комната, верха, сайдчейн).
    pub(crate) fn retune(&mut self, sample_rate: f32) {
        let sr = if sample_rate.is_finite() && sample_rate > 0.0 {
            sample_rate
        } else {
            44_100.0
        };
        for inst in &mut self.instruments {
            inst.set_sample_rate(sr);
        }
        self.space = SpaceBus::new(sr);
        self.treble_l = Svf::default();
        self.treble_r = Svf::default();
        self.treble_smooth = 0.0;
        self.hp_l = Svf::default();
        self.hp_r = Svf::default();
        self.duck_release = (-1.0 / (sr * DUCK_RELEASE_SECS)).exp();
        self.duck_attack = (-1.0 / (sr * 0.002)).exp();
        self.smooth_k = 1.0 - (-1.0 / (sr * 0.12)).exp();
        self.sample_rate = sr;
    }

    pub(crate) fn push_note(&mut self, note: &NoteEvent) {
        if let Some(&pos) = self.index_by_id.get(&note.instrument_id) {
            if let Some(inst) = self.instruments.get_mut(pos) {
                inst.trigger(note);
            }
        }
    }

    pub(crate) fn set_room(&mut self, wet: f32, size: f32) {
        self.space.set_room(wet, size);
    }

    pub(crate) fn set_duck_mult(&mut self, mult: f32) {
        self.duck_mult = mult.clamp(0.0, 2.0);
    }

    pub(crate) fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    fn volume_of(volumes: &[(usize, f32)], id: usize) -> f32 {
        for (vid, vol) in volumes {
            if *vid == id {
                return *vol;
            }
        }
        0.0
    }

    /// Один стерео-сэмпл: сайдчейн → микс → эхо → комната → верх → мастер.
    /// `volumes` — эффективные громкости (id, vol); нулевые затихшие
    /// голоса пропускаются без DSP.
    pub(crate) fn render_sample(&mut self, volumes: &[(usize, f32)], treble_hz: f32) -> (f32, f32) {
        // Проход 1: кики. Их уровень заряжает огибающую сайдчейна.
        // Немой затихший кик пропускаем (как остальных во 2-м проходе).
        let mut kick_l = 0.0;
        let mut kick_r = 0.0;
        for (pos, inst) in self.instruments.iter_mut().enumerate() {
            if !self.is_kick[pos] {
                continue;
            }
            let vol = Self::volume_of(volumes, inst.id());
            if vol <= 0.0 && !inst.is_active() {
                continue;
            }
            let (l, r) = inst.next_sample_stereo();
            kick_l += l * vol;
            kick_r += r * vol;
        }
        let kick_abs = (kick_l + kick_r) * 0.5;
        // Атака сглажена (2 мс): фронт удара не плодит широкополосный
        // хэш в огибающей, грув при этом не страдает — спад тот же.
        let a = if kick_abs.abs() > self.kick_env {
            self.duck_attack
        } else {
            self.duck_release
        };
        self.kick_env = kick_abs.abs() + (self.kick_env - kick_abs.abs()) * a;
        if self.kick_env < 1e-12 {
            self.kick_env = 0.0;
        }

        // Проход 2: остальные голоса с пригибанием по ролям.
        // Немой и полностью затихший голос пропускаем вообще:
        // исключенный слот не тратит DSP. Замьюченный, но еще
        // звучащий хвост продвигаем без вклада, чтобы включение
        // не выстрелило протухшим звуком.
        let mut mix_l = kick_l;
        let mut mix_r = kick_r;
        for (pos, inst) in self.instruments.iter_mut().enumerate() {
            if self.is_kick[pos] {
                continue;
            }
            let vol = Self::volume_of(volumes, inst.id());
            if vol > 0.0 {
                let duck = duck_gain(inst.role(), self.kick_env, self.duck_mult);
                let (l, r) = inst.next_sample_stereo();
                mix_l += l * vol * duck;
                mix_r += r * vol * duck;
            } else if inst.is_active() {
                let _ = inst.next_sample_stereo();
            }
        }

        // Чистый микс — сразу в комнату, без дилэя-прослойки.
        let (wet_l, wet_r) = self.space.process(mix_l, mix_r);
        // Срез верхов от стиля: lofi мутный, club хирургический.
        // Цель сглажена: прыжок на границе секции (свип формы) едет
        // ~120 мс, а не ступенькой в коэффициент фильтра.
        let target = treble_hz.clamp(2000.0, 20000.0);
        if self.treble_smooth <= 0.0 {
            self.treble_smooth = target;
        }
        self.treble_smooth += (target - self.treble_smooth) * self.smooth_k;
        let treble = self.treble_smooth;
        let sr = self.sample_rate;
        let (t_l, _, _) = self.treble_l.process(wet_l, treble, 0.7, sr);
        let (t_r, _, _) = self.treble_r.process(wet_r, treble, 0.7, sr);
        // Мастер-HP: инфраниз и постоянка вон (хрип динамиков),
        // музыка начинается с 32 Гц (808) — не задето.
        let (_, _, h_l) = self.hp_l.process(t_l, 28.0, 0.7, sr);
        let (_, _, h_r) = self.hp_r.process(t_r, 28.0, 0.7, sr);
        // Лимитер последним: мейкап тела + потолок пиков, затем tanh.
        let (l_l, l_r) = self.limiter.process(h_l, h_r, sr);
        (master_sample(l_l), master_sample(l_r))
    }
}

/// Насос выходного потока: блоковая подготовка (ноты из очереди,
/// эффективные громкости, живые FX) + посэмпловый рендер с фейдом паузы.
///
/// Один код для всех форматов сэмплов: часть Android-девайсов отдает
/// только I16/U16, и мономорфный F32-поток там не строился (тишина
/// без видимой ошибки). Конвертация — в тонких замыканиях `run`.
///
/// Потолок разбора очереди за подготовку блока (см. begin_block).
const MAX_DRAIN_PER_BLOCK: u64 = 32;
/// Потолок выстрелов due-нот за один кадр (см. next_frame): аккордовые
/// стеки (3–4 ноты с одним due) проходят целиком, пачка после stall'а
/// растягивается по кадрам вместо удара в один сэмпл.
const MAX_FIRES_PER_FRAME: usize = 8;
/// Давность due-расписания, после которой нота считается протухшей
/// и дропается (см. next_frame). 250 мс: дольше 16-й при 60 BPM —
/// играть её уже значит врать ритм.
const MAX_LATE_SECS: f32 = 0.25;
pub(crate) struct Pump {
    queue: Arc<ArrayQueue<NoteEvent>>,
    /// Метки шагов (честная длина каждого шага): по ним сетка времени.
    marks: Arc<ArrayQueue<crate::composer::StepMark>>,
    mixer: Arc<RwLock<MixerState>>,
    is_paused: Arc<AtomicBool>,
    fx: SharedFx,
    bpm: Arc<RwLock<f32>>,
    state: RenderState,
    volumes: [(usize, f32); MAX_ACTIVE_TRACKS],
    volume_count: usize,
    treble: f32,
    pause_gain: f32,
    pause_step: f32,
    counters: EngineCounters,
    logged_first: bool,
    filter_open: Arc<RwLock<f32>>,
    /// Отложенные ноты (due-кадр, порядок, нота), по возрастанию due.
    /// Насос стреляет нотой точно в ее медиакадр, а не когда разобрал
    /// очередь: таймингом владеет бэкенд, фронт — только байты.
    /// Порядок внутри одного кадра — порядок генерации (кик первым):
    /// движок грузит голоса по порядку, и сортировка по id инструмента
    /// этот порядок ломала. Емкость зарезервирована заранее: аудиоколбэк
    /// не аллоцирует.
    pending: Vec<(u64, u64, NoteEvent)>,
    /// Счетчик порядка: монотонный, чтобы равные due сохраняли порядок
    /// появления (сортировка нестабильная, порядок не гарантирован).
    pending_seq: u64,
    /// Сетка построена по меткам шагов (точный тайминг). Тогда ноты
    /// в сетку не пишутся: метка уже дала длину этого шага.
    marks_used: bool,
    /// Диагностика тайминга: ноты, выстрелившие заметно позже своей
    /// доли (ритм спотыкается), и пере-якорения сетки (генератор
    /// отстал — расписание пересобралось на текущем кадре).
    late_notes: u64,
    reanchors: u64,
    /// Ноты текущего блока до слияния с `pending`: переиспользуемый
    /// буфер, чтобы `begin_block` не аллоцировал в аудио-пути.
    fresh: Vec<(u64, u64, NoteEvent)>,
    /// Медиачасы: отрендеренные кадры. Монотонны, дрейфа нет.
    media_cursor: u64,
    /// Якорь сетки: какому шагу соответствует какой кадр.
    /// Ставится по первой штампованной ноте сухого потока;
    /// пока поток сух — следующая нота стартует «сейчас».
    anchor_step: u64,
    anchor_frame: u64,
    anchored: bool,
    /// Карта длин шагов (шаг → кадры): свинг делает шаги неравными,
    /// линейная формула от якоря врала тем сильнее, чем дальше нота.
    /// Суммируем истинные длины от якоря — сетка точна всегда,
    /// ре-якорь нужен только на реальных срывах, а не каждый свинг.
    /// Кольцо последних 256 шагов; вытесненное сворачиваем в якорь.
    step_grid: std::collections::VecDeque<(u64, u64)>,
}

impl Pump {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        queue: Arc<ArrayQueue<NoteEvent>>,
        marks: Arc<ArrayQueue<crate::composer::StepMark>>,
        instruments: Vec<Box<dyn Instrument>>,
        mixer: Arc<RwLock<MixerState>>,
        is_paused: Arc<AtomicBool>,
        fx: SharedFx,
        bpm: Arc<RwLock<f32>>,
        sample_rate: f32,
        counters: EngineCounters,
        filter_open: Arc<RwLock<f32>>,
    ) -> Self {
        // Старт gain от фактической паузы: пересоздание потока в паузе
        // не должно выстреливать 10 мс громкого звука.
        let paused_now = is_paused.load(Ordering::Relaxed);
        Self {
            queue,
            marks,
            mixer,
            is_paused,
            fx,
            bpm,
            state: RenderState::new(instruments, sample_rate),
            volumes: [(0usize, 0.0f32); MAX_ACTIVE_TRACKS],
            volume_count: 0,
            treble: MasterFx::default().treble_hz,
            // Гейн паузы: переход play/pause идет 10-миллисекундным
            // фейдом, а не рубкой в ноль (та рубка и была щелчком).
            pause_gain: if paused_now { 0.0 } else { 1.0 },
            pause_step: 1.0 / (0.01 * sample_rate),
            counters,
            logged_first: false,
            filter_open,
            pending: Vec::with_capacity(256),
            pending_seq: 0,
            marks_used: false,
            late_notes: 0,
            reanchors: 0,
            fresh: Vec::with_capacity(MAX_DRAIN_PER_BLOCK as usize),
            media_cursor: 0,
            anchor_step: 0,
            anchor_frame: 0,
            anchored: false,
            step_grid: std::collections::VecDeque::with_capacity(257),
        }
    }

    /// Перенастройка насоса на новую частоту вывода (реконнект):
    /// состояние микса сохраняется, гейны не рвутся.
    pub(crate) fn retune(&mut self, sample_rate: f32) {
        self.state.retune(sample_rate);
        self.pause_step = 1.0 / (0.01 * self.state.sample_rate);
    }

    pub(crate) fn sample_rate(&self) -> f32 {
        self.state.sample_rate()
    }

    /// Длина due-расписания (нот ждет свой медиакадр).
    pub(crate) fn pending_len(&self) -> usize {
        self.pending.len()
    }

    /// Подготовка блока: ноты, громкости, живые FX и время эха.
    fn begin_block(&mut self) {
        // Накопленные счетчики диагностики тайминга — в общие (раз
        // в блок, а не на каждую ноту: аудиопоток не должен трогать
        // атомики в горячем цикле).
        if self.late_notes > 0 {
            self.counters
                .late
                .fetch_add(self.late_notes, Ordering::Relaxed);
            self.late_notes = 0;
        }
        if self.reanchors > 0 {
            self.counters
                .reanchors
                .fetch_add(self.reanchors, Ordering::Relaxed);
            self.reanchors = 0;
        }
        // try_read, не блокирующий lock: аудиопоток не ждет жест UI.
        // Свежие громкости подхватит следующий блок (~10 мс) — на слух
        // мгновенно, зато редкая запись микшера не рвет DSP.
        if let Ok(state) = self.mixer.try_read() {
            // Эффективные громкости через volume_for, чтобы логика
            // mute жила в одном месте (MixerState).
            self.volume_count = 0;
            for id in state.tracks.keys() {
                if self.volume_count < MAX_ACTIVE_TRACKS {
                    self.volumes[self.volume_count] = (*id, state.volume_for(*id));
                    self.volume_count += 1;
                }
            }
        }
        let mut eaten = 0u64;
        // Метки шагов идут первыми: по ним сетка времени точная (каждый
        // шаг, а не только шаги с нотами) и по ним же ставится привязка:
        // «шаг начался сейчас» — это текущий медиакадр, а не кадр первой
        // ноты (она может прийти на шаг позже).
        let mut marks_read = 0u64;
        while let Some(mark) = self.marks.pop() {
            marks_read += 1;
            self.marks_used = true;
            let frames = (mark.us as u64 * self.state.sample_rate() as u64 / 1_000_000).max(1);
            if self.step_grid.back().map(|(s, _)| *s) == Some(mark.step) {
                continue;
            }
            // Сетка от новой шкалы: старую чистим целиком (живой сид/стиль
            // начинает нумерацию заново — иначе длины уедут вперёд).
            if mark.step < self.anchor_step
                || self
                    .step_grid
                    .back()
                    .map(|(s, _)| mark.step < *s)
                    .unwrap_or(false)
            {
                self.step_grid.clear();
                self.anchored = false;
            }
            if !self.anchored {
                // Привязка по метке: шаг начался на этом кадре.
                self.anchor_step = mark.step;
                self.anchor_frame = self.media_cursor;
                self.anchored = true;
            }
            self.step_grid.push_back((mark.step, frames));
            // Вытеснение с fold: старое сворачиваем в якорь,
            // сетка от него остается точной.
            while self.step_grid.len() > 256 {
                if let Some((s, f)) = self.step_grid.pop_front() {
                    if s >= self.anchor_step {
                        self.anchor_frame = self.anchor_frame.saturating_add(f);
                        self.anchor_step = s + 1;
                    }
                } else {
                    break;
                }
            }
        }
        let _ = marks_read;
        // Потолок разбора за вызов: backlog после stall'а раскладывается
        // по субблокам (~12мс друг от друга — почти вовремя), а не
        // выстреливает разом. Живой поток (<32 нот на 12мс невозможны
        // музыкально) потолок не замечает.
        // Штампованные ноты уходят в due-расписание (точное время),
        // legacy без штампа (тесты, прямые вызовы) — сразу в голоса.
        // Буфер `fresh` переиспользуется: никаких аллокаций в блоке.
        self.fresh.clear();
        while eaten < MAX_DRAIN_PER_BLOCK {
            let Some(note) = self.queue.pop() else {
                break;
            };
            if note.step_us == 0 {
                self.state.push_note(&note);
            } else {
                let step_frames = note.step_us as u64 * self.state.sample_rate() as u64 / 1_000_000;
                // Сброс нумерации шагов: живой сид/стиль заново начинает
                // форму с due_step 0. Обычный поток монотонен (аккорды
                // одного шага — дубликаты), поэтому шаг позади последнего
                // в сетке — именно новая шкала, а не опоздавшая нота.
                // Сетку старой шкалы нельзя сворачивать в якорь: её длины
                // уедут в расписание на десятки секунд вперед (тишина
                // после смены сида), поэтому чистим её целиком.
                // Когда сетка идет по меткам, сброс ловим там же: метки
                // приходят пачками впереди нот, и «шаг позади хвоста» —
                // норма, а не сброс шкалы.
                let numbering_reset = !self.marks_used
                    && (note.due_step < self.anchor_step
                        || self
                            .step_grid
                            .back()
                            .map(|(s, _)| note.due_step < *s)
                            .unwrap_or(false));
                if numbering_reset {
                    self.step_grid.clear();
                }
                // Карта длин: шаг уже видели — длина та же, дубликат
                // пропускаем; новый — кладем в хвост (шаги идут по порядку).
                // Если сетка идет по меткам, ноты в нее не пишем: длину
                // шага уже дала метка (иначе шаг задваивался, и тайминг
                // плыл — в полтора раза медленнее сетки).
                if !self.marks_used && self.step_grid.back().map(|(s, _)| *s) != Some(note.due_step)
                {
                    self.step_grid
                        .push_back((note.due_step, step_frames.max(1)));
                    // Вытеснение с fold: старое сворачиваем в якорь,
                    // сетка от него остается точной.
                    while self.step_grid.len() > 256 {
                        if let Some((s, f)) = self.step_grid.pop_front() {
                            if s >= self.anchor_step {
                                self.anchor_frame = self.anchor_frame.saturating_add(f);
                                self.anchor_step = s + 1;
                            }
                        } else {
                            break;
                        }
                    }
                }
                // Точная сетка: суммируем истинные длины шагов от якоря
                // до шага ноты. Неизвестный хвост (разрыв) — линейно
                // текущей длиной; сброс нумерации — сразу в якорь.
                let mut due = self.anchor_frame;
                if !numbering_reset && note.due_step >= self.anchor_step {
                    let mut cursor = self.anchor_step;
                    for &(gs, gf) in self.step_grid.iter() {
                        if gs < cursor || gs >= note.due_step {
                            continue;
                        }
                        due = due.saturating_add(gf);
                        cursor = gs + 1;
                    }
                    due = due
                        .saturating_add(note.due_step.saturating_sub(cursor) * step_frames.max(1));
                }
                // Привязка ставится по первой штампованной ноте и
                // держится, пока сетка актуальна: опоздание больше шага
                // (смена темпа, догон после stall'а) — «сейчас», а не
                // накопленная ошибка. Пустой pending — не повод
                // ре-якориться: разреженные ноты (пэды, паузы формы)
                // приходят на свою долю общей сетки, а не «сейчас».
                if numbering_reset
                    || !self.anchored
                    || due.saturating_add(step_frames.max(1)) < self.media_cursor
                {
                    if self.anchored && !numbering_reset {
                        // Пере-якорение на живом потоке: генератор отстал
                        // больше чем на шаг (stall, смена темпа, догон).
                        // Это и есть «ритм сбился»: расписание переехало
                        // на текущий кадр. Считаем — видно в баннере.
                        self.reanchors += 1;
                    }
                    self.anchor_step = note.due_step;
                    self.anchor_frame = self.media_cursor;
                    self.anchored = true;
                    // Карта старше якоря — мусор: чистим, память не растет.
                    while self
                        .step_grid
                        .front()
                        .map(|(s, _)| *s < self.anchor_step)
                        .unwrap_or(false)
                    {
                        self.step_grid.pop_front();
                    }
                    due = self.media_cursor;
                }
                self.fresh.push((due, self.pending_seq, note));
                self.pending_seq += 1;
            }
            eaten += 1;
        }
        if !self.fresh.is_empty() {
            self.pending.append(&mut self.fresh);
            // Unstable-сортировка не аллоцирует (в отличие от stable):
            // в аудио-колбэке это важнее. Порядок равных due задаёт
            // счетчик: ноты одного кадра остаются в порядке генерации
            // (кик первым) — движок грузит голоса по порядку, и сортировка
            // по id инструмента этот порядок ломала.
            self.pending
                .sort_unstable_by_key(|(due, seq, _)| (*due, *seq));
        }
        if eaten > 0 {
            self.counters.consumed.fetch_add(eaten, Ordering::Relaxed);
        }
        let fx_now = snapshot(&self.fx);
        self.state.set_room(fx_now.reverb_wet, fx_now.reverb_size);
        self.state.set_duck_mult(fx_now.sidechain);
        // Свип формы: открытость 0..1 маппим на долю базового среза.
        // Закрытый фильтр (~1/8 базы) — глухое интро, открытый — дроп.
        let open = snapshot(&self.filter_open).clamp(0.0, 1.0);
        self.treble = fx_now.treble_hz * openness_gain(open);
        // Живой темп публикуется композером (bpm_out) и читается здесь
        // для будущих темповых фич (эхо вернётся с сеткой от него же).
        let _live_bpm =
            snapshot(&self.bpm).clamp(crate::core::MIN_TEMPO_BPM, crate::core::MAX_TEMPO_BPM);
    }

    /// Один стерео-фрейм f32 с подвижкой фейда паузы.
    /// Перед рендером стреляют due-ноты, чье время пришло.
    ///
    /// Защита от залпа после stall'а: за кадр выстреливает не больше
    /// [`MAX_FIRES_PER_FRAME`] нот, остаток догоняет в следующих кадрах
    /// (растяжка на единицы мс вместо удара всей пачкой в один сэмпл —
    /// пачка рвала полифонию/кражи и звучала хрипом). Ноты, опоздавшие
    /// больше [`MAX_LATE_SECS`], дропаются молча: играть такт
    /// трёхсекундной давности после подвиса — хуже, чем пропустить.
    fn next_frame(&mut self) -> (f32, f32) {
        let late_limit = (self.state.sample_rate() * MAX_LATE_SECS) as u64;
        // Явно поздно (больше восьми части шага): слышно как сбой ритма.
        // Квант аудиоблока (10-46 мс) сюда не попадает — это нормальная
        // задержка доставки, а не сбитый ритм.
        let step_late_limit = (self.state.sample_rate() * 0.125) as u64;
        // Сколько головных нот снимаем в этом кадре: просроченные
        // дропаются, звучащие ограничены [`MAX_FIRES_PER_FRAME`].
        // `drain` вместо `remove(0)` — без сдвига хвоста на каждую ноту.
        let mut take = 0usize;
        let mut fired = 0usize;
        while let Some((due, _, _)) = self.pending.get(take) {
            if *due > self.media_cursor {
                break;
            }
            let late = self.media_cursor.saturating_sub(*due) > late_limit;
            if !late && fired >= MAX_FIRES_PER_FRAME {
                break;
            }
            if !late {
                fired += 1;
            }
            take += 1;
        }
        if take > 0 {
            for (due, _, note) in self.pending.drain(..take) {
                // Протухшие дропаем: играть давно прошедшее — врать ритм.
                let lateness = self.media_cursor.saturating_sub(due);
                if lateness <= late_limit {
                    self.state.push_note(&note);
                    // Опоздание больше четверти шага — слышно: нота
                    // встала не на свою долю. Считаем отдельно от
                    // «протухших» (их дропаем) — это диагностика ритма.
                    if lateness > step_late_limit {
                        self.late_notes += 1;
                    }
                } else {
                    self.late_notes += 1;
                }
            }
        }
        self.media_cursor += 1;
        let target = if self.is_paused.load(Ordering::Relaxed) {
            0.0
        } else {
            1.0
        };
        if self.pause_gain < target {
            self.pause_gain = (self.pause_gain + self.pause_step).min(target);
        } else if self.pause_gain > target {
            self.pause_gain = (self.pause_gain - self.pause_step).max(target);
        }
        let (l, r) = self
            .state
            .render_sample(&self.volumes[..self.volume_count], self.treble);
        (l * self.pause_gain, r * self.pause_gain)
    }

    /// Заливка блока колбэка. Полная пауза: тишина, DSP заморожен.
    pub(crate) fn fill<T>(&mut self, data: &mut [T], channels: usize, conv: fn(f32) -> T) {
        let target = if self.is_paused.load(Ordering::Relaxed) {
            0.0
        } else {
            1.0
        };
        // Кадры считаем в обеих ветках: для диагностики важно знать,
        // что колбэк вообще вызывается, даже в тишине паузы.
        let frames_in_block = data.len() / channels.max(1);
        if !self.logged_first {
            self.logged_first = true;
            eprintln!("[engine] first callback: {frames_in_block} frames/block");
        }
        if target == 0.0 && self.pause_gain <= 0.0 {
            for sample in data.iter_mut() {
                *sample = conv(0.0);
            }
            // В глухой паузе расписание сбрасывается: выход из нее —
            // новые ноты «сейчас», а не догоняющий залп старого плана.
            self.pending.clear();
            self.anchored = false;
            self.counters
                .frames
                .fetch_add(frames_in_block as u64, Ordering::Relaxed);
            return;
        }
        self.begin_block();
        for frame in data.chunks_mut(channels) {
            let (l, r) = self.next_frame();
            if channels == 1 {
                frame[0] = conv((l + r) * 0.5);
            } else {
                for (i, sample) in frame.iter_mut().enumerate() {
                    *sample = conv(if i.is_multiple_of(2) { l } else { r });
                }
            }
        }
        self.counters
            .frames
            .fetch_add(frames_in_block as u64, Ordering::Relaxed);
    }
}

pub(crate) fn f32_identity(x: f32) -> f32 {
    x
}

/// Маппинг открытости фильтра в долю среза верхов: 0 → 1/8 базы
/// (глухое интро), 1 → полный срез стиля (раскрытый дроп).
/// Чистая функция — тестируется без аудио.
fn openness_gain(open: f32) -> f32 {
    0.125 + 0.875 * open.clamp(0.0, 1.0)
}

fn f32_to_i16(x: f32) -> i16 {
    (x.clamp(-1.0, 1.0) * 32767.0) as i16
}

fn f32_to_u16(x: f32) -> u16 {
    // −1 → 0, 0 → 32768, +1 → 65535: беззнаковый PCM без перекоса
    // (старая формула `* 32767 + 32768` давала −1 → 1).
    ((x.clamp(-1.0, 1.0) + 1.0) * 32767.5).round() as u16
}

/// Приоритет конфига: F32-стерео ближе к 48 кГц — первым.
/// Чистая функция на обычных значениях (тестируется без аудио).
fn config_score(sample_format: SampleFormat, channels: u16, max_rate: u32) -> (u8, u16, u32) {
    let fmt = match sample_format {
        SampleFormat::F32 => 0,
        SampleFormat::I16 => 1,
        SampleFormat::U16 => 2,
        _ => 3,
    };
    let ch = if channels == 2 { 0 } else { 1 };
    let rate_dist = (max_rate as i32 - 48_000).unsigned_abs();
    (fmt, ch, rate_dist)
}

/// Выбор конфигурации выхода: F32-стерео preferred, иначе первый
/// поддерживаемый формат (конвертация — в `Pump::fill`).
fn pick_output_config(device: &cpal::Device) -> Result<cpal::SupportedStreamConfig, String> {
    // Кандидаты: сначала дефолт устройства (так делает рабочий v1),
    // затем все перечисленные по приоритету. getMinBufferSize на части
    // телефонов врет (F32 числится, а Oboe его не открывает), поэтому
    // побеждает первый РЕАЛЬНО открывающийся конфиг, а не первый
    // из списка.
    let mut candidates: Vec<cpal::SupportedStreamConfig> = Vec::new();
    let mut seen: Vec<(u8, u16, u32)> = Vec::new();
    let mut push_unique = |c: cpal::SupportedStreamConfig| {
        let key = (
            match c.sample_format() {
                SampleFormat::F32 => 0,
                SampleFormat::I16 => 1,
                SampleFormat::U16 => 2,
                _ => 3,
            },
            c.channels(),
            c.sample_rate().0,
        );
        if !seen.contains(&key) {
            seen.push(key);
            candidates.push(c);
        }
    };
    if let Ok(def) = device.default_output_config() {
        push_unique(def);
    }
    if let Ok(ranges) = device.supported_output_configs() {
        let mut rest: Vec<_> = ranges.collect();
        rest.sort_by_key(|r| config_score(r.sample_format(), r.channels(), r.max_sample_rate().0));
        for r in rest {
            let min = r.min_sample_rate().0;
            let max = r.max_sample_rate().0;
            push_unique(r.with_sample_rate(cpal::SampleRate(max.min(48_000).max(min))));
        }
    }
    candidates
        .into_iter()
        .find(|c| probe_output(device, c))
        .ok_or_else(|| "no supported output config (all rejected by device)".to_string())
}

/// Холостое открытие: проверяет, что конфиг реально берется Oboe.
/// Тихий колбэк, поток сразу дропается.
fn probe_output(device: &cpal::Device, config: &cpal::SupportedStreamConfig) -> bool {
    let cfg: cpal::StreamConfig = config.config();
    let ok = match config.sample_format() {
        SampleFormat::F32 => device
            .build_output_stream(&cfg, |_: &mut [f32], _| {}, |_| {}, None)
            .is_ok(),
        SampleFormat::I16 => device
            .build_output_stream(&cfg, |_: &mut [i16], _| {}, |_| {}, None)
            .is_ok(),
        SampleFormat::U16 => device
            .build_output_stream(&cfg, |_: &mut [u16], _| {}, |_| {}, None)
            .is_ok(),
        _ => false,
    };
    if !ok {
        eprintln!(
            "[engine] config rejected: {} Hz, {:?}, {}ch",
            cfg.sample_rate.0,
            config.sample_format(),
            cfg.channels
        );
    }
    ok
}

pub struct AudioEngine {
    queue: Arc<ArrayQueue<NoteEvent>>,
    /// Метки шагов: по ним насос строит точную сетку времени.
    marks: Arc<ArrayQueue<crate::composer::StepMark>>,
    instruments: Vec<Box<dyn Instrument>>,
    mixer: Arc<RwLock<MixerState>>,
    stop: Arc<AtomicBool>,
    is_paused: Arc<AtomicBool>,
    fx: SharedFx,
    bpm: Arc<RwLock<f32>>,
    counters: EngineCounters,
    audio_config: Arc<RwLock<String>>,
    filter_open: Arc<RwLock<f32>>,
}

impl AudioEngine {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        queue: Arc<ArrayQueue<NoteEvent>>,
        marks: Arc<ArrayQueue<crate::composer::StepMark>>,
        instruments: Vec<Box<dyn Instrument>>,
        mixer: Arc<RwLock<MixerState>>,
        stop: Arc<AtomicBool>,
        is_paused: Arc<AtomicBool>,
        fx: SharedFx,
        bpm: Arc<RwLock<f32>>,
        counters: EngineCounters,
        audio_config: Arc<RwLock<String>>,
        filter_open: Arc<RwLock<f32>>,
    ) -> Self {
        Self {
            queue,
            marks,
            instruments,
            mixer,
            stop,
            is_paused,
            fx,
            bpm,
            counters,
            audio_config,
            filter_open,
        }
    }

    pub fn run(self) -> Result<(), Box<dyn std::error::Error>> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or("Аудиоустройство не найдено")?;
        let counters = self.counters.clone();

        // Насос живет отдельно от потока (shared): при пересоздании потока
        // (воткнули/выдернули наушники, звонок, затык драйвера) голоса,
        // сайдчейн и фейды не рвутся — меняется только выход.
        // Мьютекс здесь без конкуренции: движок трогает насос только
        // между потоками, колбэк — только внутри потока.
        let pump = Arc::new(Mutex::new(Pump::new(
            self.queue,
            self.marks,
            self.instruments,
            self.mixer,
            self.is_paused,
            self.fx,
            self.bpm,
            44_100.0,
            counters.clone(),
            self.filter_open,
        )));
        // Флаг обрыва: ставит колбэк ошибок cpal (отключили наушники —
        // Oboe рвет поток). Сторожок ниже ловит и немые затыки.
        let broken = Arc::new(AtomicBool::new(false));

        let mut first = true;
        loop {
            if self.stop.load(Ordering::Relaxed) {
                return Ok(());
            }
            let config = match pick_output_config(&device) {
                Ok(c) => c,
                Err(e) => {
                    // Стартовая неудача — честная ошибка наружу (красный
                    // баннер), а не вечные ретраи при мертвом устройстве.
                    if first {
                        return Err(format!("Выходной формат не подошел: {e}").into());
                    }
                    eprintln!("[engine] no output config, retrying: {e}");
                    std::thread::sleep(Duration::from_secs(2));
                    continue;
                }
            };
            let sample_rate = config.sample_rate().0 as f32;
            let channels = config.channels() as usize;
            pump.lock()
                .unwrap_or_else(|e| e.into_inner())
                .retune(sample_rate);

            // Описание выбора — в статистику (баннер покажет, какой конфиг
            // взял движок) и в logcat для глубокой отладки.
            let desc = format!(
                "{} Гц, {:?}, {}ch",
                sample_rate as u32,
                config.sample_format(),
                channels
            );
            if let Ok(mut g) = self.audio_config.write() {
                *g = desc.clone();
            }
            eprintln!("[engine] output: {desc}");

            let broken_flag = Arc::clone(&broken);
            broken.store(false, Ordering::Relaxed);
            // В колбэке — блокирующий lock, а не try_lock с заливкой
            // тишины: движок берет pump только при retune, до создания
            // нового стрима (старый к этому моменту уже остановлен),
            // поэтому реальной конкуренции нет. try_lock при редком
            // совпадении вырубал целый блок в тишину — хуже короткого
            // ожидания; инвариант держит поток звука непрерывным.
            let stream = match config.sample_format() {
                SampleFormat::F32 => {
                    let pump = Arc::clone(&pump);
                    device.build_output_stream(
                        &config.into(),
                        move |data: &mut [f32], _| {
                            pump.lock().unwrap_or_else(|e| e.into_inner()).fill(
                                data,
                                channels,
                                f32_identity,
                            );
                        },
                        move |err| {
                            eprintln!("[Engine Error] {err}");
                            broken_flag.store(true, Ordering::Relaxed);
                        },
                        None,
                    )
                }
                SampleFormat::I16 => {
                    let pump = Arc::clone(&pump);
                    device.build_output_stream(
                        &config.into(),
                        move |data: &mut [i16], _| {
                            pump.lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .fill(data, channels, f32_to_i16);
                        },
                        move |err| {
                            eprintln!("[Engine Error] {err}");
                            broken_flag.store(true, Ordering::Relaxed);
                        },
                        None,
                    )
                }
                SampleFormat::U16 => {
                    let pump = Arc::clone(&pump);
                    device.build_output_stream(
                        &config.into(),
                        move |data: &mut [u16], _| {
                            pump.lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .fill(data, channels, f32_to_u16);
                        },
                        move |err| {
                            eprintln!("[Engine Error] {err}");
                            broken_flag.store(true, Ordering::Relaxed);
                        },
                        None,
                    )
                }
                fmt => {
                    if first {
                        return Err(format!("unsupported output sample format: {fmt:?}").into());
                    }
                    eprintln!("[engine] unsupported format {fmt:?}, retrying");
                    std::thread::sleep(Duration::from_secs(2));
                    continue;
                }
            };
            let stream = match stream {
                Ok(s) => s,
                Err(e) => {
                    if first {
                        return Err(e.into());
                    }
                    eprintln!("[engine] build failed, retrying: {e}");
                    std::thread::sleep(Duration::from_secs(2));
                    continue;
                }
            };
            if let Err(e) = stream.play() {
                drop(stream);
                if first {
                    return Err(e.into());
                }
                eprintln!("[engine] play failed, retrying: {e}");
                std::thread::sleep(Duration::from_secs(2));
                continue;
            }
            first = false;

            // Сторожок: обрыв без ошибки (молча вставшие колбэки) ловим
            // по остановившемуся счетчику кадров. В паузе колбэки идут
            // (тишина считается), ложных срабатываний нет.
            let mut last_frames = counters.frames.load(Ordering::Relaxed);
            let mut last_tick = Instant::now();
            loop {
                std::thread::sleep(Duration::from_millis(200));
                if self.stop.load(Ordering::Relaxed) {
                    return Ok(());
                }
                let frames = counters.frames.load(Ordering::Relaxed);
                if frames != last_frames {
                    last_frames = frames;
                    last_tick = Instant::now();
                }
                if broken.load(Ordering::Relaxed)
                    || last_tick.elapsed() > Duration::from_millis(1500)
                {
                    eprintln!("[engine] stream lost, rebuilding");
                    counters.restarts.fetch_add(1, Ordering::Relaxed);
                    break;
                }
            }
            // Пауза перед переоткрытием: даем маршруту переключиться.
            std::thread::sleep(Duration::from_millis(500));
        }
    }
}

/// Глубина сайдчейна по ролям: бас качается сильнее всех,
/// кик не давится никогда. `mult` — глубина стиля (рок давит,
/// lofi дышит). Чистая функция — тестируется без аудио.
pub(crate) fn duck_gain(role: InstrumentRole, kick_env: f32, mult: f32) -> f32 {
    let depth = match role {
        InstrumentRole::Bass => 0.6,
        InstrumentRole::Chords => 0.5,
        InstrumentRole::Lead => 0.4,
        InstrumentRole::Vocal => 0.45,
        InstrumentRole::Hat => 0.25,
        InstrumentRole::Snare => 0.12,
        InstrumentRole::Texture => 0.0,
        InstrumentRole::Kick => 0.0,
    };
    1.0 - mult.clamp(0.0, 2.0) * depth * (kick_env * DUCK_ENV_SCALE).min(1.0)
}

/// Мастер-обработка: мягкое насыщение вместо жесткого клиппинга.
/// Никогда не превышает ±1, тихие сигналы почти не трогает.
pub(crate) fn master_sample(mixed: f32) -> f32 {
    (mixed * MASTER_DRIVE).tanh()
}

/// Совместимая обертка над [`AudioEngine::run`].
pub fn run_audio_engine(
    queue: Arc<ArrayQueue<NoteEvent>>,
    marks: Arc<ArrayQueue<crate::composer::StepMark>>,
    instruments: Vec<Box<dyn Instrument>>,
    mixer: Arc<RwLock<MixerState>>,
    stop: Arc<AtomicBool>,
    is_paused: Arc<AtomicBool>,
) -> Result<(), Box<dyn std::error::Error>> {
    AudioEngine::new(
        queue,
        marks,
        instruments,
        mixer,
        stop,
        is_paused,
        Arc::new(RwLock::new(MasterFx::default())),
        Arc::new(RwLock::new(120.0)),
        EngineCounters::default(),
        Arc::new(RwLock::new(String::new())),
        Arc::new(RwLock::new(0.4)),
    )
    .run()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duck_curves_pump_bass_not_kick() {
        use crate::instruments::InstrumentRole;
        // Тишина — никакого пригибания.
        for role in [
            InstrumentRole::Bass,
            InstrumentRole::Kick,
            InstrumentRole::Snare,
            InstrumentRole::Hat,
            InstrumentRole::Chords,
            InstrumentRole::Lead,
        ] {
            assert_eq!(duck_gain(role, 0.0, 1.0), 1.0);
        }
        // Удар кика: бас пригибается сильнее всех, кик не трогаем.
        let bass = duck_gain(InstrumentRole::Bass, 1.0, 1.0);
        let chords = duck_gain(InstrumentRole::Chords, 1.0, 1.0);
        let hat = duck_gain(InstrumentRole::Hat, 1.0, 1.0);
        assert!(bass < 0.5, "bass must pump: {bass}");
        assert!(bass < chords && chords < hat, "depth order broken");
        assert_eq!(duck_gain(InstrumentRole::Kick, 1.0, 1.0), 1.0);
        // Монотонность: сильнее кик — глубже яма.
        assert!(
            duck_gain(InstrumentRole::Bass, 1.0, 1.0) < duck_gain(InstrumentRole::Bass, 0.3, 1.0)
        );
    }

    #[test]
    fn duck_mult_scales_style_pump() {
        use crate::instruments::InstrumentRole;
        // Ноль — сайдчейна нет вообще (лоуфай дышит ровно).
        assert_eq!(duck_gain(InstrumentRole::Bass, 1.0, 0.0), 1.0);
        // Больше — глубже яма (рок давит).
        let soft = duck_gain(InstrumentRole::Bass, 1.0, 0.5);
        let hard = duck_gain(InstrumentRole::Bass, 1.0, 1.5);
        assert!(hard < soft, "mult must deepen: {hard} vs {soft}");
        // Кик не давится ни при каком множителе.
        assert_eq!(duck_gain(InstrumentRole::Kick, 1.0, 2.0), 1.0);
    }

    #[test]
    fn master_never_clips_and_preserves_quiet() {
        assert!(master_sample(10.0) <= 1.0);
        assert!(master_sample(-10.0) >= -1.0);
        // Тихий сигнал почти не тронут (линейная зона tanh).
        let quiet = master_sample(0.1);
        assert!((quiet - 0.1 * MASTER_DRIVE).abs() < 1e-3);
        // Громкий микс сжат, а не срезан: монотонность сохраняется.
        assert!(master_sample(3.0) > master_sample(1.5));
    }

    #[test]
    fn config_score_prefers_f32_stereo_48k() {
        let best = config_score(SampleFormat::F32, 2, 48_000);
        assert!(config_score(SampleFormat::I16, 2, 48_000) > best);
        assert!(config_score(SampleFormat::F32, 1, 48_000) > best);
        assert!(config_score(SampleFormat::F32, 2, 44_100) > best);
        assert!(
            config_score(SampleFormat::U16, 2, 48_000) > config_score(SampleFormat::I16, 2, 48_000)
        );
    }

    #[test]
    fn openness_spans_eighth_to_full() {
        assert!((openness_gain(0.0) - 0.125).abs() < 1e-6);
        assert!((openness_gain(1.0) - 1.0).abs() < 1e-6);
        assert!((openness_gain(0.4) - 0.475).abs() < 1e-6);
        assert_eq!(openness_gain(-2.0), openness_gain(0.0));
        assert_eq!(openness_gain(99.0), openness_gain(1.0));
    }

    #[test]
    fn sample_conversions_span_full_scale() {
        assert_eq!(f32_to_i16(0.0), 0);
        assert_eq!(f32_to_i16(1.0), 32767);
        assert_eq!(f32_to_i16(-1.0), -32767);
        assert_eq!(f32_to_i16(10.0), 32767);
        assert_eq!(f32_to_u16(0.0), 32768);
        assert_eq!(f32_to_u16(1.0), 65535);
        assert_eq!(f32_to_u16(-1.0), 0);
        assert_eq!(f32_to_u16(-10.0), 0);
    }

    #[test]
    fn swing_grid_lands_on_accumulated_steps() {
        // Сетка точнее линейки: чередующиеся длинные/короткие шаги
        // (свинг) суммируются, а не умножаются текущей длиной.
        // Шаги 0..4 длинами 100/50/100/50мс: ноты должны встать
        // на 0/4410/6615/11025 кадров (@44100), а не на линейке.
        use crossbeam::queue::ArrayQueue;
        let queue = Arc::new(ArrayQueue::new(128));
        let marks = Arc::new(ArrayQueue::new(64));
        let lens = [100_000u32, 50_000, 100_000, 50_000];
        for (i, len) in lens.iter().enumerate() {
            let mut n = NoteEvent::new(6, 60, 0.8, 0.2);
            n.due_step = i as u64;
            n.step_us = *len;
            queue.push(n).unwrap();
        }
        let mut pump = Pump::new(
            queue,
            marks,
            Vec::new(),
            Arc::new(RwLock::new(crate::mixer::MixerState::new())),
            Arc::new(AtomicBool::new(false)),
            Arc::new(RwLock::new(MasterFx::default())),
            Arc::new(RwLock::new(126.0)),
            44_100.0,
            EngineCounters::default(),
            Arc::new(RwLock::new(0.4)),
        );
        let mut block = vec![0.0f32; 600 * 2];
        pump.fill(&mut block, 2, f32_identity);
        let mut dues: Vec<u64> = pump.pending.iter().map(|(d, _, _)| *d).collect();
        dues.sort();
        // Шаг 0 выстрелил в блоке, остальные висят на точных кадрах.
        assert_eq!(dues, vec![4410, 6615, 11025], "swing grid off: {dues:?}");
    }

    #[test]
    fn stamped_notes_wait_for_due_frames() {
        // Due-планирование: штампованные ноты ждут свой медиакадр,
        // а не стреляют в момент разбора. 10 нот шагов 100..109
        // по 0.1с (@44100: 4410 кадров/шаг): первый субблок (600 кадров)
        // должен выстрелить только шаг 100, остальные — висеть
        // в pending. Legacy без штампа стреляет сразу (тест ниже).
        use crossbeam::queue::ArrayQueue;
        let queue = Arc::new(ArrayQueue::new(128));
        let marks = Arc::new(ArrayQueue::new(64));
        for step in 100..110u64 {
            let mut n = NoteEvent::new(6, 60, 0.8, 0.2);
            n.due_step = step;
            n.step_us = 100_000;
            queue.push(n).unwrap();
        }
        let mut pump = Pump::new(
            queue,
            marks,
            Vec::new(),
            Arc::new(RwLock::new(crate::mixer::MixerState::new())),
            Arc::new(AtomicBool::new(false)),
            Arc::new(RwLock::new(MasterFx::default())),
            Arc::new(RwLock::new(126.0)),
            44_100.0,
            EngineCounters::default(),
            Arc::new(RwLock::new(0.4)),
        );
        let mut block = vec![0.0f32; 600 * 2];
        pump.fill(&mut block, 2, f32_identity);
        assert!(
            block.iter().all(|s| s.is_finite()),
            "non-finite scheduled render"
        );
        assert_eq!(
            pump.pending.len(),
            9,
            "due scheduling broken: pending={}",
            pump.pending.len()
        );
        // Докатываем ~1 секунду: все 10 выстрелили по своим кадрам.
        for _ in 0..10 {
            let mut more = vec![0.0f32; 4410 * 2];
            pump.fill(&mut more, 2, f32_identity);
            assert!(more.iter().all(|s| s.is_finite()));
        }
        assert!(pump.pending.is_empty(), "stamped notes lost in pending");
    }

    #[test]
    fn step_marks_give_an_exact_time_grid() {
        // Сетка времени строится по меткам шагов: длина каждого шага
        // известна точно, даже если на шаге нет нот. Без меток пропуск
        // оценивался линейно (по длине текущего шага) — при свинге
        // ошибка копилась по 20-50 мс на шаг, и через пару секунд ноты
        // «опоздали» на полсекунды: ритм сбивался, сетка пере-якоривалась.
        use crossbeam::queue::ArrayQueue;
        let queue = Arc::new(ArrayQueue::new(64));
        let marks = Arc::new(ArrayQueue::new(64));
        // Свинг: шаги чередуются 143 мс / 95 мс.
        let lens_us = [143_000u32, 95_000u32];
        let sr = 44_100.0f32;
        let frame_of = |us: u32| -> u64 { us as u64 * sr as u64 / 1_000_000 };
        for step in 0..8u64 {
            let us = lens_us[(step % 2) as usize];
            marks.push(crate::composer::StepMark { step, us }).unwrap();
            if step % 2 == 0 {
                // Нота только на четных шагах: между ними — пустые
                // шаги, длину которых знают только метки.
                let mut n = NoteEvent::new(2, 36, 0.9, 0.2);
                n.due_step = step;
                n.step_us = us;
                queue.push(n).unwrap();
            }
        }
        let mut pump = Pump::new(
            queue,
            marks,
            Vec::new(),
            Arc::new(RwLock::new(crate::mixer::MixerState::new())),
            Arc::new(AtomicBool::new(false)),
            Arc::new(RwLock::new(MasterFx::default())),
            Arc::new(RwLock::new(126.0)),
            44_100.0,
            EngineCounters::default(),
            Arc::new(RwLock::new(0.4)),
        );
        let mut block = vec![0.0f32; 600 * 2];
        pump.fill(&mut block, 2, f32_identity);
        let mut dues: Vec<u64> = pump.pending.iter().map(|(d, _, _)| *d).collect();
        dues.sort_unstable();
        // Шаги 2, 4, 6 — точные кадры по меткам (шаг 0 прозвучал сразу).
        let want = vec![
            frame_of(lens_us[0]) + frame_of(lens_us[1]),
            2 * (frame_of(lens_us[0]) + frame_of(lens_us[1])),
            3 * (frame_of(lens_us[0]) + frame_of(lens_us[1])),
        ];
        assert_eq!(dues, want, "сетка по меткам не точная");
        assert!(pump.reanchors == 0, "пере-якорение на пустой сетке");
    }

    #[test]
    fn live_reset_with_marks_reanchors_once() {
        // Живая смена сида/стиля: нумерация шагов начинается заново с нуля.
        // Метки и ноты приходят с новой шкалы — сетка пересобирается, но
        // расписание не уезжает вперёд на десятки секунд (иначе тишина),
        // а новая шкала снова считается точно.
        use crossbeam::queue::ArrayQueue;
        let queue = Arc::new(ArrayQueue::new(256));
        let marks = Arc::new(ArrayQueue::new(1024));
        let mut pump = Pump::new(
            queue.clone(),
            Arc::clone(&marks),
            Vec::new(),
            Arc::new(RwLock::new(crate::mixer::MixerState::new())),
            Arc::new(AtomicBool::new(false)),
            Arc::new(RwLock::new(MasterFx::default())),
            Arc::new(RwLock::new(126.0)),
            44_100.0,
            EngineCounters::default(),
            Arc::new(RwLock::new(0.4)),
        );
        let mut block = vec![0.0f32; 4410 * 2];
        // Старая шкала: 300 шагов, ноты на части шагов.
        for step in 0..300u64 {
            marks
                .push(crate::composer::StepMark { step, us: 100_000 })
                .unwrap();
            if step % 4 == 0 {
                let mut n = NoteEvent::new(2, 36, 0.9, 0.2);
                n.due_step = step;
                n.step_us = 100_000;
                queue.push(n).unwrap();
            }
        }
        // Разбираем очередь блоками, как живой тракт.
        for _ in 0..12 {
            pump.fill(&mut block, 2, f32_identity);
        }
        assert!(pump.anchor_step > 0, "сетка не поехала");
        // Новая шкала: шаги снова с нуля.
        for step in 0..8u64 {
            marks
                .push(crate::composer::StepMark { step, us: 100_000 })
                .unwrap();
        }
        let mut n = NoteEvent::new(2, 36, 0.9, 0.2);
        n.due_step = 4;
        n.step_us = 100_000;
        queue.push(n).unwrap();
        let before = pump.media_cursor;
        pump.fill(&mut block, 2, f32_identity);
        assert!(pump.reanchors <= 1, "пере-якорений: {}", pump.reanchors);
        assert_eq!(pump.anchor_step, 0, "якорь не пересобрался");
        // Новая шкала считается точно: шаг 4 — это четыре шага от якоря.
        let due = pump
            .pending
            .iter()
            .filter(|(_, _, n)| n.due_step == 4)
            .map(|(d, _, _)| *d)
            .min()
            .expect("нота шага 4 в расписании");
        let want = pump.anchor_frame + 4 * 4410;
        assert!(
            (due as i64 - want as i64).abs() <= 2,
            "новая шкала считается неточно: {due} против {want}"
        );
        assert!(
            due < before + 4410 * 8,
            "расписание уехало: {due} против {before}"
        );
    }

    #[test]
    fn notes_of_one_frame_keep_generation_order() {
        // Кик и бас в одном кадре: движок грузит голоса по порядку
        // генерации (кик первым — фундамент, чтобы дак бочки был готов).
        // Сортировка по id инструмента этот порядок ломала.
        use crossbeam::queue::ArrayQueue;
        let queue = Arc::new(ArrayQueue::new(8));
        let marks = Arc::new(ArrayQueue::new(64));
        // Первая нота ставит якорь сетки.
        let mut first = NoteEvent::new(6, 60, 0.9, 0.2);
        first.due_step = 0;
        first.step_us = 100_000;
        queue.push(first).unwrap();
        // Кик и бас в одном кадре: движок грузит голоса по порядку
        // генерации (кик первым — фундамент, чтобы дак бочки был готов).
        // Сортировка по id инструмента этот порядок ломала.
        for (id, pitch) in [(2usize, 36u8), (1, 40)] {
            let mut n = NoteEvent::new(id, pitch, 0.9, 0.2);
            n.due_step = 1;
            n.step_us = 100_000;
            queue.push(n).unwrap();
        }
        let mut pump = Pump::new(
            Arc::clone(&queue),
            marks,
            Vec::new(),
            Arc::new(RwLock::new(crate::mixer::MixerState::new())),
            Arc::new(AtomicBool::new(false)),
            Arc::new(RwLock::new(MasterFx::default())),
            Arc::new(RwLock::new(126.0)),
            44_100.0,
            EngineCounters::default(),
            Arc::new(RwLock::new(0.4)),
        );
        let mut block = vec![0.0f32; 600 * 2];
        pump.fill(&mut block, 2, f32_identity);
        let ids: Vec<usize> = pump
            .pending
            .iter()
            .map(|(_, _, n)| n.instrument_id)
            .collect();
        assert_eq!(ids, vec![2, 1], "порядок нот кадра нарушен");
    }

    #[test]
    fn live_numbering_reset_clears_old_step_grid() {
        // Живой сид/стиль начинает форму заново с due_step 0.
        // Старая сетка шагов не должна сворачиваться в новый якорь:
        // иначе через круг (256 шагов) расписание уезжает на десятки
        // секунд вперед и наступает тишина.
        use crossbeam::queue::ArrayQueue;
        let queue = Arc::new(ArrayQueue::new(8));
        let marks = Arc::new(ArrayQueue::new(64));
        let mut pump = Pump::new(
            Arc::clone(&queue),
            marks,
            Vec::new(),
            Arc::new(RwLock::new(crate::mixer::MixerState::new())),
            Arc::new(AtomicBool::new(false)),
            Arc::new(RwLock::new(MasterFx::default())),
            Arc::new(RwLock::new(126.0)),
            44_100.0,
            EngineCounters::default(),
            Arc::new(RwLock::new(0.4)),
        );
        let mut block = vec![0.0f32; 4410 * 2]; // один шаг 100 мс
        let push = |step: u64| {
            let mut n = NoteEvent::new(6, 60, 0.8, 0.2);
            n.due_step = step;
            n.step_us = 100_000;
            queue.push(n).unwrap();
        };
        // Длинная сессия: кольцо переполняется, якорь уезжает вперед.
        for step in 0..300u64 {
            push(step);
            pump.fill(&mut block, 2, f32_identity);
            assert!(
                pump.pending.is_empty(),
                "old timeline stalled at step {step}: pending={}",
                pump.pending.len()
            );
        }
        assert!(pump.anchor_step > 0, "grid never folded");
        // Сброс нумерации. Каждый шаг новой формы должен звучать
        // в своем блоке, а не копиться в pending.
        for step in 0..300u64 {
            push(step);
            pump.fill(&mut block, 2, f32_identity);
            assert!(
                pump.pending.is_empty(),
                "schedule drifted after live reset at new step {step}: pending={}",
                pump.pending.len()
            );
        }
    }

    #[test]
    fn sparse_notes_keep_the_shared_grid() {
        // Разреженные ноты (пэды, паузы формы) не ре-якорятся на «сейчас»
        // только потому, что очередь опустела: шаг 10 обязан звучать
        // через 10 шагов от якоря, а не в момент разбора.
        use crossbeam::queue::ArrayQueue;
        let queue = Arc::new(ArrayQueue::new(8));
        let marks = Arc::new(ArrayQueue::new(64));
        let mut pump = Pump::new(
            Arc::clone(&queue),
            marks,
            Vec::new(),
            Arc::new(RwLock::new(crate::mixer::MixerState::new())),
            Arc::new(AtomicBool::new(false)),
            Arc::new(RwLock::new(MasterFx::default())),
            Arc::new(RwLock::new(126.0)),
            44_100.0,
            EngineCounters::default(),
            Arc::new(RwLock::new(0.4)),
        );
        let mut block = vec![0.0f32; 4410 * 2];
        let mut n = NoteEvent::new(6, 60, 0.8, 0.2);
        n.due_step = 0;
        n.step_us = 100_000;
        queue.push(n).unwrap();
        pump.fill(&mut block, 2, f32_identity); // шаг 0 отзвучал
        for _ in 0..4 {
            pump.fill(&mut block, 2, f32_identity); // четыре шага тишины
        }
        assert_eq!(pump.media_cursor, 5 * 4410);
        let mut n = NoteEvent::new(6, 64, 0.8, 0.2);
        n.due_step = 10;
        n.step_us = 100_000;
        queue.push(n).unwrap();
        pump.fill(&mut block, 2, f32_identity);
        assert_eq!(pump.pending.len(), 1, "sparse note lost");
        assert_eq!(
            pump.pending[0].0, 44_100,
            "sparse note re-anchored to now instead of its grid step"
        );
    }

    #[test]
    fn unstamped_notes_fire_immediately() {
        // Legacy без штампа (тесты, прямые вызовы): разбор = выстрел,
        // никакого расписания. 100 нот одним субблоком упираются
        // только в кап разбора 32.
        use crossbeam::queue::ArrayQueue;
        let queue = Arc::new(ArrayQueue::new(128));
        let marks = Arc::new(ArrayQueue::new(64));
        for i in 0..100 {
            queue
                .push(NoteEvent::new(6, 60 + (i % 12) as u8, 0.8, 0.2))
                .unwrap();
        }
        let mut pump = Pump::new(
            queue,
            marks,
            Vec::new(),
            Arc::new(RwLock::new(crate::mixer::MixerState::new())),
            Arc::new(AtomicBool::new(false)),
            Arc::new(RwLock::new(MasterFx::default())),
            Arc::new(RwLock::new(126.0)),
            44_100.0,
            EngineCounters::default(),
            Arc::new(RwLock::new(0.4)),
        );
        let mut block = vec![0.0f32; 600 * 2];
        pump.fill(&mut block, 2, f32_identity);
        assert!(pump.pending.is_empty(), "legacy notes scheduled?!");
    }

    #[test]
    fn full_ensemble_mix_stays_finite() {
        // Мини-микс всего ансамбля через тот же RenderState, что и live:
        // конечность + вменяемый уровень.
        use crate::instruments::{
            BassGuitar, HiHat, Kick, Kick808, NoteEvent, Rain, SnareDrum, Vinyl, VoicePool,
        };
        use crate::playback::{
            BASS_ID, FESTIVAL_ID, FLUTE_ID, FORMULA_ID, FUZZ_ID, GUITAR_ID, HAT_ID, KEYS_ID,
            KICK808_ID, KICK_ID, LASER_ID, PAD_ID, PLUCK_ID, RAIN_ID, SNARE_ID, VINYL_ID, VOCAL_ID,
            WAVETABLE_ID,
        };
        let sr = 44_100.0;
        let insts: Vec<Box<dyn Instrument>> = vec![
            Box::new(BassGuitar::new(BASS_ID)),
            Box::new(Kick::new(KICK_ID)),
            Box::new(VoicePool::pads(PAD_ID, 6)),
            Box::new(SnareDrum::new(SNARE_ID)),
            Box::new(HiHat::new(HAT_ID)),
            Box::new(VoicePool::plucks(PLUCK_ID, 6)),
            Box::new(VoicePool::keys(KEYS_ID, 8)),
            Box::new(Kick808::new(KICK808_ID)),
            Box::new(VoicePool::lasers(LASER_ID, 5)),
            Box::new(Vinyl::new(VINYL_ID)),
            Box::new(VoicePool::vocals(VOCAL_ID, 4)),
            Box::new(VoicePool::flutes(FLUTE_ID, 4)),
            Box::new(VoicePool::festivals(FESTIVAL_ID, 6)),
            Box::new(VoicePool::guitars(GUITAR_ID, 6)),
            Box::new(Rain::new(RAIN_ID)),
            Box::new(VoicePool::fuzzes(FUZZ_ID, 4)),
            Box::new(VoicePool::formulas(FORMULA_ID, 4)),
            Box::new(VoicePool::wavetables(WAVETABLE_ID, 4)),
        ];
        let mut state = RenderState::new(insts, sr);
        let volumes: Vec<(usize, f32)> = crate::playback::ENSEMBLE_IDS
            .iter()
            .map(|&id| (id, 0.9))
            .collect();
        let notes = [
            (BASS_ID, 40, 0.9, 0.3),
            (KICK_ID, 36, 1.0, 0.1),
            (PAD_ID, 48, 0.6, 1.0),
            (SNARE_ID, 38, 0.8, 0.1),
            (HAT_ID, 42, 0.6, 0.05),
            (PLUCK_ID, 72, 0.7, 0.3),
            (KEYS_ID, 60, 0.6, 1.0),
            (KICK808_ID, 36, 1.0, 0.1),
            (LASER_ID, 76, 0.7, 0.3),
            (VINYL_ID, 60, 0.5, 2.0),
            (VOCAL_ID, 69, 0.7, 0.4),
            (FLUTE_ID, 72, 0.7, 1.0),
            (FESTIVAL_ID, 79, 0.7, 0.3),
            (GUITAR_ID, 64, 0.7, 1.0),
            (RAIN_ID, 60, 0.5, 2.0),
            (FUZZ_ID, 45, 0.8, 0.4),
            (FORMULA_ID, 62, 0.7, 0.3),
            (WAVETABLE_ID, 64, 0.7, 0.3),
        ];
        for (id, pitch, vel, dur) in notes {
            state.push_note(&NoteEvent::new(id, pitch, vel, dur));
        }
        let mut peak: f32 = 0.0;
        let mut sum_sq = 0.0;
        let n = (0.5 * sr) as usize;
        for _ in 0..n {
            let (l, r) = state.render_sample(&volumes, 19000.0);
            assert!(l.is_finite() && r.is_finite());
            assert!(l.abs() <= 1.0 && r.abs() <= 1.0);
            peak = peak.max(l.abs().max(r.abs()));
            sum_sq += l * l + r * r;
        }
        let rms = (sum_sq / (2 * n) as f32).sqrt();
        assert!(peak > 0.15, "mix too quiet: peak={peak}");
        assert!(rms > 0.02, "mix too quiet: rms={rms}");
    }

    #[test]
    fn treble_slew_converges_without_jump() {
        // Срез верхов едет к цели ~120 мс, а не ступенькой: граница секции
        // (свип формы) не дает щелчка в коэффициент фильтра.
        use crate::instruments::{BassGuitar, Instrument};
        let sr = 44_100.0;
        let insts: Vec<Box<dyn Instrument>> = vec![Box::new(BassGuitar::new(1))];
        let mut state = RenderState::new(insts, sr);
        let vols = [(1usize, 0.9f32)];
        for _ in 0..100 {
            state.render_sample(&vols, 2000.0);
        }
        let before = state.treble_smooth;
        assert!((before - 2000.0).abs() < 1.0, "snap failed: {before}");
        state.render_sample(&vols, 19000.0);
        let jump = (state.treble_smooth - before).abs();
        assert!(
            jump < 100.0,
            "treble jumps: {before} -> {}",
            state.treble_smooth
        );
        for _ in 0..sr as usize {
            state.render_sample(&vols, 19000.0);
        }
        assert!(
            (state.treble_smooth - 19000.0).abs() < 50.0,
            "never converges: {}",
            state.treble_smooth
        );
    }
}
