pub mod acid;
pub mod bass;
pub mod crash;
pub mod dsp;
pub mod festival;
pub mod flute;
pub mod fuzz;
pub mod guitar;
pub mod hat;
pub mod keys;
pub mod kick;
pub mod kick808;
pub mod laser;
pub mod pad;
pub mod pluck;
pub mod rain;
pub mod snare;
pub mod vinyl;
pub mod vocal;

pub use acid::Acid;
pub use bass::BassGuitar;
pub use crash::Crash;
pub use fuzz::FuzzVoice;
pub use guitar::GuitarVoice;
pub use hat::HiHat;
pub use kick::Kick;
pub use kick808::Kick808;
pub use rain::Rain;
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
/// id идут по порядку объявления (1-based) — фронт их не хранит,
/// берет из бэкенда при каждом старте.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Slot {
    Bass,
    Kick,
    Pad,
    Snare,
    Hat,
    Pluck,
    Keys,
    Kick808,
    Laser,
    Vinyl,
    Vocal,
    Flute,
    Festival,
    Crash,
    Guitar,
    Rain,
    Fuzz,
    Acid,
}

impl Slot {
    pub const COUNT: usize = 18;
    pub const ALL: [Slot; Slot::COUNT] = [
        Slot::Bass,
        Slot::Kick,
        Slot::Pad,
        Slot::Snare,
        Slot::Hat,
        Slot::Pluck,
        Slot::Keys,
        Slot::Kick808,
        Slot::Laser,
        Slot::Vinyl,
        Slot::Vocal,
        Slot::Flute,
        Slot::Festival,
        Slot::Crash,
        Slot::Guitar,
        Slot::Rain,
        Slot::Fuzz,
        Slot::Acid,
    ];

    pub fn id(self) -> usize {
        self as usize + 1
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
            Slot::Laser => "laser",
            Slot::Vinyl => "vinyl",
            Slot::Vocal => "vocal",
            Slot::Flute => "flute",
            Slot::Festival => "festival",
            Slot::Crash => "crash",
            Slot::Guitar => "guitar",
            Slot::Rain => "rain",
            Slot::Fuzz => "fuzz",
            Slot::Acid => "acid",
        }
    }

    pub fn role(self) -> InstrumentRole {
        match self {
            Slot::Bass | Slot::Acid => InstrumentRole::Bass,
            Slot::Kick => InstrumentRole::Kick,
            Slot::Pad => InstrumentRole::Chords,
            Slot::Snare => InstrumentRole::Snare,
            Slot::Hat => InstrumentRole::Hat,
            Slot::Pluck => InstrumentRole::Lead,
            Slot::Keys => InstrumentRole::Chords,
            Slot::Kick808 => InstrumentRole::Kick,
            Slot::Laser => InstrumentRole::Lead,
            Slot::Vinyl => InstrumentRole::Texture,
            Slot::Vocal => InstrumentRole::Vocal,
            Slot::Crash => InstrumentRole::Hat,
            Slot::Guitar => InstrumentRole::Lead,
            Slot::Rain => InstrumentRole::Texture,
            Slot::Flute | Slot::Festival | Slot::Fuzz => InstrumentRole::Lead,
        }
    }

    pub fn group(self) -> Group {
        match self {
            Slot::Bass | Slot::Acid => Group::Bass,
            Slot::Kick | Slot::Kick808 => Group::Kick,
            Slot::Snare => Group::Snare,
            Slot::Hat => Group::Hat,
            Slot::Pad | Slot::Keys => Group::Chords,
            Slot::Pluck | Slot::Laser => Group::Lead,
            Slot::Vocal => Group::Vocal,
            Slot::Vinyl | Slot::Rain => Group::Texture,
            Slot::Flute | Slot::Festival | Slot::Guitar | Slot::Fuzz => Group::Lead,
            Slot::Crash => Group::Hat,
        }
    }

    /// Каноническая громкость инструмента: единый источник уровней.
    /// Стиль решает только ВКЛ/ВЫКЛ (вайтлист), цифры живут здесь.
    /// Фоновые шумы (винил/поле/дождь) намеренно тихие — вдвое ниже
    /// музыкальных текстур, иначе забивают микс.
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
            Slot::Laser => 0.65,
            Slot::Vinyl => 0.25,
            Slot::Vocal => 0.6,
            Slot::Flute => 0.5,
            Slot::Festival => 0.55,
            Slot::Crash => 0.7,
            Slot::Guitar => 0.6,
            Slot::Rain => 0.2,
            Slot::Fuzz => 0.7,
            Slot::Acid => 0.6,
        }
    }
}

#[derive(Clone, Debug, Copy)]
pub struct NoteEvent {
    pub instrument_id: usize,
    pub midi_pitch: u8,
    pub velocity: f32,
    pub duration_secs: f32,
}

impl NoteEvent {
    /// Конструктор с санитизацией: velocity в [0, 1], длительность >= 0.
    pub fn new(instrument_id: usize, midi_pitch: u8, velocity: f32, duration_secs: f32) -> Self {
        Self {
            instrument_id,
            midi_pitch,
            velocity: velocity.clamp(0.0, 1.0),
            duration_secs: duration_secs.max(0.0),
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
}

use festival::FestivalVoice;
use flute::FluteVoice;
use keys::KeysVoice;
use laser::LaserVoice;
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
    Laser(LaserVoice),
    Vocal(VocalVoice),
    Flute(FluteVoice),
    Festival(FestivalVoice),
    Guitar(GuitarVoice),
    Fuzz(FuzzVoice),
}

impl Voice for AnyVoice {
    fn set_sample_rate(&mut self, sample_rate: f32) {
        match self {
            AnyVoice::Pad(v) => v.set_sample_rate(sample_rate),
            AnyVoice::Pluck(v) => v.set_sample_rate(sample_rate),
            AnyVoice::Keys(v) => v.set_sample_rate(sample_rate),
            AnyVoice::Laser(v) => v.set_sample_rate(sample_rate),
            AnyVoice::Vocal(v) => v.set_sample_rate(sample_rate),
            AnyVoice::Flute(v) => v.set_sample_rate(sample_rate),
            AnyVoice::Festival(v) => v.set_sample_rate(sample_rate),
            AnyVoice::Guitar(v) => v.set_sample_rate(sample_rate),
            AnyVoice::Fuzz(v) => v.set_sample_rate(sample_rate),
        }
    }

    fn trigger(&mut self, note: &NoteEvent) {
        match self {
            AnyVoice::Pad(v) => v.trigger(note),
            AnyVoice::Pluck(v) => v.trigger(note),
            AnyVoice::Keys(v) => v.trigger(note),
            AnyVoice::Laser(v) => v.trigger(note),
            AnyVoice::Vocal(v) => v.trigger(note),
            AnyVoice::Flute(v) => v.trigger(note),
            AnyVoice::Festival(v) => v.trigger(note),
            AnyVoice::Guitar(v) => v.trigger(note),
            AnyVoice::Fuzz(v) => v.trigger(note),
        }
    }

    fn next_sample(&mut self) -> f32 {
        match self {
            AnyVoice::Pad(v) => v.next_sample(),
            AnyVoice::Pluck(v) => v.next_sample(),
            AnyVoice::Keys(v) => v.next_sample(),
            AnyVoice::Laser(v) => v.next_sample(),
            AnyVoice::Vocal(v) => v.next_sample(),
            AnyVoice::Flute(v) => v.next_sample(),
            AnyVoice::Festival(v) => v.next_sample(),
            AnyVoice::Guitar(v) => v.next_sample(),
            AnyVoice::Fuzz(v) => v.next_sample(),
        }
    }

    fn is_active(&self) -> bool {
        match self {
            AnyVoice::Pad(v) => v.is_active(),
            AnyVoice::Pluck(v) => v.is_active(),
            AnyVoice::Keys(v) => v.is_active(),
            AnyVoice::Laser(v) => v.is_active(),
            AnyVoice::Vocal(v) => v.is_active(),
            AnyVoice::Flute(v) => v.is_active(),
            AnyVoice::Festival(v) => v.is_active(),
            AnyVoice::Guitar(v) => v.is_active(),
            AnyVoice::Fuzz(v) => v.is_active(),
        }
    }

    fn reset(&mut self) {
        match self {
            AnyVoice::Pad(v) => v.reset(),
            AnyVoice::Pluck(v) => v.reset(),
            AnyVoice::Keys(v) => v.reset(),
            AnyVoice::Laser(v) => v.reset(),
            AnyVoice::Vocal(v) => v.reset(),
            AnyVoice::Flute(v) => v.reset(),
            AnyVoice::Festival(v) => v.reset(),
            AnyVoice::Guitar(v) => v.reset(),
            AnyVoice::Fuzz(v) => v.reset(),
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
        // Старое состояние уходит в хвост с полным гейном, новый удар
        // начинается с чистой атаки — сумма непрерывна, ступеньки нет.
        let mut fresh = V::default();
        fresh.set_sample_rate(self.sample_rate);
        self.tail = std::mem::replace(&mut self.main, fresh);
        self.tail_gain = 1.0;
        self.main.trigger(note);
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

    pub fn lasers(id: usize, count: usize) -> Self {
        Self::with_voices(
            id,
            InstrumentRole::Lead,
            (0..count)
                .map(|_| AnyVoice::Laser(LaserVoice::default()))
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

    pub fn flutes(id: usize, count: usize) -> Self {
        Self::with_voices(
            id,
            InstrumentRole::Lead,
            (0..count)
                .map(|_| AnyVoice::Flute(FluteVoice::default()))
                .collect(),
        )
    }

    pub fn festivals(id: usize, count: usize) -> Self {
        Self::with_voices(
            id,
            InstrumentRole::Lead,
            (0..count)
                .map(|_| AnyVoice::Festival(FestivalVoice::default()))
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

    pub fn fuzzes(id: usize, count: usize) -> Self {
        Self::with_voices(
            id,
            InstrumentRole::Lead,
            (0..count)
                .map(|_| AnyVoice::Fuzz(FuzzVoice::default()))
                .collect(),
        )
    }

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
