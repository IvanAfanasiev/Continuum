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
use crate::instruments::{dsp::Svf, Instrument, InstrumentRole, NoteEvent};
use crate::mixer::MixerState;
use crate::playback::EngineCounters;
use crate::space::{Echo, SpaceBus};

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

/// Доля голоса, уходящая в темповый дилэй. Вокал и лид в эхе,
/// низ и бочка — сухие.
fn delay_send(role: InstrumentRole) -> f32 {
    match role {
        InstrumentRole::Vocal => 0.6,
        InstrumentRole::Lead => 0.3,
        InstrumentRole::Chords => 0.12,
        InstrumentRole::Hat => 0.08,
        InstrumentRole::Snare => 0.06,
        InstrumentRole::Texture => 0.03,
        InstrumentRole::Bass | InstrumentRole::Kick => 0.0,
    }
}

/// Состояние рендера без cpal: голоса, сайдчейн, эхо, комната, верх.
/// Живой движок и офлайн-прогон (soak) используют один и тот же код —
/// расхождений между «слышу» и «проверяю» нет по построению.
pub(crate) struct RenderState {
    instruments: Vec<Box<dyn Instrument>>,
    index_by_id: HashMap<usize, usize>,
    is_kick: Vec<bool>,
    space: SpaceBus,
    echo: Echo,
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
            echo: Echo::new(sample_rate, 1.0, 0.38, 2800.0),
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
        }
    }

    /// Перенастройка на новую частоту (смена маршрута вывода):
    /// голоса и их состояние сохраняются, сбрасываются только
    /// частотно-зависимые узлы (комната, эхо, верха, сайдчейн).
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
        self.echo = Echo::new(sr, 1.0, 0.38, 2800.0);
        self.treble_l = Svf::default();
        self.treble_r = Svf::default();
        self.treble_smooth = 0.0;
        self.hp_l = Svf::default();
        self.hp_r = Svf::default();
        self.duck_release = (-1.0 / (sr * DUCK_RELEASE_SECS)).exp();
        self.duck_attack = (-1.0 / (sr * 0.002)).exp();
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

    pub(crate) fn set_echo_time(&mut self, secs: f32) {
        self.echo.set_time(secs, self.sample_rate);
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
        let mut kick_l = 0.0;
        let mut kick_r = 0.0;
        for (pos, inst) in self.instruments.iter_mut().enumerate() {
            if !self.is_kick[pos] {
                continue;
            }
            let vol = Self::volume_of(volumes, inst.id());
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
        let mut send_l = 0.0;
        let mut send_r = 0.0;
        for (pos, inst) in self.instruments.iter_mut().enumerate() {
            if self.is_kick[pos] {
                continue;
            }
            let vol = Self::volume_of(volumes, inst.id());
            if vol > 0.0 {
                let duck = duck_gain(inst.role(), self.kick_env, self.duck_mult);
                let (l, r) = inst.next_sample_stereo();
                let cl = l * vol * duck;
                let cr = r * vol * duck;
                mix_l += cl;
                mix_r += cr;
                let send = delay_send(inst.role());
                send_l += cl * send;
                send_r += cr * send;
            } else if inst.is_active() {
                let _ = inst.next_sample_stereo();
            }
        }

        // Темповое эхо вокала/лида — в микс до ревербератора.
        let (echo_l, echo_r) = self.echo.process(send_l, send_r);
        let (wet_l, wet_r) = self.space.process(mix_l + echo_l, mix_r + echo_r);
        // Срез верхов от стиля: lofi мутный, club хирургический.
        // Цель сглажена: прыжок на границе секции (свип формы) едет
        // ~120 мс, а не ступенькой в коэффициент фильтра.
        let target = treble_hz.clamp(2000.0, 20000.0);
        if self.treble_smooth <= 0.0 {
            self.treble_smooth = target;
        }
        let sample_rate = self.sample_rate;
        let k = 1.0 - (-1.0 / (sample_rate * 0.12)).exp();
        self.treble_smooth += (target - self.treble_smooth) * k;
        let treble = self.treble_smooth;
        let (t_l, _, _) = self.treble_l.process(wet_l, treble, 0.7, sample_rate);
        let (t_r, _, _) = self.treble_r.process(wet_r, treble, 0.7, sample_rate);
        // Мастер-HP: инфраниз и постоянка вон (хрип динамиков),
        // музыка начинается с 32 Гц (808) — не задето.
        let (_, _, h_l) = self.hp_l.process(t_l, 28.0, 0.7, sample_rate);
        let (_, _, h_r) = self.hp_r.process(t_r, 28.0, 0.7, sample_rate);
        (master_sample(h_l), master_sample(h_r))
    }
}

/// Насос выходного потока: блоковая подготовка (ноты из очереди,
/// эффективные громкости, живые FX) + посэмпловый рендер с фейдом паузы.
///
/// Один код для всех форматов сэмплов: часть Android-девайсов отдает
/// только I16/U16, и мономорфный F32-поток там не строился (тишина
/// без видимой ошибки). Конвертация — в тонких замыканиях `run`.
pub(crate) struct Pump {
    queue: Arc<ArrayQueue<NoteEvent>>,
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
}

impl Pump {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        queue: Arc<ArrayQueue<NoteEvent>>,
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

    /// Подготовка блока: ноты, громкости, живые FX и время эха.
    fn begin_block(&mut self) {
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
        while let Some(note) = self.queue.pop() {
            self.state.push_note(&note);
            eaten += 1;
        }
        if eaten > 0 {
            self.counters
                .consumed
                .fetch_add(eaten, Ordering::Relaxed);
        }
        let fx_now = snapshot(&self.fx);
        self.state.set_room(fx_now.reverb_wet, fx_now.reverb_size);
        self.state.set_duck_mult(fx_now.sidechain);
        // Свип формы: открытость 0..1 маппим на долю базового среза.
        // Закрытый фильтр (~1/8 базы) — глухое интро, открытый — дроп.
        let open = snapshot(&self.filter_open).clamp(0.0, 1.0);
        self.treble = fx_now.treble_hz * openness_gain(open);
        let live_bpm = snapshot(&self.bpm).clamp(40.0, 220.0);
        // Dotted-8th от живого темпа: эхо всегда в сетке.
        self.state.set_echo_time(60.0 / live_bpm * 0.75);
    }

    /// Один стерео-фрейм f32 с подвижкой фейда паузы.
    fn next_frame(&mut self) -> (f32, f32) {
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
    (x.clamp(-1.0, 1.0) * 32767.0 + 32768.0).clamp(0.0, 65535.0) as u16
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
        rest.sort_by_key(|r| {
            config_score(r.sample_format(), r.channels(), r.max_sample_rate().0)
        });
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
            let stream = match config.sample_format() {
                SampleFormat::F32 => {
                    let pump = Arc::clone(&pump);
                    device.build_output_stream(
                        &config.into(),
                        move |data: &mut [f32], _| {
                            if let Ok(mut p) = pump.try_lock() {
                                p.fill(data, channels, f32_identity);
                            } else {
                                for s in data.iter_mut() {
                                    *s = 0.0;
                                }
                            }
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
                            if let Ok(mut p) = pump.try_lock() {
                                p.fill(data, channels, f32_to_i16);
                            } else {
                                for s in data.iter_mut() {
                                    *s = 0;
                                }
                            }
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
                            if let Ok(mut p) = pump.try_lock() {
                                p.fill(data, channels, f32_to_u16);
                            } else {
                                for s in data.iter_mut() {
                                    *s = 32768;
                                }
                            }
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
                        return Err(
                            format!("unsupported output sample format: {fmt:?}").into()
                        );
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
                if broken.load(Ordering::Relaxed) || last_tick.elapsed() > Duration::from_millis(1500) {
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
    instruments: Vec<Box<dyn Instrument>>,
    mixer: Arc<RwLock<MixerState>>,
    stop: Arc<AtomicBool>,
    is_paused: Arc<AtomicBool>,
) -> Result<(), Box<dyn std::error::Error>> {
    AudioEngine::new(
        queue,
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
        assert!(duck_gain(InstrumentRole::Bass, 1.0, 1.0) < duck_gain(InstrumentRole::Bass, 0.3, 1.0));
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
        assert!(config_score(SampleFormat::U16, 2, 48_000)
            > config_score(SampleFormat::I16, 2, 48_000));
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
        assert_eq!(f32_to_u16(-1.0), 1);
        assert_eq!(f32_to_u16(-10.0), 1);
    }

    #[test]
    fn full_ensemble_mix_stays_finite() {
        // Мини-микс всего ансамбля через тот же RenderState, что и live:
        // конечность + вменяемый уровень.
        use crate::instruments::{
            Acid, BassGuitar, HiHat, Kick, Kick808, NoteEvent, Rain, SnareDrum, Vinyl, VoicePool,
        };
        use crate::playback::{
            ACID_ID, BASS_ID, FESTIVAL_ID, FLUTE_ID, FUZZ_ID, GUITAR_ID, HAT_ID, KEYS_ID,
            KICK808_ID, KICK_ID, LASER_ID, PAD_ID, PLUCK_ID, RAIN_ID, SNARE_ID, VINYL_ID, VOCAL_ID,
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
            Box::new(Acid::new(ACID_ID)),
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
            (ACID_ID, 50, 0.8, 0.4),
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
        assert!(jump < 100.0, "treble jumps: {before} -> {}", state.treble_smooth);
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
