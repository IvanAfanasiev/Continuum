//! Управление воспроизведением для Flutter.
//!
//! Все функции возвращают [`PlaybackStatus`] — актуальный снимок состояния,
//! поэтому Dart-сторона всегда видит факт, а не предположение.
//! Ни одна функция (кроме явного `playback_stop`) не завершает главную
//! петлю: пауза, темп, стиль, сид и громкости меняются наживую.
//!
//! Семантика слотов: громкость > 0 = слот используется в генерации,
//! 0 = исключен (не генерируется и не грузит DSP).

use std::sync::OnceLock;

use crate::playback::{
    EngineStats, GroupInfo, PlaybackController, PlaybackStatus, StyleMeta, TrackInfo,
};

fn controller() -> &'static PlaybackController {
    static CONTROLLER: OnceLock<PlaybackController> = OnceLock::new();
    CONTROLLER.get_or_init(PlaybackController::new)
}

/// Доступ контроллера для нативного аудио-потока (тот же синглтон,
/// те же замки — Dart и поток видят одно состояние).
/// Только Android: больше его никто не зовет.
#[cfg(target_os = "android")]
pub(crate) fn shared_controller() -> &'static PlaybackController {
    controller()
}

pub fn playback_start(style_name: String, seed: u64) -> PlaybackStatus {
    controller().start(&style_name, seed, true)
}

pub fn playback_stop() -> PlaybackStatus {
    controller().stop()
}

pub fn playback_set_paused(paused: bool) -> PlaybackStatus {
    controller().set_paused(paused)
}

pub fn playback_set_style(style_name: String) -> PlaybackStatus {
    controller().set_style(&style_name)
}

pub fn playback_set_seed(seed: u64) -> PlaybackStatus {
    controller().set_seed(seed)
}

/// Сид кодом `XXX-XXX` (алфавит 0-9A-Z, регистр не важен).
/// Мусор выбрасывается, короткий ввод отклоняется без смены сида.
pub fn playback_set_seed_code(code: String) -> PlaybackStatus {
    controller().set_seed_code(&code)
}

/// Случайный сид (кнопка «кубик»): код вернется в статусе.
pub fn playback_random_seed() -> PlaybackStatus {
    controller().random_seed()
}

/// Живой темп в BPM (обычно 75–135, кламп 50–180).
/// Неположительный = сброс к дефолту стиля.
pub fn playback_set_tempo_bpm(bpm: f32) -> PlaybackStatus {
    controller().set_tempo_bpm(bpm)
}

/// Живая intensity 0..2 («температура» генерации).
pub fn playback_set_intensity(intensity: f32) -> PlaybackStatus {
    controller().set_intensity(intensity)
}

/// Громкость слота 0..1. Ноль исключает слот из генерации.
/// Ручная правка помечает список изменённым: смена стиля и сида его
/// больше не перетирает (сброс — отдельной кнопкой).
pub fn playback_set_track(instrument_id: u64, volume: f32) -> PlaybackStatus {
    controller().set_track(instrument_id, volume)
}

/// Сброс списка инструментов на дефолты текущего стиля (вайтлист +
/// гости сида). Снимает пометку «правил слушатель»: дальше стиль снова
/// влияет на состав.
pub fn playback_reset_instruments() -> PlaybackStatus {
    controller().reset_instruments()
}

/// Назначить инструменту чужую партию: «в rock пианино вместо гитары».
/// `line_id` — id слота-источника (партия), `None` — своя партия.
/// Назначение общее для стилей и переживает смену стиля.
pub fn playback_set_line(instrument_id: u64, line_id: Option<u64>) -> PlaybackStatus {
    controller().set_line(instrument_id, line_id)
}

/// Макро-фейдер группы 0..1 («kick», «lead», …): один «Kick» вместо
/// «панч vs 808» — конкретный вариант под капотом выбирает стиль.
/// Ноль глушит группу целиком. Переживает смену стиля.
pub fn playback_set_group_volume(group_name: String, volume: f32) -> PlaybackStatus {
    controller().set_group_volume(&group_name, volume)
}

/// Слоты микшера для UI: id, имена, роли, группы, слышимые громкости.
pub fn playback_tracks() -> Vec<TrackInfo> {
    controller().tracks()
}

/// Макро-фейдеры для UI: имя группы и ее множитель.
pub fn playback_groups() -> Vec<GroupInfo> {
    controller().groups()
}

pub fn playback_status() -> PlaybackStatus {
    controller().status()
}

/// Диагностика живого тракта (кадры/ноты/очередь) для UI-баннера.
pub fn playback_engine_stats() -> EngineStats {
    controller().engine_stats()
}

/// Старт pull-сессии для фронтенда (Android играет сам):
/// композер крутится, аудио-потока нет, сэмплы забирают по запросу.
pub fn playback_pull_start(style_name: String, seed: u64) -> PlaybackStatus {
    controller().start(&style_name, seed, false)
}

/// Pull-блок для фронтенда: стерео interleaved f32.
/// Вызывать циклично мелкими блоками (~10 раз в секунду — онсеты
/// точнее шага сетки), темп задает потребитель.
pub fn playback_pull_render(frames: u64, sample_rate: f32) -> Vec<f32> {
    controller().render_pull(frames, sample_rate)
}

/// Нативный аудио-поток (Android): сессия + рендер + запись в трек
/// целиком в Rust, Dart шлет только команды. Статус — для UI.
pub fn playback_pull_thread_start(style_name: String, seed: u64) -> PlaybackStatus {
    let status = controller().start(&style_name, seed, false);
    crate::pull_thread::thread_start();
    status
}

/// Останов нативного потока. Сессия живет (пауза держится бэкендом).
pub fn playback_pull_thread_stop() -> PlaybackStatus {
    crate::pull_thread::thread_stop();
    controller().status()
}

/// Жив ли нативный поток (для UI и самодиагностики).
pub fn playback_pull_alive() -> bool {
    crate::pull_thread::thread_alive()
}

/// Захват потока в файл (30 секунд, экспертиза скачков).
/// Путь — публичная папка приложения снаружи.
pub fn playback_pull_capture(path: String) -> bool {
    crate::pull_thread::capture_start(&path)
}

/// Журнал нативного потока для экрана диагностики.
pub fn playback_pull_diag() -> Vec<String> {
    crate::pull_thread::diag_lines()
}

/// Кадр визуалайзера из живой сессии. Без сессии — нули.
pub fn playback_live_visual() -> crate::playback::LiveVisual {
    controller().live_visual()
}

pub fn playback_style_names() -> Vec<String> {
    crate::styles::style_names()
        .iter()
        .map(|s| s.to_string())
        .collect()
}

/// Стили с полками каталога для сортировки фронта.
pub fn playback_styles() -> Vec<StyleMeta> {
    crate::styles::style_names()
        .iter()
        .filter_map(|name| {
            crate::styles::make_style(name).map(|s| StyleMeta {
                name: s.name().to_string(),
                group: s.group().name().to_string(),
            })
        })
        .collect()
}
