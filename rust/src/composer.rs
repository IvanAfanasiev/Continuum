//! Генератор композиции: тактовый цикл, опрашивающий стиль.
//!
//! Петля не владеет музыкальными решениями: стиль дает направляющие,
//! микшер — активные треки, темп — живую ручку BPM. Все три меняются
//! на ходу без перезапуска потока.
//!
//! Лидер инструментов: каждые 2 фразы один из мелодических слотов
//! становится громче (лидер), остальные уходят в поддержку. Барабаны
//! и бас не трогаем — грув стабилен, дышит только верх.

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
use crate::core::{base_step_secs, snapshot, swing_step_secs, MAX_ACTIVE_TRACKS};
use crate::instruments::{InstrumentRole, NoteEvent, Slot};
use crate::mixer::MixerState;
use crate::styles::{render_style, StepContext, Style};

/// Живой темп: `None` = дефолт стиля, `Some(bpm)` = ручка фронта.
pub type LiveTempo = Arc<RwLock<Option<f32>>>;

/// Баланс партий лидера — намеренно сдержанный: генерация не спорит
/// с ручками пользователя, лишь слегка подсвечивает ведущий голос.
const LEADER_BOOST: f32 = 1.08;
const SUPPORT_DIP: f32 = 0.95;

/// Мелодические роли, участвующие в ротации лидера.
fn is_melodic(id: usize) -> bool {
    matches!(
        Slot::from_id(id).map(Slot::role),
        Some(InstrumentRole::Lead | InstrumentRole::Chords | InstrumentRole::Vocal)
    )
}

/// Выбор лидера среди отсортированных кандидатов: детерминирован
/// фразой, меняется каждые 2 фразы. Меньше двух кандидатов — без лидера.
pub(crate) fn pick_leader(candidates: &[usize], phrase_index: usize) -> Option<usize> {
    if candidates.len() < 2 {
        return None;
    }
    Some(candidates[(phrase_index / 2) % candidates.len()])
}

/// Баланс партий: лидер громче, поддержка тише. Барабаны/бас не трогаем.
pub(crate) fn apply_leader_balance(notes: &mut [NoteEvent], leader: Option<usize>) {
    let Some(lead_id) = leader else { return };
    for note in notes.iter_mut() {
        if note.instrument_id == lead_id {
            note.velocity = (note.velocity * LEADER_BOOST).min(1.0);
        } else if is_melodic(note.instrument_id) {
            note.velocity *= SUPPORT_DIP;
        }
    }
}

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
}

/// Номер секции для снимка (порядок объявления SongSection).
pub(crate) fn section_index(section: crate::arranger::SongSection) -> u8 {
    section as u8
}

pub struct Composer {
    queue: Arc<ArrayQueue<NoteEvent>>,
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
    /// Живой снимок для визуалайзера (делится с сессией).
    visual: Arc<RwLock<VisualSnap>>,
    // Состояние генерации (бывшие локальные переменные run):
    rng: StdRng,
    arranger: Arranger,
    song: SongContext,
    current_seed: u64,
    step_count: usize,
    last_style_name: String,
    last_phrase: usize,
    phrase_ticks: u64,
    leader: Option<usize>,
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
            Arc::new(RwLock::new(VisualSnap::default())),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_shared(
        queue: Arc<ArrayQueue<NoteEvent>>,
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
        visual: Arc<RwLock<VisualSnap>>,
    ) -> Self {
        let current_seed = snapshot(&seed);
        let score = match style.read() {
            Ok(g) => g.arrangement(),
            Err(poisoned) => poisoned.into_inner().arrangement(),
        };
        let mut arranger = Arranger::with_score(current_seed, score);
        let song = arranger.advance_measure();
        let last_phrase = song.phrase_index;
        Self {
            queue,
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
            visual,
            rng: StdRng::seed_from_u64(current_seed),
            arranger,
            song,
            current_seed,
            step_count: 0,
            last_style_name: String::new(),
            last_phrase,
            phrase_ticks: 0,
            leader: None,
        }
    }

    fn reset_form(&mut self, seed: u64) {
        // Партитура — от текущего стиля: спокойные идут коротким кругом.
        let score = match self.style.read() {
            Ok(g) => g.arrangement(),
            Err(poisoned) => poisoned.into_inner().arrangement(),
        };
        self.current_seed = seed;
        self.rng = StdRng::seed_from_u64(seed);
        self.arranger = Arranger::with_score(seed, score);
        self.song = self.arranger.advance_measure();
        self.step_count = 0;
        self.last_phrase = self.song.phrase_index;
        self.phrase_ticks = 0;
        self.leader = None;
    }

    /// Один шаг генерации: ноты + длительность до следующего шага.
    /// Чистая механика без сна — используется и петлей, и офлайн-рендером.
    pub fn next_step(&mut self) -> (Vec<NoteEvent>, f32) {
        let mut active_tracks = [0usize; MAX_ACTIVE_TRACKS];
        let active_count = match self.mixer.read() {
            Ok(state) => state.active_ids(&mut active_tracks),
            Err(poisoned) => poisoned.into_inner().active_ids(&mut active_tracks),
        };
        // active_ids уже сортирует — порядок детерминирован.
        // Треки с громкостью 0 сюда не попадают: выключенные слоты
        // не грузят ни генерацию, ни DSP. Срез по ядру дропа —
        // ниже, после чтения стиля.

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
            if !self.last_style_name.is_empty() {
                // Смена стиля начинает новую форму с чистого листа.
                self.reset_form(self.current_seed);
            }
            self.last_style_name = style_name;
        }

        let guard = match self.style.read() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        let style = &**guard;

        let active = &active_tracks[..active_count];

        let steps_per_measure = style.steps_per_measure();
        // Эффективный темп: ручка фронта перекрывает дефолт стиля.
        // Морфинг плавный — шаг пересчитывается каждый такт цикла.
        // Значение публикуем движку для темпового эха.
        let bpm = snapshot(&self.tempo)
            .filter(|bpm| bpm.is_finite() && *bpm > 0.0)
            .unwrap_or_else(|| style.bpm());
        if let Ok(mut share) = self.bpm_out.write() {
            *share = bpm;
        }
        let base_step_secs = base_step_secs(bpm, steps_per_measure);
        let step_in_measure = self.step_count % steps_per_measure;

        if step_in_measure == 0 && self.step_count > 0 {
            self.song = self.arranger.advance_measure();
        }
        // Открытость фильтра — от свежей секции: движок свистит
        // срезом верхов к дропу каждый шаг.
        if let Ok(mut share) = self.filter_open.write() {
            *share = crate::arranger::section_openness(self.song.section, self.song.progress);
        }

        // Мелодические слоты в миксе — кандидаты в лидеры и проверка,
        // что лидер все еще играет. Считаем один раз на шаг.
        let melodic: Vec<usize> = active
            .iter()
            .copied()
            .filter(|id| is_melodic(*id))
            .collect();
        // Ротация лидера: каждые 2 новые фразы — новый лидер верха.
        if self.song.phrase_index != self.last_phrase {
            self.last_phrase = self.song.phrase_index;
            self.phrase_ticks += 1;
            if self.phrase_ticks.is_multiple_of(2) {
                self.leader = pick_leader(&melodic, self.song.phrase_index);
            }
        }

        let mut sctx = StepContext {
            step: self.step_count,
            step_in_measure,
            base_step_secs,
            active_tracks: active,
            song: &self.song,
            rng: &mut self.rng,
            intensity: snapshot(&self.intensity).clamp(0.0, 2.0),
        };
        let mut notes = render_style(&**guard, &mut sctx);

        // Лидер звучит, если все еще в миксе; иначе баланс не трогаем.
        let effective = self.leader.filter(|l| melodic.contains(l));
        apply_leader_balance(&mut notes, effective);

        // Живой снимок для визуалайзера: позиция темы на этом шаге
        // повторяет окно фразы из render_style (шаг/2 + ротация).
        if let Ok(mut snap) = self.visual.write() {
            let rot = self.song.phrase_index % self.song.motif.len().max(1);
            let pos =
                (step_in_measure / 2 + rot) % self.song.motif.len().max(1);
            *snap = VisualSnap {
                section: section_index(self.song.section),
                energy: self.song.energy,
                motif: self.song.motif,
                motif_pos: pos as u8,
                step_phase: step_in_measure as f32
                    / steps_per_measure.max(1) as f32,
            };
        }

        self.step_count += 1;
        // Свинг сида поверх стиля: у каждого сида свой кач.
        let swing = style.swing_ratio() * crate::styles::swing_var(self.song.seed_hash);
        let duration = swing_step_secs(base_step_secs, swing, step_in_measure);
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
            let n = notes.len() as u64;
            for note in notes {
                // Полная очередь: дропаем новейшую ноту, старые не трогаем.
                let _ = self.queue.push(note);
            }
            if n > 0 {
                self.pushed.fetch_add(n, Ordering::Relaxed);
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

    #[test]
    fn leader_rotates_every_two_phrases() {
        let candidates = vec![3usize, 6, 7, 11];
        // Фразы 0,1 → индекс 0; 2,3 → индекс 1; и так далее.
        assert_eq!(pick_leader(&candidates, 0), Some(3));
        assert_eq!(pick_leader(&candidates, 1), Some(3));
        assert_eq!(pick_leader(&candidates, 2), Some(6));
        assert_eq!(pick_leader(&candidates, 5), Some(7));
        // Меньше двух кандидатов — лидера нет, баланс не трогаем.
        assert_eq!(pick_leader(&[3], 4), None);
        assert_eq!(pick_leader(&[], 4), None);
    }

    #[test]
    fn leader_balance_boosts_lead_only() {
        let mut notes = vec![
            NoteEvent::new(2, 36, 0.8, 0.1), // kick: не трогаем
            NoteEvent::new(1, 40, 0.8, 0.2), // bass: не трогаем
            NoteEvent::new(6, 72, 0.5, 0.2), // pluck-лидер: вверх
            NoteEvent::new(7, 60, 0.5, 0.4), // keys-поддержка: вниз
        ];
        apply_leader_balance(&mut notes, Some(6));
        assert_eq!(notes[0].velocity, 0.8);
        assert_eq!(notes[1].velocity, 0.8);
        assert!((notes[2].velocity - 0.54).abs() < 1e-6);
        assert!((notes[3].velocity - 0.475).abs() < 1e-6);
        // Без лидера — ничего не меняется.
        apply_leader_balance(&mut notes, None);
        assert!((notes[2].velocity - 0.54).abs() < 1e-6);
    }
}
