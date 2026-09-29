//! Стили как данные-направления, а не партитуры.
//!
//! Каждый стиль объявляет: лад, темп, свинг, дефолтный микс по слотам
//! и по одной направляющей [`RoleGuide`] на слот. Как именно из весов
//! родятся ноты, решает общий движок [`render_style`]: вес 1.0 —
//! структурный якорь жанра (всегда играет), веса между — вероятности,
//! масштабируемые энергией секции. Никаких номеров шагов в коде стилей.

pub mod club;
pub mod lofi;
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
/// `weights` — веса шагов такта (длина = steps_per_measure стиля):
/// 1.0 — якорь (звучит всегда, формирует лицо жанра),
/// 0.0 — никогда, между — порог фиксированного гейта сида.
/// Итоговая вероятность: `w * energy² * energy_response (+ climax_boost)`.
/// Гейт НЕ кубик: шаг звучит, если хэш(сид, слот, шаг, секция) ниже порога.
/// Тот же сид — тот же грув такт в такт; другой сид — другой грув.
/// Квадрат энергии держит куплеты разреженными, а дроп — плотным.
/// `alt` — второй полюс морфинга (другой грув той же роли).
/// `poles` — набор из 2–4 готовых грувов (бит!): сид выбирает один
/// полюс на весь трек. Разные сиды — разный бит при том же стиле.
/// Если полюса заданы, `weights` игнорируется (морфинг идет к `alt`,
/// если он есть, иначе полюс статичен).
/// Направление морфинга — битом хэша сида: сиды дышат по-разному,
/// внутри сида дыхание стабильно. Нет `alt` = статичный паттерн.
/// Итоговая вероятность: `w * energy² * energy_response (+ climax_boost)`.
/// Квадрат энергии держит куплеты разреженными, а дроп — плотным.
#[derive(Clone, Copy)]
pub struct RoleGuide {
    pub weights: &'static [f32],
    pub alt: Option<&'static [f32]>,
    pub poles: Option<&'static [&'static [f32]]>,
    pub energy_response: f32,
    /// Всплеск вероятности в последнем такте секции (роллы, заливки).
    pub climax_boost: f32,
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
}

impl RoleGuide {
    /// Пустая направляющая: слот молчит и не трогает RNG.
    pub const SILENT: Self = Self {
        weights: &[],
        alt: None,
        poles: None,
        energy_response: 0.0,
        climax_boost: 0.0,
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
    };

    pub fn is_silent(&self) -> bool {
        self.weights.iter().all(|&w| w <= 0.0)
    }

    /// Заготовка ударного: фиксированный питч, остальное по дефолту.
    /// Вызывается с перекрытиями (`..RoleGuide::kit(...)`).
    pub const fn kit(weights: &'static [f32], pitch: u8, vel: f32) -> Self {
        Self {
            weights,
            alt: None,
            poles: None,
            energy_response: 1.0,
            climax_boost: 0.0,
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
            alt: None,
            poles: None,
            energy_response: 1.0,
            climax_boost: 0.0,
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
pub fn render_style(style: &dyn Style, sctx: &mut StepContext) -> Vec<NoteEvent> {
    let guides = style.slot_guides();
    let scale = style.scale();
    let energy = (style.shape_energy(sctx.song.energy) * sctx.intensity).clamp(0.0, 1.5);
    let mut notes = Vec::new();
    for (i, slot) in Slot::ALL.iter().enumerate() {
        if !sctx.active_tracks.contains(&slot.id()) {
            continue;
        }
        let guide = &guides[i];
        if guide.is_silent() {
            continue;
        }
        // Выбор ритмического варианта — битом хэша сида по номеру слота:
        // детерминирован внутри сида, независим между слотами.
        let alt_pick = ((sctx.song.seed_hash >> i) & 1) == 1;
        notes.extend(play_guide(
            guide,
            scale,
            sctx.song,
            energy,
            alt_pick,
            sctx.step_in_measure,
            style.steps_per_measure(),
            sctx.base_step_secs,
            slot.id(),
            i,
            sctx.rng,
        ));
    }
    notes
}

/// Затухание тонов аккорда: корень в полную силу, верх тише.
const CHORD_ATTEN: [f32; 4] = [1.0, 0.75, 0.65, 0.6];

/// Prior участия акцентных слотов в визитах секций (остальные — всегда).
/// Крэш, лазеры, гимны и чопы — эффекты: их отсутствие в части стыков
/// и есть разнообразие сидов, а не поломка.
fn accent_prior(instrument_id: usize) -> f32 {
    match Slot::from_id(instrument_id) {
        Some(Slot::Crash) => 0.5,
        Some(Slot::Laser) => 0.7,
        Some(Slot::Festival) => 0.75,
        Some(Slot::Vocal) => 0.75,
        _ => 1.0,
    }
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
/// Детерминированный гейт вероятности: тот же (сид, слот, шаг, секция) —
/// то же решение, такт в такт, круг в круг. В отличие от кубика на каждый
/// шаг, грув фиксирован и повторяется идеально (как в электронной музыке),
/// а разные сиды дают разные зафиксированные грувы.
fn fixed_gate(seed_hash: u64, slot_idx: usize, step: usize, salt: u64) -> f32 {
    let h = crate::arranger::splitmix64(
        seed_hash
            ^ (slot_idx as u64).wrapping_mul(0x9E3779B9)
            ^ (step as u64).wrapping_mul(0x85EBCA6B)
            ^ salt,
    );
    (h >> 32) as f32 / (u32::MAX as f32)
}

#[allow(clippy::too_many_arguments)]
fn play_guide(
    guide: &RoleGuide,
    scale: &[u8],
    song: &SongContext,
    energy: f32,
    alt_pick: bool,
    step_in_measure: usize,
    steps_per_measure: usize,
    base_step_secs: f32,
    instrument_id: usize,
    slot_idx: usize,
    rng: &mut StdRng,
) -> Vec<NoteEvent> {
    // Полюс грува от сида: один из 2–4 готовых битов на весь трек.
    // Разные сиды — разный бит при том же стиле.
    let base_w = guide.weights.get(step_in_measure).copied().unwrap_or(0.0);
    let base_w = if let Some(poles) = guide.poles.filter(|p| !p.is_empty()) {
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
    // Морфинг паттерна внутри секции: вес плывет от одного полюса
    // к другому по мере прогресса — партия «раскрывается» к концу,
    // а не крутится по кругу. Направление (base→alt или alt→base) —
    // битом сида: сиды дышат по-разному.
    let alt_w = guide
        .alt
        .and_then(|a| a.get(step_in_measure).copied())
        .unwrap_or(base_w);
    let (from_w, to_w) = if alt_pick {
        (alt_w, base_w)
    } else {
        (base_w, alt_w)
    };
    let morph = song.progress.clamp(0.0, 1.0);
    let weight = from_w + (to_w - from_w) * morph;
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
    // Акценты звучат не в каждой секции, а в некоторых ее визитах:
    // крэш — каждый второй стык, лазеры/гимны/чопы — чаще, но тоже
    // с пропусками. Конкретный выбор — гейтом сида от номера визита,
    // поэтому это вариация сида, а не долбежка из раза в раз.
    // Фундамент (барабаны, бас, пэды, гитара) играет всегда.
    let prior = accent_prior(instrument_id);
    if prior < 1.0
        && fixed_gate(song.seed_hash, slot_idx, song.stage % 64, 0xA66E47) >= prior
    {
        return Vec::new();
    }

    let probability = if weight >= 1.0 {
        1.0
    } else {
        // Плотность сида масштабирует все вероятности: разреженные
        // и густые сиды одного стиля. Якоря (1.0) не трогаем.
        let mut p = weight * energy * energy * guide.energy_response * song.density;
        if climax {
            p += guide.climax_boost;
        }
        p.clamp(0.0, 1.0)
    };
    // Якоря (1.0) бьют всегда. Остальное — фиксированным гейтом сида:
    // решение зависит только от (сид, слот, шаг, секция), поэтому грув
    // повторяется такт в такт, а не рассыпается случайными нотами.
    if probability < 1.0
        && fixed_gate(song.seed_hash, slot_idx, step_in_measure, song.section as u64)
            >= probability
    {
        return Vec::new();
    }

    let mut velocity = (guide.vel_base
        + energy * guide.vel_energy
        + rng.gen_range(-guide.vel_jitter..=guide.vel_jitter))
    .clamp(0.1, 1.0);

    // Роль слота: квинтовый бас, аккорды по умолчанию, баланс партии.
    let role = Slot::from_id(instrument_id).map(|s| s.role());

    // Оффбитные 8-е: шаг q/2 внутри четверти (q = шагов на четверть).
    let quarter = steps_per_measure / 4;
    let long = guide.long_offbeats && quarter > 0 && step_in_measure % quarter == quarter / 2;
    let duration = base_step_secs * guide.duration_mult * if long { guide.long_mult } else { 1.0 };

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
            let prog_shift = if guide.follow_motif {
                0
            } else {
                song.prog[(song.measure_index / 2) % song.prog.len()] as usize
            };
            // Вопрос-ответ: нечетные фразы отвечают транспозицией темы
            // на терцию вверх — та же тема, другое дыхание.
            let answer = if guide.follow_motif && song.phrase_index % 2 == 1 {
                2
            } else {
                0
            };
            let (degree, jitter) = if guide.follow_motif {
                let motif_step = song.motif[(step_in_measure / 2) % song.motif.len()] as usize;
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
                // Блуждание тоже фиксировано сидом: та же нота в том же
                // месте каждого такта, у других сидов — другая.
                (crate::arranger::splitmix64(
                    song.seed_hash
                        ^ (slot_idx as u64).wrapping_mul(0xC0FFEE)
                        ^ (step_in_measure as u64).wrapping_mul(0x9E3779B9)
                        ^ 0xD1F7,
                ) % (jitter as u64 + 1)) as usize
            } else {
                0
            };
            let base = song.phrase_index + degree + drift + rotation;
            // Октава общая для тонов аккорда; подъем проверяем по корню.
            let root_idx = base % scale.len();
            let root = modal_tone(scale, root_idx, song.seed_hash);
            let bump = guide.octave_up > 0.0
                // Потолок регистра лидов: выше D6 (86) — ледяной свист,
                // а не музыка. Берем минимум из гайда и колпака.
                && root.saturating_add(12) <= guide.top_pitch.min(86)
                && fixed_gate(song.seed_hash, slot_idx, step_in_measure, 0x0C74)
                    < guide.octave_up;
            (base, octave + if bump { 1 } else { 0 })
        }
    };

    // Аккордовая озвучка: пэды и клавиши берут трезвучие (или интервалы
    // из гайда), мелодия — одиночные ноты. Все тоны в ладу по построению.
    let intervals: &[i8] = match guide.chord {
        Some(iv) => iv,
        None if matches!(role, Some(InstrumentRole::Chords)) && !guide.follow_motif => &[0, 2, 4],
        _ => &[0],
    };
    let mut out = Vec::with_capacity(intervals.len());
    for (ti, iv) in intervals.iter().enumerate() {
        let idx = (tone_base as i32 + *iv as i32).rem_euclid(scale.len() as i32) as usize;
        let tone = modal_tone(scale, idx, song.seed_hash)
            .saturating_add_signed(octaves.saturating_mul(12));
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
        if v >= 0.05 {
            out.push(NoteEvent::new(instrument_id, tone, v, duration));
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
    use rock::RockStyle;
    match name.trim().to_lowercase().as_str() {
        "club" => Some(Box::new(ClubStyle::new())),
        "lofi" => Some(Box::new(LofiStyle::new())),
        "rock" => Some(Box::new(RockStyle::new())),
        _ => None,
    }
}

/// Все доступные стили: имена для CLI и Flutter.
pub fn style_names() -> &'static [&'static str] {
    &["club", "rock", "lofi"]
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
            prog: [0, 0, 0, 0],
        }
    }

    const MORPH_BASE: &[f32] = &[0.2; 16];
    const MORPH_ALT: &[f32] = &[0.8; 16];

    fn morph_hits(progress: f32, alt_pick: bool) -> usize {
        let guide = RoleGuide {
            weights: MORPH_BASE,
            alt: Some(MORPH_ALT),
            ..RoleGuide::kit(MORPH_BASE, 36, 0.9)
        };
        let song = test_song(progress);
        let mut rng = rand::SeedableRng::seed_from_u64(7);
        // Гейт фиксирован сидом: считаем РАЗНЫЕ шаги (0..16), а не повторы.
        (0..16)
            .filter(|step| {
                !play_guide(
                    &guide, &[48], &song, 1.0, alt_pick, *step, 16, 0.1, 2, 0, &mut rng,
                )
                .is_empty()
            })
            .count()
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
        // Веса и альтернативы обязаны совпадать с сеткой стиля,
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
                if let Some(alt) = guide.alt {
                    assert_eq!(alt.len(), spm, "{name} slot {i}: alt len");
                }
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
    fn morph_opens_pattern_toward_section_end() {
        // base 0.2 → alt 0.8: в начале секции проходит мало шагов,
        // в конце — почти все. Обратное направление — зеркально.
        // Гейты детерминированы: числа фиксированы, флейка нет.
        let early_fwd = morph_hits(0.0, false);
        let late_fwd = morph_hits(1.0, false);
        assert!(late_fwd > early_fwd, "{early_fwd} vs {late_fwd}");
        assert!(late_fwd >= 10, "morph does not open: {late_fwd}");
        assert!(early_fwd <= 6, "morph leaks early: {early_fwd}");
        let early_rev = morph_hits(0.0, true);
        let late_rev = morph_hits(1.0, true);
        assert!(early_rev > late_rev, "{early_rev} vs {late_rev}");
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
                    false,
                    step * 2,
                    16,
                    0.1,
                    6,
                    5,
                    &mut rng,
                )
                .into_iter()
                .next()
                .map(|n| n.midi_pitch)
            })
            .collect()
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
                    &guide, &[48], &song, 1.0, false, *step, 8, 0.1, 2, slot_idx, &mut rng,
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
        let notes = play_guide(&guide, &scale, &song, 1.0, false, 0, 8, 0.1, 3, 2, &mut rng);
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
        // Крэш с prior 0.5: за 17 визитов он есть не везде и не нигде —
        // вариация сида, а не долбежка и не тишина.
        let guide = RoleGuide {
            climax_only: true,
            ..RoleGuide::kit(&[1.0; 8], 49, 0.8)
        };
        let mut hits = 0;
        for stage in 0..17 {
            let mut song = test_song(0.9);
            song.is_last_measure = true;
            song.stage = stage;
            let mut rng = rand::SeedableRng::seed_from_u64(5);
            // id 14 = Crash, индекс слота 13 (см. Slot::ALL).
            if !play_guide(&guide, &[48], &song, 1.0, false, 0, 8, 0.1, 14, 13, &mut rng)
                .is_empty()
            {
                hits += 1;
            }
        }
        assert!(hits > 0 && hits < 17, "crash visits: {hits}/17");
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
    fn poles_pick_groove_by_seed() {
        // Один полюс на весь трек: шаги только из POLE_A или только из POLE_B.
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
            play_guide(&guide, &scale, &song, 1.0, false, 0, 8, 0.1, 7, 6, &mut rng)
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
}
