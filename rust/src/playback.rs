//! Контроллер воспроизведения для Flutter и CLI.
//!
//! Владение потоками: движок и композер живут внутри контроллера.
//! Все управляющие вызовы (пауза, темп, стиль, сид, громкости слотов)
//! меняют только флаги и shared-объекты — главная петля и потоки
//! бэкенда при этом НЕ завершаются и НЕ пересоздаются.
//!
//! Семантика присутствия: **громкость слота > 0 = используется**.
//! Нулевой слот исключается из генерации (композер) и не грузит DSP,
//! когда его голос затих (движок). Отдельного флага on/off нет.

use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, Mutex, RwLock,
};
use std::thread::JoinHandle;

use crossbeam::queue::ArrayQueue;

use crate::composer::{Composer, LiveTempo};
use crate::core::{snapshot, DEFAULT_SEED, NOTE_QUEUE_CAPACITY};
use crate::engine::{f32_identity, AudioEngine, MasterFx, Pump, SharedFx};
use crate::instruments::{
    BassGuitar, Group, HiHat, Instrument, Kick, Kick808, NoteEvent, Rain, Shaker, Slot,
    SnareDrum, Vinyl, VoicePool,
};
use crate::mixer::MixerState;
use crate::styles::{self, Style};

pub const BASS_ID: usize = 1;
pub const KICK_ID: usize = 2;
pub const PAD_ID: usize = 3;
pub const SNARE_ID: usize = 4;
pub const HAT_ID: usize = 5;
pub const PLUCK_ID: usize = 6;
pub const KEYS_ID: usize = 7;
pub const KICK808_ID: usize = 8;
pub const LASER_ID: usize = 9;
pub const VINYL_ID: usize = 10;
pub const VOCAL_ID: usize = 11;
pub const FLUTE_ID: usize = 12;
pub const FESTIVAL_ID: usize = 13;
pub const GUITAR_ID: usize = 14;
pub const RAIN_ID: usize = 15;
pub const FUZZ_ID: usize = 16;
pub const SHAKER_ID: usize = 17;
pub const FORMULA_ID: usize = 18;

/// Все id ансамбля — для валидации `set_track` из Flutter.
pub const ENSEMBLE_IDS: [usize; Slot::COUNT] = [
    BASS_ID,
    KICK_ID,
    PAD_ID,
    SNARE_ID,
    HAT_ID,
    PLUCK_ID,
    KEYS_ID,
    KICK808_ID,
    LASER_ID,
    VINYL_ID,
    VOCAL_ID,
    FLUTE_ID,
    FESTIVAL_ID,
    GUITAR_ID,
    RAIN_ID,
    FUZZ_ID,
    SHAKER_ID,
    FORMULA_ID,
];

/// Границы ручки темпа фронта, BPM.
pub const MIN_TEMPO_BPM: f32 = 50.0;
pub const MAX_TEMPO_BPM: f32 = 180.0;

/// Снимок состояния для UI. Именно этот тип едет через flutter_rust_bridge.
#[derive(Debug, Clone)]
pub struct PlaybackStatus {
    pub running: bool,
    pub paused: bool,
    pub style_name: String,
    pub seed: u64,
    /// Канонический код сида `XXX-XXX`: фронт показывает и вводит его.
    pub seed_code: String,
    /// Эффективный темп: ручка фронта или дефолт стиля.
    pub tempo_bpm: f32,
    /// Ручка intensity фронта («температура» генерации).
    pub intensity: f32,
    pub engine_error: Option<String>,
    /// Версия звукового движка (см. ENGINE_VERSION): объясняет,
    /// почему любимый сид звучит иначе после апдейта.
    pub engine_version: u32,
}

impl Default for PlaybackStatus {
    fn default() -> Self {
        Self {
            running: false,
            paused: false,
            style_name: "club".to_string(),
            seed: DEFAULT_SEED,
            seed_code: crate::seed::format_seed_code(DEFAULT_SEED),
            tempo_bpm: 126.0,
            intensity: 1.0,
            engine_error: None,
            engine_version: crate::core::ENGINE_VERSION,
        }
    }
}

/// Один слот микшера для UI: id, имена и живая громкость.
/// `volume` — эффективная (слот × группа), то есть слышимая.
/// `base_volume` — канонический уровень стиля (100% ползунка слота).
/// `group` — имя макро-фейдера слота для фронта.
#[derive(Debug, Clone)]
pub struct TrackInfo {
    pub id: u64,
    pub name: String,
    pub role: String,
    pub group: String,
    pub volume: f32,
    pub base_volume: f32,
}

/// Один макро-фейдер для UI: имя группы и ее множитель.
#[derive(Debug, Clone)]
pub struct GroupInfo {
    pub name: String,
    pub volume: f32,
}

/// Стиль каталога для UI: имя и полка (`club`/`rock`/`chill`).
#[derive(Debug, Clone)]
pub struct StyleMeta {
    pub name: String,
    pub group: String,
}

pub(crate) fn build_instruments() -> Vec<Box<dyn Instrument>> {
    Slot::ALL
        .iter()
        .map(|slot| -> Box<dyn Instrument> {
            match slot {
                Slot::Bass => Box::new(BassGuitar::new(slot.id())),
                Slot::Kick => Box::new(Kick::new(slot.id())),
                Slot::Pad => Box::new(VoicePool::pads(slot.id(), 6)),
                Slot::Snare => Box::new(SnareDrum::new(slot.id())),
                Slot::Hat => Box::new(HiHat::new(slot.id())),
                Slot::Pluck => Box::new(VoicePool::plucks(slot.id(), 6)),
                Slot::Keys => Box::new(VoicePool::keys(slot.id(), 8)),
                Slot::Kick808 => Box::new(Kick808::new(slot.id())),
                Slot::Laser => Box::new(VoicePool::lasers(slot.id(), 5)),
                Slot::Vinyl => Box::new(Vinyl::new(slot.id())),
                Slot::Vocal => Box::new(VoicePool::vocals(slot.id(), 4)),
                Slot::Flute => Box::new(VoicePool::flutes(slot.id(), 4)),
                Slot::Festival => Box::new(VoicePool::festivals(slot.id(), 6)),
                Slot::Guitar => Box::new(VoicePool::guitars(slot.id(), 6)),
                Slot::Rain => Box::new(Rain::new(slot.id())),
                Slot::Fuzz => Box::new(VoicePool::fuzzes(slot.id(), 4)),
                Slot::Shaker => Box::new(Shaker::new(slot.id())),
                Slot::Formula => Box::new(VoicePool::formulas(slot.id(), 4)),
            }
        })
        .collect()
}

/// Счетчики живого тракта (shared между потоками).
/// frames — кадры, отданные аудиоустройству; pushed — ноты от генератора;
/// consumed — ноты, разобранные движком из очереди;
/// restarts — пересоздания потока (смена маршрута, обрыв колбэков).
/// Публичен ради конструктора движка; снаружи интересен только EngineStats.
#[derive(Debug, Clone, Default)]
pub struct EngineCounters {
    pub frames: Arc<AtomicU64>,
    pub pushed: Arc<AtomicU64>,
    pub consumed: Arc<AtomicU64>,
    pub restarts: Arc<AtomicU64>,
}

/// Диагностика живого тракта для фронта. Нули без сессии.
/// Позволяет отличить «поток не стартует» от «генератор молчит»
/// от «все работает, смотри громкость телефона».
#[derive(Debug, Clone, Default)]
pub struct EngineStats {
    pub frames: u64,
    pub pushed: u64,
    pub consumed: u64,
    pub queued: u64,
    /// Выбранный выход (`48000 Гц, F32, 2ch`): видно в баннере,
    /// какой конфиг взял движок на этом телефоне.
    pub audio_config: String,
    /// Сколько раз поток пересоздавался наживую (смена маршрута,
    /// обрыв колбэков). Видно в баннере при затыке.
    pub restarts: u64,
}

/// Запуск фонового потока со страховкой от немой смерти: паника раньше
/// убивала петлю без следа (frames=0 без engine_error). Теперь паника
/// превращается в текст — красный баннер вместо гадания.
fn spawn_guarded(
    name: &'static str,
    sink: &Arc<Mutex<Option<String>>>,
    f: impl FnOnce() + Send + 'static,
) -> JoinHandle<()> {
    let sink = Arc::clone(sink);
    std::thread::spawn(move || {
        if let Err(panic) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
            let msg = if let Some(s) = panic.downcast_ref::<&str>() {
                s.to_string()
            } else if let Some(s) = panic.downcast_ref::<String>() {
                s.clone()
            } else {
                "unknown panic".to_string()
            };
            *sink.lock().unwrap_or_else(|e| e.into_inner()) = Some(format!("{name}: {msg}"));
        }
    })
}

/// Дефолтный набор инструментов стиля: канонические громкости слотов
/// из вайтлиста плюс гости из флекс-пула (выбор — сидом), остальное — в ноль.
/// Общий для контроллера и CLI.
/// Слоты вне набора НЕ запрещены: пользователь может поднять любой
/// вручную, смена стиля или сида просто снова расставит дефолты.
pub fn apply_style_mix(style: &dyn Style, mixer: &mut MixerState, seed: u64) {
    let ensemble = style.ensemble();
    let flex = pick_flex_slots(style, seed);
    for slot in Slot::ALL {
        let vol = if ensemble.contains(&slot) || flex.contains(&slot) {
            slot.base_volume()
        } else {
            0.0
        };
        mixer.set_track(slot.id(), vol);
    }
}

/// Гости микса: взвешенная лотерея сида в бюджете стиля.
/// Число гостей — сидом внутри flex_range; состав — рулеткой по
/// flex_weights без повторов (вытянутое выбывает). Один сид — один
/// состав, distribution следует предпочтениям: частые гости и редкие
/// специи вместо равномерной ротации. Детерминировано полностью.
fn pick_flex_slots(style: &dyn Style, seed: u64) -> Vec<Slot> {
    let pool = style.flex_pool();
    if pool.is_empty() {
        return Vec::new();
    }
    let (lo, hi) = style.flex_range();
    let max = hi.min(pool.len());
    let min = lo.min(max);
    let span = max - min + 1;
    let n = min + (crate::arranger::splitmix64(seed ^ 0x9E5D) % span as u64) as usize;
    if n == 0 {
        return Vec::new();
    }
    let weights_all = style.flex_weights();
    let mut weights: Vec<f32> = pool
        .iter()
        .enumerate()
        .map(|(i, _)| weights_all.get(i).copied().unwrap_or(1.0).max(0.0))
        .collect();
    let mut out = Vec::with_capacity(n);
    let mut draw = crate::arranger::splitmix64(seed ^ 0xF1E9);
    for _ in 0..n {
        let total: f32 = weights.iter().sum();
        if total <= 0.0 {
            break;
        }
        draw = crate::arranger::splitmix64(draw);
        let mut r = (draw >> 32) as f32 / (u32::MAX as f32) * total;
        let mut pick = 0;
        for (i, w) in weights.iter().enumerate() {
            r -= *w;
            if r <= 0.0 {
                pick = i;
                break;
            }
        }
        weights[pick] = 0.0;
        out.push(pool[pick]);
    }
    out
}

/// Кламп ручки темпа. Неположительное значение = сброс к дефолту стиля.
pub fn normalize_tempo(bpm: f32) -> Option<f32> {
    if bpm <= 0.0 {
        None
    } else {
        Some(bpm.clamp(MIN_TEMPO_BPM, MAX_TEMPO_BPM))
    }
}

/// Мастер-эффекты стиля в shared-владении для движка.
fn fx_for_style(style: &dyn Style) -> MasterFx {
    MasterFx {
        reverb_wet: style.reverb_wet(),
        reverb_size: style.reverb_size(),
        treble_hz: style.treble_cutoff(),
        sidechain: style.sidechain_depth().clamp(0.0, 2.0),
    }
}

struct RunningSession {
    queue: Arc<ArrayQueue<NoteEvent>>,
    style: Arc<RwLock<Box<dyn Style>>>,
    seed: Arc<RwLock<u64>>,
    tempo: LiveTempo,
    intensity: Arc<RwLock<f32>>,
    fx: SharedFx,
    live_bpm: Arc<RwLock<f32>>,
    filter_open: Arc<RwLock<f32>>,
    mixer: Arc<RwLock<MixerState>>,
    paused: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    composer_handle: Option<JoinHandle<()>>,
    engine_handle: Option<JoinHandle<()>>,
    engine_error: Arc<Mutex<Option<String>>>,
    counters: EngineCounters,
    audio_config: Arc<RwLock<String>>,
}

impl RunningSession {
    fn launch(style_name: &str, seed: u64, with_audio: bool) -> Self {
        let style_box = styles::make_style(style_name)
            .unwrap_or_else(|| styles::make_style("club").expect("builtin style"));
        let bpm_init = style_box.bpm();
        let fx_init = fx_for_style(&*style_box);
        let style: Arc<RwLock<Box<dyn Style>>> = Arc::new(RwLock::new(style_box));
        let seed_lock = Arc::new(RwLock::new(seed));
        let tempo_lock: LiveTempo = Arc::new(RwLock::new(None));
        let intensity_lock = Arc::new(RwLock::new(1.0f32));
        let fx_lock: SharedFx = Arc::new(RwLock::new(fx_init));
        let bpm_lock = Arc::new(RwLock::new(bpm_init));
        let filter_open = Arc::new(RwLock::new(0.4f32));
        let mixer = Arc::new(RwLock::new(MixerState::new()));
        let queue = Arc::new(ArrayQueue::new(NOTE_QUEUE_CAPACITY));
        let paused = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));
        let engine_error = Arc::new(Mutex::new(None));
        let counters = EngineCounters::default();
        let audio_config: Arc<RwLock<String>> = Arc::new(RwLock::new(String::new()));

        let instruments = build_instruments();
        {
            let style_guard = style.read().unwrap_or_else(|e| e.into_inner());
            let mut mix_guard = mixer.write().unwrap_or_else(|e| e.into_inner());
            apply_style_mix(&**style_guard, &mut mix_guard, seed);
        }

        let composer = Composer::new_shared(
            Arc::clone(&queue),
            Arc::clone(&style),
            Arc::clone(&mixer),
            Arc::clone(&stop),
            Arc::clone(&paused),
            Arc::clone(&seed_lock),
            Arc::clone(&tempo_lock),
            Arc::clone(&intensity_lock),
            Arc::clone(&bpm_lock),
            Arc::clone(&filter_open),
            Arc::clone(&counters.pushed),
        );
        let composer_handle = spawn_guarded("composer", &engine_error, || composer.run());

        // Замыкание захватывает инструменты всегда; если аудио не нужно,
        // оно просто дропается вместе с ними — поток не создается.
        let engine_handle = with_audio.then(|| {
            let engine = AudioEngine::new(
                Arc::clone(&queue),
                instruments,
                Arc::clone(&mixer),
                Arc::clone(&stop),
                Arc::clone(&paused),
                Arc::clone(&fx_lock),
                Arc::clone(&bpm_lock),
                counters.clone(),
                Arc::clone(&audio_config),
                Arc::clone(&filter_open),
            );
            let sink = Arc::clone(&engine_error);
            spawn_guarded("engine", &engine_error, move || {
                if let Err(e) = engine.run() {
                    *sink.lock().unwrap_or_else(|e| e.into_inner()) = Some(e.to_string());
                }
            })
        });

        Self {
            queue,
            style,
            seed: seed_lock,
            tempo: tempo_lock,
            intensity: intensity_lock,
            fx: fx_lock,
            live_bpm: bpm_lock,
            filter_open,
            mixer,
            paused,
            stop,
            composer_handle: Some(composer_handle),
            engine_handle,
            engine_error,
            counters,
            audio_config,
        }
    }

    fn set_style_live(&self, name: &str) -> bool {
        match styles::make_style(name) {
            Some(next) => {
                // Сид читаем ДО style.write: иначе инверсия порядка
                // с set_seed (seed→style против style→seed) = дедлок.
                let seed_now = snapshot(&self.seed);
                if let Ok(mut guard) = self.style.write() {
                    // Новый стиль приносит свой дефолтный набор инструментов,
                    // свою комнату/верх и чистые группы: применяется целиком,
                    // петля живет. Ручки прошлого стиля не переживают переход.
                    let fx_next = fx_for_style(&*next);
                    *guard = next;
                    if let Ok(mut mixer) = self.mixer.write() {
                        mixer.reset_groups();
                        apply_style_mix(&**guard, &mut mixer, seed_now);
                    }
                    if let Ok(mut fx) = self.fx.write() {
                        *fx = fx_next;
                    }
                    true
                } else {
                    false
                }
            }
            None => false,
        }
    }

    /// Чтение микшера без паники на отравленном lock.
    fn with_mixer<T>(&self, f: impl FnOnce(&MixerState) -> T) -> T {
        match self.mixer.read() {
            Ok(g) => f(&g),
            Err(poisoned) => f(&poisoned.into_inner()),
        }
    }

    /// Правка микса с защитой последнего слышимого слота:
    /// изменение, глушащее всё, молча отклоняется.
    fn update_mix(&self, apply: impl FnOnce(&mut MixerState)) {
        if let Ok(mut mixer) = self.mixer.write() {
            let mut probe = mixer.clone();
            apply(&mut probe);
            if probe.has_audible() {
                *mixer = probe;
            }
        }
    }

    fn effective_tempo(&self) -> f32 {
        let override_bpm = snapshot(&self.tempo)
            .filter(|bpm| bpm.is_finite() && *bpm > 0.0);
        match override_bpm {
            Some(bpm) => bpm,
            None => self
                .style
                .read()
                .map(|g| g.bpm())
                .unwrap_or_else(|e| e.into_inner().bpm()),
        }
    }

    fn read_intensity(&self) -> f32 {
        snapshot(&self.intensity).clamp(0.0, 2.0)
    }

    fn status(&self) -> PlaybackStatus {
        let style_name = self
            .style
            .read()
            .map(|g| g.name().to_string())
            .unwrap_or_else(|e| e.into_inner().name().to_string());
        let seed = snapshot(&self.seed);
        PlaybackStatus {
            running: true,
            paused: self.paused.load(Ordering::Relaxed),
            style_name,
            seed,
            seed_code: crate::seed::format_seed_code(seed),
            tempo_bpm: self.effective_tempo(),
            intensity: self.read_intensity(),
            engine_error: self
                .engine_error
                .lock()
                .map(|g| g.clone())
                .unwrap_or_else(|e| e.into_inner().clone()),
            engine_version: crate::core::ENGINE_VERSION,
        }
    }

    fn tracks(&self) -> Vec<TrackInfo> {
        self.with_mixer(|mixer| {
            Slot::ALL
                .iter()
                .map(|slot| TrackInfo {
                    id: slot.id() as u64,
                    name: slot.name().to_string(),
                    role: slot.role().name().to_string(),
                    group: slot.group().name().to_string(),
                    volume: mixer.volume_for(slot.id()),
                    base_volume: slot.base_volume(),
                })
                .collect()
        })
    }

    fn groups(&self) -> Vec<GroupInfo> {
        self.with_mixer(|mixer| {
            Group::ALL
                .iter()
                .map(|gr| GroupInfo {
                    name: gr.name().to_string(),
                    volume: mixer.group_volume(*gr),
                })
                .collect()
        })
    }
}

pub struct PlaybackController {
    session: Mutex<Option<RunningSession>>,
    /// Насос pull-режима (фронтенд играет сам): создается лениво первым
    /// render_pull, переживает вызовы (голоса/хвосты целы). Живет ОТДЕЛЬНО
    /// от мьютекса сессии специально: рендер блока НЕ должен держать
    /// замок сессии, иначе каждый блок сериализует за собой все UI-вызовы
    /// (статус, слайдеры, треки) — фронт лагает, и тем сильнее,
    /// чем больше голосов в миксе.
    pull_pump: Mutex<Option<Pump>>,
}

impl PlaybackController {
    pub fn new() -> Self {
        Self {
            session: Mutex::new(None),
            pull_pump: Mutex::new(None),
        }
    }

    /// Запуск бэкенда. Идемпотентен: если петля уже работает —
    /// только применяет стиль/сид наживую, потоки не пересоздаются.
    pub fn start(&self, style_name: &str, seed: u64, with_audio: bool) -> PlaybackStatus {
        let mut guard = self.session.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(session) = guard.as_mut() {
            session.set_style_live(style_name);
            if let Ok(mut s) = session.seed.write() {
                *s = seed;
            }
            if with_audio && session.engine_handle.is_none() {
                let engine = AudioEngine::new(
                    Arc::clone(&session.queue),
                    build_instruments(),
                    Arc::clone(&session.mixer),
                    Arc::clone(&session.stop),
                    Arc::clone(&session.paused),
                    Arc::clone(&session.fx),
                    Arc::clone(&session.live_bpm),
                    session.counters.clone(),
                    Arc::clone(&session.audio_config),
                    Arc::clone(&session.filter_open),
                );
                let sink = Arc::clone(&session.engine_error);
                session.engine_handle = Some(spawn_guarded("engine", &session.engine_error, move || {
                    if let Err(e) = engine.run() {
                        *sink.lock().unwrap_or_else(|e| e.into_inner()) = Some(e.to_string());
                    }
                }));
            }
            return session.status();
        }
        let session = RunningSession::launch(style_name, seed, with_audio);
        let status = session.status();
        *guard = Some(session);
        // Новая сессия — новые shared-объекты: старый насос сброшен,
        // пересоздастся лениво первым render_pull.
        *self.pull_pump.lock().unwrap_or_else(|e| e.into_inner()) = None;
        status
    }

    /// Останов: поднимает stop-флаг и джойнит потоки.
    /// Единственная функция, завершающая петлю, — по явному запросу.
    pub fn stop(&self) -> PlaybackStatus {
        let session = self
            .session
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        match session {
            None => PlaybackStatus::default(),
            Some(mut s) => {
                let mut status = s.status();
                s.stop.store(true, Ordering::Relaxed);
                if let Some(h) = s.composer_handle.take() {
                    let _ = h.join();
                }
                if let Some(h) = s.engine_handle.take() {
                    let _ = h.join();
                }
                *self.pull_pump.lock().unwrap_or_else(|e| e.into_inner()) = None;
                status.running = false;
                status
            }
        }
    }

    pub fn set_paused(&self, paused: bool) -> PlaybackStatus {
        let guard = self.session.lock().unwrap_or_else(|e| e.into_inner());
        match guard.as_ref() {
            Some(s) => {
                s.paused.store(paused, Ordering::Relaxed);
                s.status()
            }
            None => PlaybackStatus::default(),
        }
    }

    /// Горячая смена стиля: подменяет объект за RwLock и применяет его
    /// дефолтный набор инструментов. Петля продолжает работу.
    /// Неизвестное имя игнорируется — статус вернет фактический стиль.
    pub fn set_style(&self, name: &str) -> PlaybackStatus {
        let guard = self.session.lock().unwrap_or_else(|e| e.into_inner());
        match guard.as_ref() {
            Some(s) => {
                s.set_style_live(name);
                s.status()
            }
            None => PlaybackStatus::default(),
        }
    }

    /// Горячая смена сида: композер подхватит его на следующем шаге,
    /// а микс пересоберется под новый состав (вайтлист + гости сида).
    /// Групповые фейдеры при этом не трогаем — только слоты.
    pub fn set_seed(&self, seed: u64) -> PlaybackStatus {
        let guard = self.session.lock().unwrap_or_else(|e| e.into_inner());
        match guard.as_ref() {
            Some(s) => {
                if let Ok(mut g) = s.seed.write() {
                    *g = seed;
                }
                if let Ok(style) = s.style.read() {
                    if let Ok(mut mixer) = s.mixer.write() {
                        apply_style_mix(&**style, &mut mixer, seed);
                    }
                }
                s.status()
            }
            None => PlaybackStatus::default(),
        }
    }

    /// Сид кодом `XXX-XXX` для фронта: мусор санитизируется, короткий
    /// ввод отклоняется (статус вернет прежний код — фронт откатит поле).
    pub fn set_seed_code(&self, code: &str) -> PlaybackStatus {
        match crate::seed::parse_seed_code(code) {
            Some(seed) => self.set_seed(seed),
            None => self.status(),
        }
    }

    /// Случайный сид для кнопки «кубик»: новый код в статусе.
    pub fn random_seed(&self) -> PlaybackStatus {
        use rand::Rng;
        let seed: u64 = rand::thread_rng().gen::<u64>() % crate::seed::CODE_SPACE;
        self.set_seed(seed)
    }

    /// Живой темп в BPM. Неположительный = сброс к дефолту стиля.
    /// Применяется со следующего шага, петля не перезапускается.
    pub fn set_tempo_bpm(&self, bpm: f32) -> PlaybackStatus {
        let guard = self.session.lock().unwrap_or_else(|e| e.into_inner());
        match guard.as_ref() {
            Some(s) => {
                if let Ok(mut g) = s.tempo.write() {
                    *g = normalize_tempo(bpm);
                }
                s.status()
            }
            None => PlaybackStatus::default(),
        }
    }

    /// Живая intensity 0..2 («температура» генерации): выше — плотнее
    /// и ярче, ниже — разреженнее и тише. Дефолт 1.0 = характер стиля.
    /// Применяется со следующего шага, петля не перезапускается.
    pub fn set_intensity(&self, intensity: f32) -> PlaybackStatus {
        let guard = self.session.lock().unwrap_or_else(|e| e.into_inner());
        match guard.as_ref() {
            Some(s) => {
                if let Ok(mut g) = s.intensity.write() {
                    *g = intensity.clamp(0.0, 2.0);
                }
                s.status()
            }
            None => PlaybackStatus::default(),
        }
    }

    /// Громкость слота наживую. Ноль = слот исключен из генерации и DSP.
    /// Неизвестный id игнорируется. Выключение последнего слышимого
    /// слота отклоняется (тишины быть не должно — фронт откатит ползунок).
    pub fn set_track(&self, instrument_id: u64, volume: f32) -> PlaybackStatus {
        let guard = self.session.lock().unwrap_or_else(|e| e.into_inner());
        match guard.as_ref() {
            Some(s) => {
                let id = instrument_id as usize;
                if ENSEMBLE_IDS.contains(&id) {
                    s.update_mix(|mixer| mixer.set_track(id, volume));
                }
                s.status()
            }
            None => PlaybackStatus::default(),
        }
    }

    /// Макро-фейдер группы наживую («kick», «lead», …). Множитель поверх
    /// слотов: ноль глушит всю группу целиком (исключает из генерации).
    /// Неизвестное имя игнорируется. Переживает смену стиля.
    pub fn set_group_volume(&self, group_name: &str, volume: f32) -> PlaybackStatus {
        let guard = self.session.lock().unwrap_or_else(|e| e.into_inner());
        match guard.as_ref() {
            Some(s) => {
                if let Some(group) = Group::from_name(group_name) {
                    s.update_mix(|mixer| mixer.set_group(group, volume));
                }
                s.status()
            }
            None => PlaybackStatus::default(),
        }
    }

    /// Живой микс для UI: слоты с именами, ролями и громкостями.
    pub fn tracks(&self) -> Vec<TrackInfo> {
        let guard = self.session.lock().unwrap_or_else(|e| e.into_inner());
        match guard.as_ref() {
            Some(s) => s.tracks(),
            None => Vec::new(),
        }
    }

    /// Макро-фейдеры для UI: имя группы и ее множитель.
    pub fn groups(&self) -> Vec<GroupInfo> {
        let guard = self.session.lock().unwrap_or_else(|e| e.into_inner());
        match guard.as_ref() {
            Some(s) => s.groups(),
            None => Vec::new(),
        }
    }

    /// Диагностика живого тракта для UI-баннера. Без сессии — нули.
    pub fn engine_stats(&self) -> EngineStats {
        let guard = self.session.lock().unwrap_or_else(|e| e.into_inner());
        match guard.as_ref() {
            Some(s) => EngineStats {
                frames: s.counters.frames.load(Ordering::Relaxed),
                pushed: s.counters.pushed.load(Ordering::Relaxed),
                consumed: s.counters.consumed.load(Ordering::Relaxed),
                queued: s.queue.len() as u64,
                audio_config: snapshot(&s.audio_config),
                restarts: s.counters.restarts.load(Ordering::Relaxed),
            },
            None => EngineStats::default(),
        }
    }

    /// Pull-рендер для фронтенда (Android играет сам через AudioTrack):
    /// стерео interleaved f32, `frames` фреймов. Насос создается лениво
    /// и переживает вызовы; смена частоты ретюнит без потери голосов.
    /// Замок сессии держим только на клонирование shared-частей —
    /// сам рендер идет без него (иначе лаги UI). Без сессии — пусто.
    /// Кап 4 секунд за вызов от дурака.
    /// Квант планирования — внутри: блок режется на субблоки ~12мс,
    /// очередь разбирается в начале каждого (а не раз на весь блок).
    /// Иначе все ноты блока стартуют разом — залп вместо грува.
    /// Десктопные колбэки и так мелкие, им это ничего не меняет.
    pub fn render_pull(&self, frames: u64, sample_rate: f32) -> Vec<f32> {
        let n = (frames as usize).min(192_000);
        if n == 0 {
            return Vec::new();
        }
        let sr = if sample_rate.is_finite() && sample_rate > 0.0 {
            sample_rate
        } else {
            44_100.0
        };
        struct Parts {
            queue: Arc<ArrayQueue<NoteEvent>>,
            mixer: Arc<RwLock<MixerState>>,
            paused: Arc<AtomicBool>,
            fx: SharedFx,
            live_bpm: Arc<RwLock<f32>>,
            filter_open: Arc<RwLock<f32>>,
            counters: EngineCounters,
        }
        let parts: Option<Parts> = {
            let guard = self.session.lock().unwrap_or_else(|e| e.into_inner());
            guard.as_ref().map(|s| Parts {
                queue: Arc::clone(&s.queue),
                mixer: Arc::clone(&s.mixer),
                paused: Arc::clone(&s.paused),
                fx: Arc::clone(&s.fx),
                live_bpm: Arc::clone(&s.live_bpm),
                filter_open: Arc::clone(&s.filter_open),
                counters: s.counters.clone(),
            })
        };
        let Some(p) = parts else {
            return Vec::new();
        };
        let mut pump_guard = self.pull_pump.lock().unwrap_or_else(|e| e.into_inner());
        let pump = match pump_guard.as_mut() {
            Some(pump) => {
                if (pump.sample_rate() - sr).abs() > 1.0 {
                    pump.retune(sr);
                }
                pump
            }
            None => {
                *pump_guard = Some(Pump::new(
                    p.queue,
                    build_instruments(),
                    p.mixer,
                    p.paused,
                    p.fx,
                    p.live_bpm,
                    sr,
                    p.counters,
                    p.filter_open,
                ));
                pump_guard.as_mut().expect("pump just created")
            }
        };
        let mut out = vec![0.0f32; n * 2];
        // Субблоки по ~12мс (как десктопные колбэки): онсеты точнее
        // шага сетки при любом размере запроса фронтенда.
        const SUB_FRAMES: usize = 600;
        for chunk in out.chunks_mut(SUB_FRAMES * 2) {
            pump.fill(chunk, 2, f32_identity);
        }
        out
    }

    pub fn status(&self) -> PlaybackStatus {
        let guard = self.session.lock().unwrap_or_else(|e| e.into_inner());
        match guard.as_ref() {
            Some(s) => s.status(),
            None => PlaybackStatus::default(),
        }
    }
}

impl Default for PlaybackController {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Офлайн-рендер: генерация без аудиоустройства. Используется флагом
// `--render N` в CLI для проверки, что стиль/сид реально влияют на музыку.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct RenderedNote {
    pub step: usize,
    pub instrument_id: usize,
    pub pitch: u8,
    pub velocity: f32,
}

#[derive(Debug, Clone)]
pub struct RenderReport {
    pub style_name: String,
    pub bpm: f32,
    pub seed: u64,
    pub steps: usize,
    pub notes_total: usize,
    /// (instrument_id, note_count), отсортировано по id.
    pub notes_per_instrument: Vec<(usize, usize)>,
    pub first_notes: Vec<RenderedNote>,
}

pub fn render_offline(style_name: &str, seed: u64, steps: usize) -> Option<RenderReport> {
    let style = styles::make_style(style_name)?;
    let style_name_out = style.name().to_string();
    let bpm = style.bpm();
    let mixer = Arc::new(RwLock::new(MixerState::new()));
    {
        let mut m = mixer.write().unwrap_or_else(|e| e.into_inner());
        apply_style_mix(&*style, &mut m, seed);
    }
    let queue = Arc::new(ArrayQueue::new(NOTE_QUEUE_CAPACITY));
    let style_shared: Arc<RwLock<Box<dyn Style>>> = Arc::new(RwLock::new(style));
    let stop = Arc::new(AtomicBool::new(false));
    let paused = Arc::new(AtomicBool::new(false));
    let mut composer = Composer::new(
        queue,
        Arc::clone(&style_shared),
        Arc::clone(&mixer),
        stop,
        paused,
        seed,
    );

    let mut notes: Vec<RenderedNote> = Vec::new();
    for step in 0..steps {
        let (step_notes, _duration) = composer.next_step();
        for n in step_notes {
            notes.push(RenderedNote {
                step,
                instrument_id: n.instrument_id,
                pitch: n.midi_pitch,
                velocity: n.velocity,
            });
        }
    }

    let mut per_instrument: Vec<(usize, usize)> = ENSEMBLE_IDS
        .iter()
        .map(|&id| (id, notes.iter().filter(|n| n.instrument_id == id).count()))
        .collect();
    per_instrument.sort_unstable();

    Some(RenderReport {
        style_name: style_name_out,
        bpm,
        seed,
        steps,
        notes_total: notes.len(),
        notes_per_instrument: per_instrument,
        first_notes: notes.into_iter().take(12).collect(),
    })
}

// ---------------------------------------------------------------------------
// Soak-прогон: генерация + весь DSP-тракт офлайн на N секунд.
// Доказательство бесконечной стабильности: вся память фиксирована
// (массивы, кольцевые буферы, bounded-очередь), потоки не плодятся,
// счетчики не переполняются за время жизни вселенной. Гоняет тот же
// RenderState и тот же Composer::next_step, что и живой тракт.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct SoakReport {
    pub style_name: String,
    pub seed: u64,
    pub seconds: f32,
    pub samples: u64,
    pub notes: usize,
    pub peak: f32,
    pub rms: f32,
    pub non_finite: u64,
}

pub fn soak_render(
    style_name: &str,
    seed: u64,
    seconds: f32,
    sample_rate: f32,
) -> Option<SoakReport> {
    use crate::core::MAX_ACTIVE_TRACKS;
    use crate::engine::RenderState;

    let style = styles::make_style(style_name)?;
    let style_name_out = style.name().to_string();
    let sr = if sample_rate.is_finite() && sample_rate > 0.0 {
        sample_rate
    } else {
        44_100.0
    };
    let wet = style.reverb_wet();
    let size = style.reverb_size();
    let treble = style.treble_cutoff();
    let echo_secs = 60.0 / style.bpm() * 0.75;

    let mixer = Arc::new(RwLock::new(MixerState::new()));
    {
        let mut m = mixer.write().unwrap_or_else(|e| e.into_inner());
        apply_style_mix(&*style, &mut m, seed);
    }
    let queue = Arc::new(ArrayQueue::new(NOTE_QUEUE_CAPACITY));
    let style_shared: Arc<RwLock<Box<dyn Style>>> = Arc::new(RwLock::new(style));
    let mut composer = Composer::new(
        Arc::clone(&queue),
        Arc::clone(&style_shared),
        Arc::clone(&mixer),
        Arc::new(AtomicBool::new(false)),
        Arc::new(AtomicBool::new(false)),
        seed,
    );

    let mut render = RenderState::new(build_instruments(), sr);
    render.set_room(wet, size);
    render.set_echo_time(echo_secs);

    let target = (seconds.max(0.0) * sr) as u64;
    let mut done = 0u64;
    let mut note_count = 0usize;
    let mut peak: f32 = 0.0;
    let mut sum_sq = 0.0;
    let mut non_finite = 0u64;

    while done < target {
        let (step_notes, duration) = composer.next_step();
        for note in &step_notes {
            render.push_note(note);
        }
        note_count += step_notes.len();

        let mut vols = [(0usize, 0.0f32); MAX_ACTIVE_TRACKS];
        let vlen = {
            let m = mixer.read().unwrap_or_else(|e| e.into_inner());
            let mut buf = [0usize; MAX_ACTIVE_TRACKS];
            let n = m.active_ids(&mut buf);
            for (i, id) in buf[..n].iter().enumerate() {
                vols[i] = (*id, m.volume_for(*id));
            }
            n
        };

        let mut step_samples = ((duration * sr) as u64).max(1);
        while step_samples > 0 && done < target {
            let (l, r) = render.render_sample(&vols[..vlen], treble);
            if l.is_finite() && r.is_finite() {
                peak = peak.max(l.abs().max(r.abs()));
                sum_sq += l * l + r * r;
            } else {
                non_finite += 1;
            }
            done += 1;
            step_samples -= 1;
        }
    }

    let rms = if done > 0 {
        (sum_sq / (2 * done) as f32).sqrt()
    } else {
        0.0
    };
    Some(SoakReport {
        style_name: style_name_out,
        seed,
        seconds: done as f32 / sr,
        samples: done,
        notes: note_count,
        peak,
        rms,
        non_finite,
    })
}

// ---------------------------------------------------------------------------
// WAV-экспорт: первые N секунд генерации в PCM16-файл для прослушивания.
// Тот же тракт, что soak (Composer + RenderState), плюс запись сэмплов.
// Без внешних зависимостей: 44-байтный заголовок WAV пишем вручную.
// ---------------------------------------------------------------------------

/// Рендерит `seconds` секунд стиля/сида в WAV PCM16 stereo.
/// Возвращает отчет (как soak) — пик/RMS файла видно до прослушивания.
pub fn render_wav(
    style_name: &str,
    seed: u64,
    seconds: f32,
    sample_rate: f32,
    path: &str,
) -> Result<SoakReport, String> {
    use crate::core::MAX_ACTIVE_TRACKS;
    use crate::engine::RenderState;

    let style = styles::make_style(style_name).ok_or_else(|| format!("unknown style '{style_name}'"))?;
    let style_name_out = style.name().to_string();
    let sr = if sample_rate.is_finite() && sample_rate > 0.0 {
        sample_rate
    } else {
        44_100.0
    };
    let wet = style.reverb_wet();
    let size = style.reverb_size();
    let treble = style.treble_cutoff();
    let echo_secs = 60.0 / style.bpm() * 0.75;

    let mixer = Arc::new(RwLock::new(MixerState::new()));
    {
        let mut m = mixer.write().unwrap_or_else(|e| e.into_inner());
        apply_style_mix(&*style, &mut m, seed);
    }
    let queue = Arc::new(ArrayQueue::new(NOTE_QUEUE_CAPACITY));
    let style_shared: Arc<RwLock<Box<dyn Style>>> = Arc::new(RwLock::new(style));
    let mut composer = Composer::new(
        Arc::clone(&queue),
        Arc::clone(&style_shared),
        Arc::clone(&mixer),
        Arc::new(AtomicBool::new(false)),
        Arc::new(AtomicBool::new(false)),
        seed,
    );

    let mut render = RenderState::new(build_instruments(), sr);
    render.set_room(wet, size);
    render.set_echo_time(echo_secs);

    let target = (seconds.max(0.0) * sr) as u64;
    let mut pcm: Vec<i16> = Vec::with_capacity((target * 2) as usize);
    let mut done = 0u64;
    let mut note_count = 0usize;
    let mut peak: f32 = 0.0;
    let mut sum_sq = 0.0;
    let mut non_finite = 0u64;

    while done < target {
        let (step_notes, duration) = composer.next_step();
        for note in &step_notes {
            render.push_note(note);
        }
        note_count += step_notes.len();

        let mut vols = [(0usize, 0.0f32); MAX_ACTIVE_TRACKS];
        let vlen = {
            let m = mixer.read().unwrap_or_else(|e| e.into_inner());
            let mut buf = [0usize; MAX_ACTIVE_TRACKS];
            let n = m.active_ids(&mut buf);
            for (i, id) in buf[..n].iter().enumerate() {
                vols[i] = (*id, m.volume_for(*id));
            }
            n
        };

        let mut step_samples = ((duration * sr) as u64).max(1);
        while step_samples > 0 && done < target {
            let (l, r) = render.render_sample(&vols[..vlen], treble);
            if l.is_finite() && r.is_finite() {
                peak = peak.max(l.abs().max(r.abs()));
                sum_sq += l * l + r * r;
                pcm.push((l.clamp(-1.0, 1.0) * 32767.0) as i16);
                pcm.push((r.clamp(-1.0, 1.0) * 32767.0) as i16);
            } else {
                non_finite += 1;
                pcm.push(0);
                pcm.push(0);
            }
            done += 1;
            step_samples -= 1;
        }
    }

    write_wav_pcm16(path, &pcm, 2, sr as u32)
        .map_err(|e| format!("cannot write '{path}': {e}"))?;

    let rms = if done > 0 {
        (sum_sq / (2 * done) as f32).sqrt()
    } else {
        0.0
    };
    Ok(SoakReport {
        style_name: style_name_out,
        seed,
        seconds: done as f32 / sr,
        samples: done,
        notes: note_count,
        peak,
        rms,
        non_finite,
    })
}

/// Минимальный WAV-писатель: PCM16 stereo, 44-байтный заголовок.
fn write_wav_pcm16(
    path: &str,
    samples: &[i16],
    channels: u16,
    sample_rate: u32,
) -> std::io::Result<()> {
    use std::io::Write;
    let data_bytes = (samples.len() * 2) as u32;
    let mut buf = Vec::with_capacity((44 + data_bytes) as usize);
    buf.extend_from_slice(b"RIFF");
    buf.extend_from_slice(&(36 + data_bytes).to_le_bytes());
    buf.extend_from_slice(b"WAVE");
    buf.extend_from_slice(b"fmt ");
    buf.extend_from_slice(&16u32.to_le_bytes());
    buf.extend_from_slice(&1u16.to_le_bytes());
    buf.extend_from_slice(&channels.to_le_bytes());
    buf.extend_from_slice(&sample_rate.to_le_bytes());
    buf.extend_from_slice(&(sample_rate * channels as u32 * 2).to_le_bytes());
    buf.extend_from_slice(&(channels * 2).to_le_bytes());
    buf.extend_from_slice(&16u16.to_le_bytes());
    buf.extend_from_slice(b"data");
    buf.extend_from_slice(&data_bytes.to_le_bytes());
    for s in samples {
        buf.extend_from_slice(&s.to_le_bytes());
    }
    std::fs::File::create(path)?.write_all(&buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arranger::Arranger;
    use crate::styles::{render_style, StepContext};
    use rand::{rngs::StdRng, SeedableRng};
    use std::time::{Duration, Instant};

    fn wait_for_notes(c: &PlaybackController, timeout: Duration) -> bool {
        let start = Instant::now();
        while start.elapsed() < timeout {
            let guard = c.session.lock().unwrap_or_else(|e| e.into_inner());
            let ready = guard.as_ref().map(|s| !s.queue.is_empty()).unwrap_or(false);
            drop(guard);
            if ready {
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        false
    }

    #[test]
    fn headless_lifecycle_keeps_loop_alive() {
        let c = PlaybackController::new();

        // Старт без аудио (без аудиоустройства в CI).
        let s = c.start("club", 42, false);
        assert!(s.running);
        assert_eq!(s.style_name, "club");
        assert_eq!(s.seed, 42);
        assert_eq!(s.tempo_bpm, 126.0);

        // Дефолтный набор club — deep house: панч, саб, снейр, хэты,
        // плак темы, Rhodes-стабы, шейкер, формула + гость (хор по сиду).
        let tracks = c.tracks();
        assert_eq!(tracks.len(), Slot::COUNT);
        let vol = |id: usize| {
            tracks
                .iter()
                .find(|t| t.id == id as u64)
                .map(|t| t.volume)
                .unwrap_or(-1.0)
        };
        assert!(vol(BASS_ID) > 0.0);
        assert!(vol(KICK_ID) > 0.0);
        assert!(vol(SNARE_ID) > 0.0);
        assert!(vol(HAT_ID) > 0.0);
        assert_eq!(vol(KICK808_ID), 0.0);
        assert_eq!(vol(LASER_ID), 0.0);
        assert!(vol(PLUCK_ID) > 0.0);
        assert!(vol(KEYS_ID) > 0.0);
        assert!(vol(SHAKER_ID) > 0.0);
        assert_eq!(vol(VINYL_ID), 0.0);
        // Гость из пула (бюджет 0–1): хор либо есть, либо нет.
        // Кислота в автомикс не ходит (только вручную).
        let guests: Vec<u64> = tracks
            .iter()
            .filter(|t| {
                t.volume > 0.0
                    && ![
                        BASS_ID, KICK_ID, SNARE_ID, HAT_ID, PLUCK_ID, KEYS_ID, SHAKER_ID,
                        FORMULA_ID,
                    ]
                    .contains(&(t.id as usize))
            })
            .map(|t| t.id)
            .collect();
        assert!(
            guests.len() <= 1,
            "guests: {guests:?}"
        );
        for id in &guests {
            let slot = Slot::from_id(*id as usize).expect("known slot");
            assert!(
                [Slot::Vocal].contains(&slot),
                "stranger guest: {slot:?}"
            );
        }

        // Петля реально генерирует ноты.
        assert!(wait_for_notes(&c, Duration::from_secs(3)));

        // Группы: 8 макро-фейдеров; один Kick на два кика.
        assert_eq!(c.groups().len(), 8);
        let s = c.set_group_volume("kick", 0.5);
        assert!(s.running);
        let kick = c
            .tracks()
            .iter()
            .find(|t| t.id == KICK_ID as u64)
            .unwrap()
            .clone();
        assert_eq!(kick.group, "kick");
        assert_eq!(kick.volume, 0.95 * 0.5); // 0.95 слота × 0.5 группы
        assert!(c.set_group_volume("nope", 0.5).running); // чужое имя игнор
        assert!(c.status().running);

        // Повторный старт — не плодит потоки, только применяет параметры.
        let s = c.start("club", 42, false);
        assert!(s.running);

        // Пауза/плей не останавливают петлю.
        assert!(c.set_paused(true).paused);
        assert!(!c.set_paused(false).paused);
        assert!(c.status().running);

        // Живой темп: кламп и сброс к дефолту, петля жива.
        assert_eq!(c.set_tempo_bpm(100.0).tempo_bpm, 100.0);
        assert_eq!(c.set_tempo_bpm(1000.0).tempo_bpm, MAX_TEMPO_BPM);
        assert_eq!(c.set_tempo_bpm(0.0).tempo_bpm, 126.0);
        // Intensity: кламп 0..2, дефолт 1.0.
        assert_eq!(c.set_intensity(1.5).intensity, 1.5);
        assert_eq!(c.set_intensity(99.0).intensity, 2.0);
        assert_eq!(c.set_intensity(-1.0).intensity, 0.0);
        assert!(c.status().running);

        // Горячая смена стиля и сида: lofi гасит 808 и лазер, включает
        // винил; группы при этом сбрасываются на дефолт (был 0.5 → 1.0).
        let s = c.set_style("lofi");
        assert_eq!(s.style_name, "lofi");
        let tracks = c.tracks();
        let kick808 = tracks
            .iter()
            .find(|t| t.id == KICK808_ID as u64)
            .unwrap();
        assert_eq!(kick808.volume, 0.0);
        let vinyl = tracks.iter().find(|t| t.id == VINYL_ID as u64).unwrap();
        assert!(vinyl.volume > 0.0);
        assert_eq!(
            c.groups().iter().find(|g| g.name == "kick").unwrap().volume,
            1.0
        );
        assert!(c.status().running);
        // Неизвестный стиль игнорируется.
        assert_eq!(c.set_style("nope").style_name, "lofi");
        assert_eq!(c.set_seed(7).seed, 7);

        // Громкость 0 исключает слот; неизвестный id игнорируется.
        let s = c.set_track(BASS_ID as u64, 0.0);
        assert!(s.running);
        assert_eq!(
            c.tracks()
                .iter()
                .find(|t| t.id == BASS_ID as u64)
                .unwrap()
                .volume,
            0.0
        );
        let s = c.set_track(999, 1.0);
        assert!(s.running);
        // Последний слышимый слот выключить нельзя: возвращаем бас,
        // глушим все группы, кроме басовой, затем пробуем и ее —
        // отклонено, звук остался.
        c.set_track(BASS_ID as u64, 0.9);
        for g in c.groups() {
            if g.name != "bass" {
                c.set_group_volume(&g.name, 0.0);
            }
        }
        let before = c
            .tracks()
            .iter()
            .find(|t| t.id == BASS_ID as u64)
            .unwrap()
            .volume;
        assert!(before > 0.0);
        c.set_group_volume("bass", 0.0);
        let after = c
            .tracks()
            .iter()
            .find(|t| t.id == BASS_ID as u64)
            .unwrap()
            .volume;
        assert_eq!(after, before, "last audible must stay");
        let _ = c.set_group_volume("bass", 1.0);

        // Стоп — единственная точка завершения петли.
        let s = c.stop();
        assert!(!s.running);
        assert!(!c.status().running);
    }

    #[test]
    fn pull_pump_lives_outside_session_lock() {
        // Рендер блока НЕ держит замок сессии: иначе каждый блок
        // сериализует за собой статус/слайдеры/треки и фронт лагает
        // тем сильнее, чем больше голосов (регрессия с телефона).
        let c = PlaybackController::new();
        c.start("club", 42, false);
        assert!(c.pull_pump.lock().unwrap().is_none());
        let out = c.render_pull(44_100, 44_100.0);
        assert_eq!(out.len(), 88_200);
        assert!(out.iter().all(|s| s.is_finite()));
        assert!(c.pull_pump.lock().unwrap().is_some());
        c.stop();
        assert!(c.pull_pump.lock().unwrap().is_none());
    }

    #[test]
    fn concurrent_calls_do_not_deadlock_render() {
        // Статус/статистика из 4 потоков во время рендеров:
        // завершается (порядок замков консистентен), а не висит.
        use std::sync::{Arc, Barrier};
        let c = Arc::new(PlaybackController::new());
        c.start("club", 42, false);
        let bar = Arc::new(Barrier::new(5));
        let mut hs = Vec::new();
        for _ in 0..4 {
            let (cc, bb) = (Arc::clone(&c), Arc::clone(&bar));
            hs.push(std::thread::spawn(move || {
                bb.wait();
                for _ in 0..10 {
                    let _ = cc.status();
                    let _ = cc.engine_stats();
                }
            }));
        }
        bar.wait();
        for _ in 0..3 {
            let out = c.render_pull(44_100, 44_100.0);
            assert_eq!(out.len(), 88_200);
        }
        for h in hs {
            h.join().unwrap();
        }
    }

    #[test]
    fn pull_backlog_spreads_across_subblocks() {
        // Залп вместо грува: backlog после stall'а разбирается порциями
        // по субблокам (~12мс), а не одним махом в начале блока.
        // Один субблок (600 фреймов) съедает не больше капа — остальное
        // ждет следующих субблоков. Композер подсыпает сверху, поэтому
        // границы с запасом, но разрыв кратный.
        let c = PlaybackController::new();
        c.start("club", 42, false);
        let push100 = || {
            let guard = c.session.lock().unwrap_or_else(|e| e.into_inner());
            let s = guard.as_ref().expect("session");
            for i in 0..100 {
                let _ = s.queue.push(NoteEvent::new(PLUCK_ID, 60 + (i % 12) as u8, 0.8, 0.2));
            }
        };
        push100();
        let before = c.engine_stats().consumed;
        let out = c.render_pull(600, 44_100.0);
        assert_eq!(out.len(), 1200);
        assert!(out.iter().all(|s| s.is_finite()));
        let eaten = c.engine_stats().consumed - before;
        assert!(eaten < 60, "backlog volley in one subblock: {eaten}");
        // А весь блок 4800 разбирает все восемью порциями.
        push100();
        let before = c.engine_stats().consumed;
        let out = c.render_pull(4800, 44_100.0);
        assert_eq!(out.len(), 9600);
        assert!(out.iter().all(|s| s.is_finite()));
        let eaten = c.engine_stats().consumed - before;
        assert!(eaten >= 100, "subblocks do not drain: {eaten}");
        let _ = c.stop();
    }

    #[test]
    fn live_tempo_and_intensity_reach_composer() {
        // Ручки фронта доходят до генерации: статус отражает,
        // плотность нот следует за intensity.
        let c = PlaybackController::new();
        c.start("club", 11, false);
        let s = c.set_intensity(0.1);
        assert!((s.intensity - 0.1).abs() < 1e-5, "intensity ignored");
        let s = c.set_tempo_bpm(100.0);
        assert!((s.tempo_bpm - 100.0).abs() < 1e-3, "tempo ignored");
        // Тихая генерация дает меньше нот, чем горячая.
        std::thread::sleep(Duration::from_millis(300));
        c.set_intensity(0.1);
        let _ = c.render_pull(4800, 44_100.0);
        let cold = c.engine_stats().pushed;
        c.set_intensity(2.0);
        std::thread::sleep(Duration::from_millis(1500));
        let _ = c.render_pull(4800, 44_100.0);
        let hot = c.engine_stats().pushed;
        assert!(hot > cold, "intensity does not drive density: {cold} vs {hot}");
        let _ = c.stop();
    }

    #[test]
    fn status_carries_engine_version() {
        // Версия движка едет в статусе: объясняет смену звука сидов.
        let c = PlaybackController::new();
        assert_eq!(
            c.status().engine_version,
            crate::core::ENGINE_VERSION
        );
        c.start("club", 11, false);
        assert_eq!(
            c.status().engine_version,
            crate::core::ENGINE_VERSION
        );
        let _ = c.stop();
    }

    #[test]
    fn pull_renders_finite_blocks() {
        // Pull-режим фронта: старт без аудио, затем блоки сэмплов.
        // Размер = фреймы × 2 канала, все конечно.
        let c = PlaybackController::new();
        let s = c.start("club", 42, false);
        assert!(s.running);
        // Даем композеру положить ноты в очередь.
        std::thread::sleep(Duration::from_secs(2));
        let out = c.render_pull(44_100, 44_100.0);
        assert_eq!(out.len(), 88_200);
        assert!(out.iter().all(|s| s.is_finite()));
        // Смена частоты ретюнит насос без потери состояния.
        let out2 = c.render_pull(48_000, 48_000.0);
        assert_eq!(out2.len(), 96_000);
        assert!(out2.iter().all(|s| s.is_finite()));
        // Без сессии — пусто, без паники.
        let c2 = PlaybackController::new();
        assert!(c2.render_pull(100, 44_100.0).is_empty());
        assert!(c.render_pull(0, 44_100.0).is_empty());
        let _ = c.stop();
    }

    #[test]
    fn render_differs_per_style() {
        let club = render_offline("club", 11, 64).expect("club renders");
        let lofi = render_offline("lofi", 11, 64).expect("lofi renders");
        assert_ne!(club.bpm, lofi.bpm);
        let club_pitches: Vec<u8> = club.first_notes.iter().map(|n| n.pitch).collect();
        let lofi_pitches: Vec<u8> = lofi.first_notes.iter().map(|n| n.pitch).collect();
        assert_ne!(club_pitches, lofi_pitches);
    }

    #[test]
    fn render_rejects_unknown_style() {
        assert!(render_offline("nope", 1, 8).is_none());
    }

    #[test]
    fn seeds_123_and_321_differ_from_first_bar() {
        // Жалоба пользователя: первые такты сидов 123 и 321 звучали
        // одинаково. Полюса грува, ротация лада и прогрессия обязаны дать
        // разную музыку с начала грува (первые два такта: heartbeat
        // одинаков, дальше паттерны расходятся).
        fn signature(seed: u64) -> Vec<(usize, usize, u8)> {
            let style = styles::make_style("club").expect("club");
            let ensemble = style.ensemble();
            let active: Vec<usize> = Slot::ALL
                .iter()
                .filter(|s| ensemble.contains(s))
                .map(|s| s.id())
                .collect();
            let mut rng = StdRng::seed_from_u64(seed);
            let mut arranger = Arranger::new(seed);
            let mut song = arranger.advance_measure();
            let mut out = Vec::new();
            for step in 0..64 {
                let spm = style.steps_per_measure();
                let sim = step % spm;
                // Песню двигаем как живой движок: иначе все 64 шага
                // торчат в первом такте интро и тест врет.
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
                    out.push((step, note.instrument_id, note.midi_pitch));
                }
            }
            out.sort_unstable();
            out
        }
        let a = signature(123);
        let b = signature(321);
        assert!(!a.is_empty() && !b.is_empty());
        assert_ne!(a, b, "seeds 123 and 321 open identically");
    }

    #[test]
    fn different_seeds_give_different_music() {
        // «Бесконечные» вариации: ротация лада, модальный вариант,
        // плотность и RNG-поток — все от сида. Два сида одного стиля
        // обязаны дать заметно разную музыку. Сравниваем полное
        // распределение по инструментам, а не тоталы (тоталы случайно
        // совпадают, музыка при этом разная).
        let a = render_offline("club", 11, 64).expect("renders");
        let b = render_offline("club", 12, 64).expect("renders");
        assert_ne!(
            a.notes_per_instrument, b.notes_per_instrument,
            "seeds 11 and 12 sound identical"
        );
    }

    #[test]
    fn pole_grooves_engage_on_some_seeds() {
        // Полюса реально переключаются: синкопа кика на 14-м шаге
        // встречается у одних сидов и отсутствует у других.
        // (Морфинга больше нет — разнообразие только из полюсов и гейтов.)
        fn full_has_push(seed: u64) -> bool {
            let style = styles::make_style("club").expect("club");
            let active: Vec<usize> = Slot::ALL.iter().map(|s| s.id()).collect();
            let mut rng = StdRng::seed_from_u64(seed);
            let mut arranger = Arranger::new(seed);
            let mut song = arranger.advance_measure();
            for step in 0..400 {
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
                    if note.instrument_id == KICK_ID && sim == 14 {
                        return true;
                    }
                }
            }
            false
        }
        let mut with_push = 0;
        for seed in 0..40 {
            if full_has_push(seed) {
                with_push += 1;
            }
        }
        assert!(
            with_push > 0 && with_push < 40,
            "poles never vary: {with_push}/40"
        );
    }

    #[test]
    fn all_styles_keep_bass_in_bass_octave() {
        // Регрессия «в басу нет басов»: басовые ноты любого стиля обязаны
        // лежать в 28..=52 (A0..E3 ≈ 41..165 Гц), а не в нижней середине.
        // Верхняя граница включительна: октавные скачки — тоже бас.
        for name in styles::style_names() {
            let style = styles::make_style(name).expect("known style");
            let mut rng = StdRng::seed_from_u64(424242);
            let active: Vec<usize> = ENSEMBLE_IDS.into_iter().collect();
            let mut arranger = Arranger::new(424242);
            let mut song = arranger.advance_measure();
            let mut bass_count = 0;
            for step in 0..64 {
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
                    if note.instrument_id == BASS_ID {
                        assert!(
                            (28..=52).contains(&note.midi_pitch),
                            "{name}: bass pitch {} out of bass octave",
                            note.midi_pitch
                        );
                        bass_count += 1;
                    }
                }
            }
            assert!(bass_count > 0, "{name}: no bass notes at all");
        }
    }

    #[test]
    fn render_is_deterministic() {
        let a = render_offline("club", 5, 32).expect("renders");
        let b = render_offline("club", 5, 32).expect("renders");
        assert_eq!(a.notes_total, b.notes_total);
        assert_eq!(a.notes_per_instrument, b.notes_per_instrument);
    }

    #[test]
    fn wav_export_writes_valid_file() {
        // 2 секунды на пониженной частоте: быстро, но весь тракт настоящий.
        let path = std::env::temp_dir().join("cont_wav_export_test.wav");
        let path_str = path.to_string_lossy().to_string();
        let report = render_wav("club", 11, 2.0, 22_050.0, &path_str).expect("renders wav");
        assert_eq!(report.samples, 44_100);
        assert_eq!(report.non_finite, 0);

        let bytes = std::fs::read(&path).expect("wav readable");
        // RIFF/WAVE-магия + размер = заголовок + сэмплы × 2 канала × 2 байта.
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        assert_eq!(&bytes[12..16], b"fmt ");
        assert_eq!(&bytes[36..40], b"data");
        assert_eq!(bytes.len(), 44 + 44_100 * 2 * 2);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn mix_brightness_stays_in_ref_bands() {
        // Яркость микса в полосах референсов: lofi темный (пыль, не резкость),
        // club светлый (хэт и клэп слышно). Метрика — доля ВЧ-энергии
        // (first-difference) за 20 секунд генерации с дефолтным миксом.
        // Замеры: lofi 0.0008 (Autumn 0.0107 — мы темнее, это ок),
        // club 0.0019. Полосы широкие: ловят регресс, а не вкус.
        use crate::composer::Composer;
        use crate::core::{MAX_ACTIVE_TRACKS, NOTE_QUEUE_CAPACITY};
        use crate::engine::RenderState;
        use crossbeam::queue::ArrayQueue;
        fn hf_ratio(style_name: &str, seed: u64) -> f32 {
            let style = styles::make_style(style_name).expect("style");
            let sr = 44_100.0;
            let treble = style.treble_cutoff();
            let mixer = Arc::new(RwLock::new(MixerState::new()));
            {
                let mut m = mixer.write().unwrap_or_else(|e| e.into_inner());
                apply_style_mix(&*style, &mut m, seed);
            }
            let queue = Arc::new(ArrayQueue::new(NOTE_QUEUE_CAPACITY));
            let shared: Arc<RwLock<Box<dyn Style>>> = Arc::new(RwLock::new(style));
            let mut composer = Composer::new(
                Arc::clone(&queue),
                Arc::clone(&shared),
                Arc::clone(&mixer),
                Arc::new(AtomicBool::new(false)),
                Arc::new(AtomicBool::new(false)),
                seed,
            );
            let mut render = RenderState::new(build_instruments(), sr);
            let target = (20.0 * sr) as u64;
            let mut done = 0u64;
            let (mut sum_sq, mut hf, mut prev) = (0.0f32, 0.0f32, 0.0f32);
            while done < target {
                let (step_notes, duration) = composer.next_step();
                for note in &step_notes {
                    render.push_note(note);
                }
                let mut vols = [(0usize, 0.0f32); MAX_ACTIVE_TRACKS];
                let vlen = {
                    let m = mixer.read().unwrap_or_else(|e| e.into_inner());
                    let mut buf = [0usize; MAX_ACTIVE_TRACKS];
                    let n = m.active_ids(&mut buf);
                    for (i, id) in buf[..n].iter().enumerate() {
                        vols[i] = (*id, m.volume_for(*id));
                    }
                    n
                };
                let mut step_samples = ((duration * sr) as u64).max(1);
                while step_samples > 0 && done < target {
                    let (l, r) = render.render_sample(&vols[..vlen], treble);
                    let s = (l + r) * 0.5;
                    assert!(s.is_finite());
                    sum_sq += s * s;
                    hf += (s - prev) * (s - prev);
                    prev = s;
                    done += 1;
                    step_samples -= 1;
                }
            }
            hf / sum_sq.max(1e-9)
        }
        let lofi = hf_ratio("lofi", 11);
        assert!(lofi < 0.004, "lofi too bright: {lofi}");
        let club = hf_ratio("club", 11);
        assert!(club > 0.001, "club too dark: {club}");
    }

    #[test]
    fn soak_proves_long_run_stability() {
        // Весь тракт (генерация + DSP) 6 секунд на пониженной частоте:
        // конечность, уровень, генерация не глохнет. Первый такт —
        // уже тема в полную (разогрева нет), порог нот держит плотность.
        // Часы/сутки — флагом --soak в CLI, код тот же самый.
        let report = soak_render("club", 11, 6.0, 22050.0).expect("soaks");
        assert_eq!(report.non_finite, 0);
        assert_eq!(report.samples, 132300);
        assert!(report.notes > 10, "generation stalled: {}", report.notes);
        assert!(
            report.peak > 0.05 && report.peak <= 1.0,
            "peak out of range: {}",
            report.peak
        );
        assert!(report.rms > 0.005, "mix too quiet: {}", report.rms);
    }

    #[test]
    fn loudness_holds_commercial_band() {
        // Мастеринг-контракт: rms в полосе референсов (коммерция ~0.3,
        // наши были ~0.1), пики в потолке лимитера, крест-фактор жив
        // (транзиенты не сквошены: ниже 4.0 — каша, выше 13 — тихо).
        let report = soak_render("club", 11, 12.0, 22050.0).expect("soaks");
        assert_eq!(report.non_finite, 0);
        assert!(
            report.peak <= 1.0,
            "limiter leaks: {}",
            report.peak
        );
        assert!(
            (0.12..=0.30).contains(&report.rms),
            "rms out of band: {}",
            report.rms
        );
        let crest = report.peak / report.rms.max(1e-6);
        assert!(
            (4.0..=13.0).contains(&crest),
            "transients squashed or mix dead: crest {crest}"
        );
    }

    #[test]
    fn style_ensembles_match_character() {
        // Характер дефолтных наборов из референсов пользователя:
        // club — битмейкер (бочка, снейр, хэты, саб, винил) + гости;
        // lofi — винил, Rhodes, флейта, гитара и дождь, без лазера и 808.
        let ensemble_of = |style: &str| styles::make_style(style).unwrap().ensemble().to_vec();
        use Slot::*;

        let club = ensemble_of("club");
        for s in [Bass, Kick, Snare, Hat, Pluck, Keys, Shaker, Formula] {
            assert!(club.contains(&s), "club: {s:?} missing");
        }
        for s in [
            Pad, Laser, Vocal, Flute, Guitar, Rain, Festival, Fuzz,
            Kick808, Vinyl,
        ] {
            assert!(!club.contains(&s), "club: {s:?} should be off");
        }

        let lofi = ensemble_of("lofi");
        for s in [
            Bass, Kick, Pad, Snare, Hat, Keys, Vinyl, Flute, Guitar,
        ] {
            assert!(lofi.contains(&s), "lofi: {s:?} missing");
        }
        // Плак вне дефолта: поверх Rhodes и гитары дает кашу —
        // включается вручную. Дождь тоже ручной (винил один держит
        // подложку). Остальные чужие — тоже вне набора.
        for s in [Pluck, Kick808, Laser, Vocal, Festival, Formula, Rain] {
            assert!(!lofi.contains(&s), "lofi: {s:?} should be off");
        }

        // Все стили: непустой вайтлист из известных слотов.
        for name in styles::style_names() {
            let ensemble = ensemble_of(name);
            assert!(!ensemble.is_empty(), "{name}: empty ensemble");
            for s in &ensemble {
                assert!(Slot::ALL.contains(s), "{name}: unknown slot {s:?}");
            }
        }

        // Rock: механика — чистая гитара риффом плюс рев фазза.
        let rock = ensemble_of("rock");
        for s in [Bass, Kick, Snare, Hat, Guitar, Fuzz] {
            assert!(rock.contains(&s), "rock: {s:?} missing");
        }
        for s in [Pad, Keys, Laser, Vocal, Vinyl, Rain, Flute, Festival, Kick808, Pluck, Formula] {
            assert!(!rock.contains(&s), "rock: {s:?} should be off");
        }

        // Phonk без колокола: кик, 808, саб, снейр, хэты, шейкер,
        // тёмные клавиши и формульная тень. Хор — редкий гость.
        let phonk = ensemble_of("phonk");
        for s in [Bass, Kick, Snare, Hat, Keys, Kick808, Shaker, Formula] {
            assert!(phonk.contains(&s), "phonk: {s:?} missing");
        }
        for s in [Pad, Pluck, Laser, Vocal, Vinyl, Flute, Guitar, Rain, Festival, Fuzz] {
            assert!(!phonk.contains(&s), "phonk: {s:?} should be off");
        }

        // Proto-гибрид: худое ядро (кик, саб, снейр, хэты, щипок).
        let proto = ensemble_of("proto");
        for s in [Bass, Kick, Snare, Hat, Pluck] {
            assert!(proto.contains(&s), "proto: {s:?} missing");
        }
        for s in [Pad, Keys, Laser, Vocal, Vinyl, Flute, Guitar, Rain, Festival, Fuzz, Kick808, Shaker, Formula] {
            assert!(!proto.contains(&s), "proto: {s:?} should be off");
        }
    }

    #[test]
    fn flex_lottery_follows_weights() {
        // Взвешенная лотерея: детерминирована сидом, бюджет держится.
        // Пул club — один вокал-хор: сиды чередуют «хор есть / хора нет».
        let style = styles::make_style("club").expect("club");
        let mut empty = 0;
        let mut vocal = 0;
        for seed in 0..64u64 {
            let picked = pick_flex_slots(&*style, seed);
            assert_eq!(picked, pick_flex_slots(&*style, seed), "not deterministic");
            let (lo, hi) = style.flex_range();
            assert!((lo..=hi).contains(&picked.len()), "budget broken");
            for s in &picked {
                assert!(style.flex_pool().contains(s), "stranger guest: {s:?}");
            }
            if picked.is_empty() {
                empty += 1;
            } else {
                vocal += 1;
            }
        }
        // Оба исхода выпадают: гость — специя, а не константа.
        assert!(empty > 0 && vocal > 0, "lottery stuck: empty={empty} vocal={vocal}");
    }

    #[test]
    fn mixes_match_whitelists_with_base_volumes() {
        // Микс = вайтлист + гости сида, громкости канонические.
        // Гости — только из пула стиля, остальные строго в нуле.
        for name in styles::style_names() {
            let style = styles::make_style(name).expect("known style");
            let mut mixer = MixerState::new();
            apply_style_mix(&*style, &mut mixer, 7);
            let picked = pick_flex_slots(&*style, 7);
            for slot in Slot::ALL {
                let vol = mixer.volume_for(slot.id());
                let expected = style.ensemble().contains(&slot) || picked.contains(&slot);
                if expected {
                    assert_eq!(vol, slot.base_volume(), "{name}: {slot:?} volume");
                } else {
                    assert_eq!(vol, 0.0, "{name}: {slot:?} should be muted");
                }
            }
        }
    }

    #[test]
    fn flex_guests_depend_on_seed() {
        // Гости меняются от сида (но всегда из пула), ядро стабильно.
        use std::collections::HashSet;
        let style = styles::make_style("club").expect("club");
        let core: HashSet<Slot> = style.ensemble().iter().copied().collect();
        let mut variants = HashSet::new();
        for seed in 0..16u64 {
            let mut mixer = MixerState::new();
            apply_style_mix(&*style, &mut mixer, seed);
            let audible: HashSet<Slot> = Slot::ALL
                .iter()
                .copied()
                .filter(|s| mixer.volume_for(s.id()) > 0.0)
                .collect();
            // Ядро всегда на месте.
            assert!(audible.is_superset(&core), "seed {seed}: core missing");
            // Вне ядра и пула — никого.
            for s in &audible {
                assert!(
                    core.contains(s) || style.flex_pool().contains(s),
                    "seed {seed}: stranger {s:?}"
                );
            }
            let guests: Vec<Slot> = audible.difference(&core).copied().collect();
            // Число гостей — в бюджете стиля, состав — из пула.
            let (lo, hi) = style.flex_range();
            let max = hi.min(style.flex_pool().len());
            let min = lo.min(max);
            assert!(
                (min..=max).contains(&guests.len()),
                "seed {seed}: {} guests outside {min}..={max}",
                guests.len()
            );
            variants.insert(guests);
        }
        assert!(variants.len() > 1, "guests never vary");
    }
}
