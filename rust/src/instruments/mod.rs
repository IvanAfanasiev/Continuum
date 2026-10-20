pub mod bass;
pub mod dsp;
pub mod guitar;
pub mod hat;
pub mod keys;
pub mod kick;
pub mod kick808;
pub mod pad;
pub mod pluck;
pub mod rain;
pub mod shaker;
pub mod snare;
pub mod vinyl;
pub mod vocal;

pub use bass::BassGuitar;
pub use guitar::GuitarVoice;
pub use hat::HiHat;
pub use kick::Kick;
pub use kick808::Kick808;
pub use rain::Rain;
pub use shaker::Shaker;
pub use snare::SnareDrum;
pub use vinyl::Vinyl;

/// Роль в аранжировке. Гранулярность под электронику: кик, снейр и хэт
/// получают собственные паттерны в стилях (раньше все ударные делили
/// один `Percussion` и играли партию кика).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InstrumentRole {
    Bass,
    Kick,
    Snare,
    Hat,
    Chords,
    Lead,
    /// Вокальные чопы: свой дилэй-сенд и средний сайдчейн.
    Vocal,
    /// Текстуры (винил, дождь): вне пампа и почти вне эха.
    Texture,
}

impl InstrumentRole {
    /// Индекс роли = дискриминант (порядок объявления).
    /// Используется как индекс колонок таблицы громкостей ступеней.
    pub fn index(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        match self {
            InstrumentRole::Bass => "bass",
            InstrumentRole::Kick => "kick",
            InstrumentRole::Snare => "snare",
            InstrumentRole::Hat => "hat",
            InstrumentRole::Chords => "chords",
            InstrumentRole::Lead => "lead",
            InstrumentRole::Vocal => "vocal",
            InstrumentRole::Texture => "texture",
        }
    }
}

/// Группа инструментов для фронта: обычному пользователю не нужны
/// десяток разновидностей одного инструмента. Одна ручка группы —
/// макро-фейдер поверх слотов (множитель 0..1, дефолт 1.0).
/// Какой конкретный вариант звучит (панч или 808) — решает стиль
/// через свой дефолтный микс; ручка группы переживает смену стиля.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Group {
    Bass,
    Kick,
    Snare,
    Hat,
    Chords,
    Lead,
    Vocal,
    Texture,
}

impl Group {
    /// Индекс группы = дискриминант (порядок объявления): бит маски
    /// слоев аранжировки.
    pub const fn index(self) -> usize {
        self as usize
    }

    pub const ALL: [Group; 8] = [
        Group::Bass,
        Group::Kick,
        Group::Snare,
        Group::Hat,
        Group::Chords,
        Group::Lead,
        Group::Vocal,
        Group::Texture,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Group::Bass => "bass",
            Group::Kick => "kick",
            Group::Snare => "snare",
            Group::Hat => "hat",
            Group::Chords => "chords",
            Group::Lead => "lead",
            Group::Vocal => "vocal",
            Group::Texture => "texture",
        }
    }

    pub fn from_name(name: &str) -> Option<Group> {
        match name.trim().to_lowercase().as_str() {
            "bass" => Some(Group::Bass),
            "kick" => Some(Group::Kick),
            "snare" => Some(Group::Snare),
            "hat" | "hats" => Some(Group::Hat),
            "chords" | "pad" | "pads" => Some(Group::Chords),
            "lead" => Some(Group::Lead),
            "vocal" => Some(Group::Vocal),
            "texture" | "atmosphere" => Some(Group::Texture),
            _ => None,
        }
    }
}
/// Слот ансамбля: стабильная единица адресации для стилей, микшера и UI.
/// Номера ЗАМОРОЖЕНЫ навсегда (явные дискриминанты): удаление слота
/// оставляет дыру, а не сдвигает остальных — ручки фронта, сохраненные
/// миксы и Wad-файлы привязок не гниют. Фронт id не хранит,
/// берет из бэкенда при каждом старте. Порядок в ALL — только порядок
/// гайдов и обхода, на id не влияет.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Slot {
    Bass = 1,
    Kick = 2,
    Pad = 3,
    Snare = 4,
    Hat = 5,
    Pluck = 6,
    Keys = 7,
    Kick808 = 8,
    Vinyl = 10,
    Vocal = 11,
    Guitar = 14,
    Rain = 15,
    Shaker = 17,
    /// Соло-гитара (перебор/длинные линии): тот же DSP-ядро, что рифф,
    /// другой характер (сустейн, верх, лёгкий драйв) и свои направляющие.
    /// Добавлен позже остальных — номер следующий свободный.
    GuitarLead = 20,
}

impl Slot {
    pub const COUNT: usize = 14;
    pub const ALL: [Slot; Slot::COUNT] = [
        Slot::Bass,
        Slot::Kick,
        Slot::Pad,
        Slot::Snare,
        Slot::Hat,
        Slot::Pluck,
        Slot::Keys,
        Slot::Kick808,
        Slot::Vinyl,
        Slot::Vocal,
        Slot::Guitar,
        Slot::Rain,
        Slot::Shaker,
        Slot::GuitarLead,
    ];

    pub fn id(self) -> usize {
        // Явный дискриминант, НЕ позиция в ALL: переживает удаления.
        self as usize
    }

    pub fn from_id(id: usize) -> Option<Slot> {
        Slot::ALL.iter().find(|s| s.id() == id).copied()
    }

    pub fn name(self) -> &'static str {
        match self {
            Slot::Bass => "bass",
            Slot::Kick => "kick",
            Slot::Pad => "pad",
            Slot::Snare => "snare",
            Slot::Hat => "hat",
            Slot::Pluck => "pluck",
            Slot::Keys => "keys",
            Slot::Kick808 => "kick808",
            Slot::Vinyl => "vinyl",
            Slot::Vocal => "vocal",
            Slot::Guitar => "guitar",
            Slot::Rain => "rain",
            Slot::Shaker => "shaker",
            Slot::GuitarLead => "guitar lead",
        }
    }

    pub fn role(self) -> InstrumentRole {
        match self {
            Slot::Bass => InstrumentRole::Bass,
            Slot::Kick => InstrumentRole::Kick,
            Slot::Hat | Slot::Shaker => InstrumentRole::Hat,
            Slot::Pad => InstrumentRole::Chords,
            Slot::Snare => InstrumentRole::Snare,
            Slot::Pluck => InstrumentRole::Lead,
            Slot::Keys => InstrumentRole::Chords,
            Slot::Kick808 => InstrumentRole::Kick,
            Slot::Vinyl => InstrumentRole::Texture,
            Slot::Vocal => InstrumentRole::Vocal,
            Slot::Guitar | Slot::GuitarLead => InstrumentRole::Lead,
            Slot::Rain => InstrumentRole::Texture,
        }
    }

    pub fn group(self) -> Group {
        match self {
            Slot::Bass => Group::Bass,
            Slot::Kick | Slot::Kick808 => Group::Kick,
            Slot::Snare => Group::Snare,
            Slot::Hat | Slot::Shaker => Group::Hat,
            Slot::Pad | Slot::Keys => Group::Chords,
            Slot::Pluck | Slot::Guitar | Slot::GuitarLead => Group::Lead,
            Slot::Vocal => Group::Vocal,
            Slot::Vinyl | Slot::Rain => Group::Texture,
        }
    }

    /// Каноническая громкость инструмента: единый источник уровней.
    /// Стиль решает только ВКЛ/ВЫКЛ (вайтлист), цифры живут здесь.
    /// Фоновые шумы (винил) намеренно тихие — вдвое ниже музыкальных
    /// текстур, иначе забивают микс.
    pub fn base_volume(self) -> f32 {
        match self {
            Slot::Bass => 0.9,
            Slot::Kick => 0.95,
            Slot::Pad => 0.5,
            Slot::Snare => 0.8,
            Slot::Hat => 0.7,
            Slot::Pluck => 0.55,
            Slot::Keys => 0.65,
            Slot::Kick808 => 1.0,
            Slot::Vinyl => 0.25,
            Slot::Vocal => 0.6,
            Slot::Guitar => 0.6,
            Slot::Rain => 0.2,
            Slot::Shaker => 0.55,
            Slot::GuitarLead => 0.6,
        }
    }
}

#[derive(Clone, Debug, Copy)]
pub struct NoteEvent {
    pub instrument_id: usize,
    pub midi_pitch: u8,
    pub velocity: f32,
    pub duration_secs: f32,
    /// Множители атаки и релиза от сида (1.0 = характер инструмента).
    /// Читают только мягкие голоса (бас, пэд, клавиши): ударные играют
    /// как написано всегда. Диапазон 0.8..1.8 — характер цел, сиды разные.
    pub attack_mul: f32,
    pub release_mul: f32,
    /// Саб-перекос 0..1 от стиля: 1.0 = чистый синус без струны/щелчка.
    /// Читает только бас-голос.
    pub sub_bias: f32,
    /// Due-планирование насоса: глобальный шаг ноты и длительность
    /// шага в микросекундах. Насос стреляет нотой точно в ее медиакадр,
    /// а не когда разобрал очередь. Нули = legacy (стрелять сразу):
    /// так строят ноты тесты и прямые вызовы движка.
    pub due_step: u64,
    pub step_us: u32,
}

impl NoteEvent {
    /// Конструктор с санитизацией: velocity в [0, 1], длительность >= 0.
    pub fn new(instrument_id: usize, midi_pitch: u8, velocity: f32, duration_secs: f32) -> Self {
        Self {
            instrument_id,
            midi_pitch,
            velocity: velocity.clamp(0.0, 1.0),
            duration_secs: duration_secs.max(0.0),
            attack_mul: 1.0,
            release_mul: 1.0,
            sub_bias: 0.0,
            due_step: 0,
            step_us: 0,
        }
    }

    /// Артикуляция сида: множители атаки/релиза с клампом в разумное.
    pub fn shaped(self, attack_mul: f32, release_mul: f32) -> Self {
        Self {
            attack_mul: attack_mul.clamp(0.25, 3.0),
            release_mul: release_mul.clamp(0.25, 3.0),
            ..self
        }
    }
}

/// Монофонический процедурный инструмент (кик, снейр, хэт, бас).
/// Ударным и басу монофония положена музыкально: ретриггер глушит
/// предыдущий хит (choke хэта — как у живого инструмента).
///
/// Частота дискретизации задается один раз через `set_sample_rate`,
/// а не пробрасывается в каждый вызов.
pub trait Instrument: Send {
    fn set_sample_rate(&mut self, sample_rate: f32);
    fn trigger(&mut self, note: &NoteEvent);
    fn next_sample(&mut self) -> f32;
    /// Стерео-рендер. Дефолт — моно-дубль (центр): так работают ударные
    /// и бас, низ всегда по центру, как принято в клубной музыке.
    /// Полифонические пулы перекрывают: раскладывают голоса по сторонам.
    fn next_sample_stereo(&mut self) -> (f32, f32) {
        let m = self.next_sample();
        (m, m)
    }
    fn id(&self) -> usize;
    fn role(&self) -> InstrumentRole;
    fn reset(&mut self);
    fn is_active(&self) -> bool;
}

/// Один полифонический голос (пад, клавиши, плак).
/// Конкретные голоса перечислены в [`AnyVoice`]; пул [`VoicePool`]
/// держит их как данные, а не как generic-параметр — так весь граф
/// типов остается конкретным и понятным FFI-парсеру.
pub trait Voice: Send {
    fn set_sample_rate(&mut self, sample_rate: f32);
    fn trigger(&mut self, note: &NoteEvent);
    fn next_sample(&mut self) -> f32;
    fn is_active(&self) -> bool;
    fn reset(&mut self);
    /// Стартовая частота для портаменто: вызывается оберткой до trigger.
    /// Дефолт — игнор (портаменто нужно единицам, как 808-слайды).
    fn set_glide_start(&mut self, _freq_hz: f32) {}
    /// Легато-переход на новую ноту без перезапуска атаки/фазы:
    /// длинная линия не перебивает сама себя. Дефолт — обычный
    /// ретриггер (ударным нужна новая атака, а не глисс).
    /// `has_legato` отличает свой глисс от дефолта: обертка идет
    /// коротким путем (без хвоста) только со своим.
    fn legato(&mut self, note: &NoteEvent) {
        self.trigger(note);
    }
    fn has_legato(&self) -> bool {
        false
    }
}

use keys::KeysVoice;
use pad::PadVoice;
use pluck::PluckVoice;
use vocal::VocalVoice;

/// Любой голос полифонического пула. Enum вместо generic-параметра:
/// конкретный тип в каждом варианте, динамическая диспетчеризация
/// без `dyn` в полях структур.
#[derive(Clone)]
pub(crate) enum AnyVoice {
    Pad(PadVoice),
    Pluck(PluckVoice),
    Keys(KeysVoice),
    Vocal(VocalVoice),
    Guitar(GuitarVoice),
}

impl Voice for AnyVoice {
    fn set_sample_rate(&mut self, sample_rate: f32) {
        match self {
            AnyVoice::Pad(v) => v.set_sample_rate(sample_rate),
            AnyVoice::Pluck(v) => v.set_sample_rate(sample_rate),
            AnyVoice::Keys(v) => v.set_sample_rate(sample_rate),
            AnyVoice::Vocal(v) => v.set_sample_rate(sample_rate),
            AnyVoice::Guitar(v) => v.set_sample_rate(sample_rate),
        }
    }

    fn trigger(&mut self, note: &NoteEvent) {
        match self {
            AnyVoice::Pad(v) => v.trigger(note),
            AnyVoice::Pluck(v) => v.trigger(note),
            AnyVoice::Keys(v) => v.trigger(note),
            AnyVoice::Vocal(v) => v.trigger(note),
            AnyVoice::Guitar(v) => v.trigger(note),
        }
    }

    fn next_sample(&mut self) -> f32 {
        match self {
            AnyVoice::Pad(v) => v.next_sample(),
            AnyVoice::Pluck(v) => v.next_sample(),
            AnyVoice::Keys(v) => v.next_sample(),
            AnyVoice::Vocal(v) => v.next_sample(),
            AnyVoice::Guitar(v) => v.next_sample(),
        }
    }

    fn is_active(&self) -> bool {
        match self {
            AnyVoice::Pad(v) => v.is_active(),
            AnyVoice::Pluck(v) => v.is_active(),
            AnyVoice::Keys(v) => v.is_active(),
            AnyVoice::Vocal(v) => v.is_active(),
            AnyVoice::Guitar(v) => v.is_active(),
        }
    }

    fn reset(&mut self) {
        match self {
            AnyVoice::Pad(v) => v.reset(),
            AnyVoice::Pluck(v) => v.reset(),
            AnyVoice::Keys(v) => v.reset(),
            AnyVoice::Vocal(v) => v.reset(),
            AnyVoice::Guitar(v) => v.reset(),
        }
    }
}

/// Перекрест сторон в пуле: 0 = жесткое L/R, 1 = моно.
const STEREO_BLEED: f32 = 0.35;
/// Хвост кроссфейда при ретриггере/краже: предыдущий хит гаснет
/// поверх нового удара вместо ступеньки-щелчка.
const TAIL_FADE_SECS: f32 = 0.008;

fn tail_decay_for(sample_rate: f32) -> f32 {
    (-1.0 / (sample_rate.max(1.0) * TAIL_FADE_SECS)).exp()
}

/// Моно-голос с кроссфейдом хвоста: ретриггер не рубит предыдущий хит
/// ступенькой (главный источник щелчков на частых повторах), а гасит
/// его за ~8 мс поверх свежего удара. Choke-логика (хэт!) сохраняется:
/// старый хит все равно глушится, но гладко.
pub struct Mono<V: Voice + Default + Clone> {
    id: usize,
    role: InstrumentRole,
    main: V,
    tail: V,
    tail_gain: f32,
    tail_decay: f32,
    sample_rate: f32,
    /// Высота звучащей ноты: повтор той же высоты — свежая атака
    /// (артикуляция повтора), смена высоты на legato-голосе — глисс
    /// без перезапуска (длинная линия не перебивает сама себя).
    pitch: Option<u8>,
}

impl<V: Voice + Default + Clone> Mono<V> {
    pub fn new(id: usize, role: InstrumentRole) -> Self {
        let sample_rate = 44_100.0;
        Self {
            id,
            role,
            main: V::default(),
            tail: V::default(),
            tail_gain: 0.0,
            tail_decay: tail_decay_for(sample_rate),
            sample_rate,
            pitch: None,
        }
    }

    fn render_tail(&mut self) -> f32 {
        if self.tail_gain <= 0.0 {
            return 0.0;
        }
        let out = self.tail.next_sample() * self.tail_gain;
        self.tail_gain *= self.tail_decay;
        if self.tail_gain < 0.001 || !self.tail.is_active() {
            self.tail_gain = 0.0;
        }
        out
    }

    /// Триггер с настройкой свежего голоса до старта ноты
    /// (портаменто 808: откуда слайдить). Хвост — как обычно.
    pub fn trigger_with_setup(&mut self, note: &NoteEvent, setup: impl FnOnce(&mut V)) {
        let mut fresh = V::default();
        fresh.set_sample_rate(self.sample_rate);
        setup(&mut fresh);
        self.tail = std::mem::replace(&mut self.main, fresh);
        self.tail_gain = 1.0;
        self.main.trigger(note);
        self.pitch = Some(note.midi_pitch);
    }
}

impl<V: Voice + Default + Clone> Instrument for Mono<V> {
    fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sane_sample_rate(sample_rate);
        self.tail_decay = tail_decay_for(self.sample_rate);
        self.main.set_sample_rate(self.sample_rate);
        self.tail.set_sample_rate(self.sample_rate);
    }

    fn trigger(&mut self, note: &NoteEvent) {
        // Легато: звонкий голос берет новую ВЫСОТУ без перезапуска —
        // линия течет глиссом, а не рубится на каждой восьмой.
        // Та же высота — свежая атака (повтор артикулируется).
        // Голоса без своего legato идут прежним путем с хвостом.
        if self.main.is_active() && self.main.has_legato() && self.pitch != Some(note.midi_pitch) {
            self.main.legato(note);
            self.pitch = Some(note.midi_pitch);
            return;
        }
        // Старое состояние уходит в хвост с полным гейном, новый удар
        // начинается с чистой атаки — сумма непрерывна, ступеньки нет.
        let mut fresh = V::default();
        fresh.set_sample_rate(self.sample_rate);
        self.tail = std::mem::replace(&mut self.main, fresh);
        self.tail_gain = 1.0;
        self.main.trigger(note);
        self.pitch = Some(note.midi_pitch);
    }

    fn next_sample(&mut self) -> f32 {
        self.main.next_sample() + self.render_tail()
    }

    fn id(&self) -> usize {
        self.id
    }

    fn role(&self) -> InstrumentRole {
        self.role
    }

    fn reset(&mut self) {
        self.main.reset();
        self.tail.reset();
        self.tail_gain = 0.0;
    }

    fn is_active(&self) -> bool {
        self.main.is_active() || self.tail_gain > 0.0
    }
}

/// Пул голосов с кражей старейшего. Аккорды и налегающие ноты
/// звучат одновременно вместо обрывания друг друга.
/// Конструкторы `pads`/`plucks`/`keys` задают и тембр, и роль.
pub struct VoicePool {
    id: usize,
    role: InstrumentRole,
    voices: Vec<AnyVoice>,
    age: Vec<u64>,
    clock: u64,
    tail: Option<AnyVoice>,
    tail_gain: f32,
    tail_decay: f32,
    sample_rate: f32,
}

impl VoicePool {
    fn with_voices(id: usize, role: InstrumentRole, voices: Vec<AnyVoice>) -> Self {
        let age = vec![0u64; voices.len()];
        let sample_rate = 44_100.0;
        Self {
            id,
            role,
            voices,
            age,
            clock: 0,
            tail: None,
            tail_gain: 0.0,
            tail_decay: tail_decay_for(sample_rate),
            sample_rate,
        }
    }

    pub fn pads(id: usize, count: usize) -> Self {
        Self::with_voices(
            id,
            InstrumentRole::Chords,
            (0..count)
                .map(|_| AnyVoice::Pad(PadVoice::default()))
                .collect(),
        )
    }

    pub fn plucks(id: usize, count: usize) -> Self {
        Self::with_voices(
            id,
            InstrumentRole::Lead,
            (0..count)
                .map(|_| AnyVoice::Pluck(PluckVoice::default()))
                .collect(),
        )
    }

    pub fn keys(id: usize, count: usize) -> Self {
        Self::with_voices(
            id,
            InstrumentRole::Chords,
            (0..count)
                .map(|_| AnyVoice::Keys(KeysVoice::default()))
                .collect(),
        )
    }

    pub fn vocals(id: usize, count: usize) -> Self {
        Self::with_voices(
            id,
            InstrumentRole::Vocal,
            (0..count)
                .map(|_| AnyVoice::Vocal(VocalVoice::default()))
                .collect(),
        )
    }

    pub fn guitars(id: usize, count: usize) -> Self {
        Self::with_voices(
            id,
            InstrumentRole::Lead,
            (0..count)
                .map(|_| AnyVoice::Guitar(GuitarVoice::default()))
                .collect(),
        )
    }

    /// Соло-гитара (перебор/длинные линии): то же DSP-ядро, характер Lead
    /// (сустейн, верх, лёгкий драйв). Отдельный слот, т.к. у соло свой
    /// ритм, регистр и огибающие — и оно звучит одновременно с риффом.
    pub fn guitar_leads(id: usize, count: usize) -> Self {
        Self::with_voices(
            id,
            InstrumentRole::Lead,
            (0..count)
                .map(|_| AnyVoice::Guitar(GuitarVoice::lead()))
                .collect(),
        )
    }

    /// Формульный лид: функция времени вместо осциллятора.
    /// Вейвтейбл-лид: круговая волна из таблицы вместо осциллятора.
    fn steal_or_free(&mut self) -> (usize, bool) {
        self.clock += 1;
        for (i, voice) in self.voices.iter().enumerate() {
            if !voice.is_active() {
                self.age[i] = self.clock;
                return (i, false);
            }
        }
        let mut oldest = 0;
        for i in 1..self.voices.len() {
            if self.age[i] < self.age[oldest] {
                oldest = i;
            }
        }
        self.age[oldest] = self.clock;
        (oldest, true)
    }

    fn render_tail(&mut self) -> f32 {
        if self.tail_gain <= 0.0 {
            return 0.0;
        }
        match self.tail.as_mut() {
            Some(tail) => {
                let out = tail.next_sample() * self.tail_gain;
                self.tail_gain *= self.tail_decay;
                if self.tail_gain < 0.001 || !tail.is_active() {
                    self.tail_gain = 0.0;
                }
                out
            }
            None => {
                self.tail_gain = 0.0;
                0.0
            }
        }
    }
}

impl Instrument for VoicePool {
    fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sane_sample_rate(sample_rate);
        self.tail_decay = tail_decay_for(self.sample_rate);
        for voice in &mut self.voices {
            voice.set_sample_rate(self.sample_rate);
        }
    }

    fn trigger(&mut self, note: &NoteEvent) {
        let (idx, stole) = self.steal_or_free();
        if stole {
            // Украденный голос уходит в хвост: обрыв без щелчка.
            self.tail = Some(self.voices[idx].clone());
            self.tail_gain = 1.0;
        }
        self.voices[idx].trigger(note);
    }

    /// Моно — среднее стерео (один источник истины, без двойного рендера).
    fn next_sample(&mut self) -> f32 {
        let (l, r) = self.next_sample_stereo();
        (l + r) * 0.5
    }

    /// Ширина без дублирования: четные голоса влево, нечетные вправо,
    /// плюс перекрестный bleed для моносовместимости. Хвост кражи —
    /// по центру поверх.
    fn next_sample_stereo(&mut self) -> (f32, f32) {
        let mut even = 0.0;
        let mut odd = 0.0;
        for (i, voice) in self.voices.iter_mut().enumerate() {
            let s = voice.next_sample();
            if i.is_multiple_of(2) {
                even += s;
            } else {
                odd += s;
            }
        }
        let tail = self.render_tail();
        (
            even + odd * STEREO_BLEED + tail,
            odd + even * STEREO_BLEED + tail,
        )
    }

    fn id(&self) -> usize {
        self.id
    }

    fn role(&self) -> InstrumentRole {
        self.role
    }

    fn reset(&mut self) {
        for voice in &mut self.voices {
            voice.reset();
        }
        self.tail = None;
        self.tail_gain = 0.0;
        self.clock = 0;
    }

    fn is_active(&self) -> bool {
        self.voices.iter().any(|v| v.is_active()) || self.tail_gain > 0.0
    }
}

/// Защита от нулевой/отрицательной частоты дискретизации.
pub(crate) fn sane_sample_rate(sample_rate: f32) -> f32 {
    if sample_rate.is_finite() && sample_rate > 0.0 {
        sample_rate
    } else {
        44_100.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_event_clamps() {
        let n = NoteEvent::new(1, 60, 5.0, -1.0);
        assert_eq!(n.velocity, 1.0);
        assert_eq!(n.duration_secs, 0.0);
    }

    #[test]
    fn slot_ids_are_frozen_forever() {
        // Номера слотов — вечные: удаление слота оставляет дыру,
        // остальные не сдвигаются. Этот тест прибит гвоздями:
        // любое изменение id здесь — ломающее, требует версионирования.
        use Slot::*;
        let frozen = [
            (Bass, 1),
            (Kick, 2),
            (Pad, 3),
            (Snare, 4),
            (Hat, 5),
            (Pluck, 6),
            (Keys, 7),
            (Kick808, 8),
            (Vinyl, 10),
            (Vocal, 11),
            (Guitar, 14),
            (Rain, 15),
            (Shaker, 17),
            (GuitarLead, 20),
        ];
        assert_eq!(frozen.len(), Slot::COUNT);
        for (slot, id) in frozen {
            assert_eq!(slot.id(), id, "{slot:?} moved!");
            assert_eq!(Slot::from_id(id), Some(slot));
        }
        assert_eq!(Slot::from_id(0), None);
        assert_eq!(Slot::from_id(999), None);
    }

    #[test]
    fn note_event_shape_defaults_to_neutral() {
        // Без shaping артикуляция нейтральна (ударные и старые пути),
        // shaped клампится в разумное и цепляется билдером.
        let n = NoteEvent::new(1, 60, 0.8, 1.0);
        assert_eq!((n.attack_mul, n.release_mul), (1.0, 1.0));
        let s = n.shaped(1.8, 0.8);
        assert_eq!((s.attack_mul, s.release_mul), (1.8, 0.8));
        let wild = n.shaped(99.0, -5.0);
        assert_eq!((wild.attack_mul, wild.release_mul), (3.0, 0.25));
        assert_eq!(n.sub_bias, 0.0);
    }

    #[test]
    fn poly_steals_oldest_without_panic() {
        // Пул из 2 голосов, триггерим 5 перекрывающихся нот —
        // кража старейшего не должна паниковать и глушить все.
        let mut pool = VoicePool::pads(3, 2);
        pool.set_sample_rate(44_100.0);
        for i in 0..5 {
            pool.trigger(&NoteEvent::new(3, 60 + i, 0.8, 1.0));
            assert!(pool.is_active());
        }
        // Сумма голосов конечна и ненулевая.
        let mut sum = 0.0;
        for _ in 0..1000 {
            let s = pool.next_sample();
            assert!(s.is_finite());
            sum += s.abs();
        }
        assert!(sum > 0.0);
    }

    #[test]
    fn poly_goes_quiet_after_notes_end() {
        let mut pool = VoicePool::pads(3, 2);
        pool.set_sample_rate(44_100.0);
        pool.trigger(&NoteEvent::new(3, 60, 0.8, 0.1));
        for _ in 0..((0.5 * 44_100.0) as usize) {
            pool.next_sample();
        }
        assert!(!pool.is_active());
        assert_eq!(pool.next_sample(), 0.0);
    }
}
