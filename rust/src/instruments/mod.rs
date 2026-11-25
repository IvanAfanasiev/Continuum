pub mod bass;
pub mod dsp;
pub mod guitar;
pub mod hat;
pub mod keys;
pub mod kick;
pub mod piano;
pub mod kick808;
pub mod pad;
pub mod pluck;
pub mod rain;
pub mod shaker;
pub mod snare;
pub mod vinyl;

pub use bass::BassGuitar;
pub use guitar::GuitarVoice;
pub use hat::HiHat;
pub use kick::Kick;
pub use kick808::Kick808;
pub use rain::Rain;
pub use shaker::Shaker;
pub use snare::SnareDrum;
pub use vinyl::Vinyl;

/// Role in the arrangement. Granular for electronic music: kick, snare and
/// hi-hat get their own patterns in styles (previously all drums shared one
/// `Percussion` and played the kick part).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InstrumentRole {
    Bass,
    Kick,
    Snare,
    Hat,
    Chords,
    Lead,
    /// Textures (vinyl, rain): outside the pump and almost outside the delay.
    Texture,
}

impl InstrumentRole {
    /// Role index = discriminant (declaration order).
    /// Used as the column index of the step-volume table.
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
            InstrumentRole::Texture => "texture",
        }
    }
}

/// Instrument group for the frontend: a regular user doesn't need a dozen
/// variants of the same instrument. One group knob —
/// a macro fader over slots (multiplier 0..1, default 1.0).
/// Which specific variant sounds (punch or 808) is decided by the style
/// through its default mix; the group knob survives a style change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Group {
    Bass,
    Kick,
    Snare,
    Hat,
    Chords,
    Lead,
    Texture,
}

impl Group {
    /// Group index = discriminant (declaration order): a bit of the
    /// arrangement-layer mask.
    pub const fn index(self) -> usize {
        self as usize
    }

    pub const ALL: [Group; 7] = [
        Group::Bass,
        Group::Kick,
        Group::Snare,
        Group::Hat,
        Group::Chords,
        Group::Lead,
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
            "texture" | "atmosphere" => Some(Group::Texture),
            _ => None,
        }
    }
}
/// Ensemble slot: a stable unit of addressing for styles, mixer and UI.
/// Numbers are FROZEN forever (explicit discriminants): removing a slot
/// leaves a hole instead of shifting the others — frontend knobs, saved
/// mixes and Wad binding files don't rot. The frontend doesn't store an id,
/// it gets it from the backend at every start. Order in ALL is only the order
/// of guides and traversal, it doesn't affect ids.
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
    Guitar = 14,
    Rain = 15,
    Shaker = 17,
    /// Lead guitar (picking/long lines): the same DSP core as the riff,
    /// a different character (sustain, top, light drive) and its own guides.
    /// Added after the rest — the next free number.
    GuitarLead = 20,
    /// Piano: a string-struck keyboard from the `continuum` backend.
    Piano = 23,
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
        Slot::Guitar,
        Slot::Rain,
        Slot::Shaker,
        Slot::GuitarLead,
        Slot::Piano,
    ];

    pub fn id(self) -> usize {
        // Explicit discriminant, NOT position in ALL: survives deletions.
        self as usize
    }

    /// Position in `ALL`: style guides are indexed by it.
    /// The id discriminant can't be used for this (it has holes).
    pub const fn index(self) -> usize {
        match self {
            Slot::Bass => 0,
            Slot::Kick => 1,
            Slot::Pad => 2,
            Slot::Snare => 3,
            Slot::Hat => 4,
            Slot::Pluck => 5,
            Slot::Keys => 6,
            Slot::Kick808 => 7,
            Slot::Vinyl => 8,
            Slot::Guitar => 9,
            Slot::Rain => 10,
            Slot::Shaker => 11,
            Slot::GuitarLead => 12,
            Slot::Piano => 13,
        }
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
            Slot::Guitar => "guitar",
            Slot::Rain => "rain",
            Slot::Shaker => "shaker",
            Slot::GuitarLead => "guitar lead",
            Slot::Piano => "piano",
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
            Slot::Guitar | Slot::GuitarLead => InstrumentRole::Lead,
            Slot::Rain => InstrumentRole::Texture,
            Slot::Piano => InstrumentRole::Chords,
        }
    }

    pub fn group(self) -> Group {
        match self {
            Slot::Bass => Group::Bass,
            Slot::Kick | Slot::Kick808 => Group::Kick,
            Slot::Snare => Group::Snare,
            Slot::Hat | Slot::Shaker => Group::Hat,
            Slot::Pad | Slot::Keys | Slot::Piano => Group::Chords,
            Slot::Pluck | Slot::Guitar | Slot::GuitarLead => Group::Lead,
            Slot::Vinyl | Slot::Rain => Group::Texture,
        }
    }

    /// Canonical instrument volume: a single source of levels.
    /// The style sets the palette, the seed the cast; the numbers live here.
    /// Background noises (vinyl) are deliberately quiet — half the musical
    /// textures, otherwise they overwhelm the mix.
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
            Slot::Guitar => 0.6,
            Slot::Rain => 0.2,
            Slot::Shaker => 0.55,
            Slot::GuitarLead => 0.6,
            Slot::Piano => 0.6,
        }
    }
}

#[derive(Clone, Debug, Copy)]
pub struct NoteEvent {
    pub instrument_id: usize,
    pub midi_pitch: u8,
    pub velocity: f32,
    pub duration_secs: f32,
    /// Seed-based attack and release multipliers (1.0 = the instrument's character).
    /// Only soft voices read them (bass, pad, keys): drums always play
    /// as written. Range 0.8..1.8 — the character is intact, seeds differ.
    pub attack_mul: f32,
    pub release_mul: f32,
    /// Tie: the note continues an already-sounding note of the same pitch
    /// instead of re-attacking. The composer sets it on mono layers (bass)
    /// when a repeat lands on a still-sounding note: a long note really
    /// stretches instead of being cut by the next onset.
    pub tie: bool,
    /// Sub bias 0..1 from the style: 1.0 = a pure sine without string/click.
    /// Only the bass voice reads it.
    pub sub_bias: f32,
    /// Due scheduling of the pump: the note's global step and the step
    /// duration in microseconds. The pump fires the note exactly in its
    /// media frame, not whenever it drained the queue. Zeros = legacy (fire
    /// immediately): that's how tests and direct engine calls build notes.
    pub due_step: u64,
    pub step_us: u32,
}

impl NoteEvent {
    /// Constructor with sanitization: velocity in [0, 1], duration >= 0.
    pub fn new(instrument_id: usize, midi_pitch: u8, velocity: f32, duration_secs: f32) -> Self {
        Self {
            instrument_id,
            midi_pitch,
            velocity: velocity.clamp(0.0, 1.0),
            duration_secs: duration_secs.max(0.0),
            attack_mul: 1.0,
            release_mul: 1.0,
            tie: false,
            sub_bias: 0.0,
            due_step: 0,
            step_us: 0,
        }
    }

    /// Seed articulation: attack/release multipliers clamped to a sane range.
    pub fn shaped(self, attack_mul: f32, release_mul: f32) -> Self {
        Self {
            attack_mul: attack_mul.clamp(0.25, 3.0),
            release_mul: release_mul.clamp(0.25, 3.0),
            ..self
        }
    }
}

/// Monophonic procedural instrument (kick, snare, hi-hat, bass).
/// Monophony is musically right for drums and bass: a retrigger mutes
/// the previous hit (the hi-hat's choke — like a live instrument).
///
/// The sample rate is set once via `set_sample_rate`,
/// not threaded into every call.
pub trait Instrument: Send {
    fn set_sample_rate(&mut self, sample_rate: f32);
    fn trigger(&mut self, note: &NoteEvent);
    fn next_sample(&mut self) -> f32;
    /// Stereo render. Default — mono double (center): that's how drums
    /// and bass work, the low end is always centered, as is customary in
    /// club music. Polyphonic pools override: they spread voices to the sides.
    fn next_sample_stereo(&mut self) -> (f32, f32) {
        let m = self.next_sample();
        (m, m)
    }
    fn id(&self) -> usize;
    fn role(&self) -> InstrumentRole;
    fn reset(&mut self);
    fn is_active(&self) -> bool;
}

/// One polyphonic voice (pad, keys, pluck).
/// Concrete voices are listed in [`AnyVoice`]; the [`VoicePool`] holds them
/// as data rather than as a generic parameter — so the whole type graph
/// stays concrete and legible to the FFI parser.
pub trait Voice: Send {
    fn set_sample_rate(&mut self, sample_rate: f32);
    fn trigger(&mut self, note: &NoteEvent);
    fn next_sample(&mut self) -> f32;
    /// Stereo render of a voice. Default — mono double; voices with their own
    /// positioning (guitar) only need to override this method.
    fn next_sample_stereo(&mut self) -> (f32, f32) {
        let m = self.next_sample();
        (m, m)
    }
    fn is_active(&self) -> bool;
    fn reset(&mut self);
    /// Starting frequency for portamento: called by the wrapper before trigger.
    /// Default — ignore (portamento is needed by units like 808 slides).
    fn set_glide_start(&mut self, _freq_hz: f32) {}
    /// Legato transition to a new note without restarting attack/phase:
    /// a long line doesn't interrupt itself. Default — a normal
    /// retrigger (drums need a new attack, not a glide).
    /// `has_legato` distinguishes a voice's own glide from the default: the
    /// wrapper takes the short path (no tail) only with its own.
    fn legato(&mut self, note: &NoteEvent) {
        self.trigger(note);
    }
    /// Extension of an already-sounding note (tie). Default — legato, but
    /// a voice may preserve timbre/attack and extend only the tail:
    /// a tie is a continuation of the same note, not a new strike.
    fn extend(&mut self, note: &NoteEvent) {
        self.legato(note);
    }
    fn has_legato(&self) -> bool {
        false
    }
}

use keys::KeysVoice;
use pad::PadVoice;
use piano::PianoVoice;
use pluck::PluckVoice;

/// Any voice of a polyphonic pool. An enum instead of a generic parameter:
/// a concrete type in each variant, dynamic dispatch
/// without `dyn` in struct fields.
#[derive(Clone)]
pub(crate) enum AnyVoice {
    Pad(PadVoice),
    Pluck(PluckVoice),
    Keys(KeysVoice),
    Piano(PianoVoice),
    Guitar(GuitarVoice),
}

impl Voice for AnyVoice {
    fn set_sample_rate(&mut self, sample_rate: f32) {
        match self {
            AnyVoice::Pad(v) => v.set_sample_rate(sample_rate),
            AnyVoice::Pluck(v) => v.set_sample_rate(sample_rate),
            AnyVoice::Keys(v) => v.set_sample_rate(sample_rate),
            AnyVoice::Piano(v) => v.set_sample_rate(sample_rate),
            AnyVoice::Guitar(v) => v.set_sample_rate(sample_rate),
        }
    }

    fn trigger(&mut self, note: &NoteEvent) {
        match self {
            AnyVoice::Pad(v) => v.trigger(note),
            AnyVoice::Pluck(v) => v.trigger(note),
            AnyVoice::Keys(v) => v.trigger(note),
            AnyVoice::Piano(v) => v.trigger(note),
            AnyVoice::Guitar(v) => v.trigger(note),
        }
    }

    fn next_sample(&mut self) -> f32 {
        match self {
            AnyVoice::Pad(v) => v.next_sample(),
            AnyVoice::Pluck(v) => v.next_sample(),
            AnyVoice::Keys(v) => v.next_sample(),
            AnyVoice::Piano(v) => v.next_sample(),
            AnyVoice::Guitar(v) => v.next_sample(),
        }
    }

    fn next_sample_stereo(&mut self) -> (f32, f32) {
        match self {
            AnyVoice::Pad(v) => v.next_sample_stereo(),
            AnyVoice::Pluck(v) => v.next_sample_stereo(),
            AnyVoice::Keys(v) => v.next_sample_stereo(),
            AnyVoice::Piano(v) => v.next_sample_stereo(),
            AnyVoice::Guitar(v) => v.next_sample_stereo(),
        }
    }

    fn is_active(&self) -> bool {
        match self {
            AnyVoice::Pad(v) => v.is_active(),
            AnyVoice::Pluck(v) => v.is_active(),
            AnyVoice::Keys(v) => v.is_active(),
            AnyVoice::Piano(v) => v.is_active(),
            AnyVoice::Guitar(v) => v.is_active(),
        }
    }

    fn reset(&mut self) {
        match self {
            AnyVoice::Pad(v) => v.reset(),
            AnyVoice::Pluck(v) => v.reset(),
            AnyVoice::Keys(v) => v.reset(),
            AnyVoice::Piano(v) => v.reset(),
            AnyVoice::Guitar(v) => v.reset(),
        }
    }
}

/// Cross-side bleed in the pool: 0 = hard L/R, 1 = mono.
const STEREO_BLEED: f32 = 0.35;
/// Crossfade tail on retrigger/steal: the previous hit fades out
/// over the new strike instead of a step-click.
const TAIL_FADE_SECS: f32 = 0.008;

fn tail_decay_for(sample_rate: f32) -> f32 {
    (-1.0 / (sample_rate.max(1.0) * TAIL_FADE_SECS)).exp()
}

/// Mono voice with tail crossfade: a retrigger doesn't chop the previous
/// hit with a step (the main source of clicks on frequent repeats), it fades
/// it out over ~8 ms on top of the fresh strike. Choke logic (hi-hat!) is
/// preserved: the old hit is still muted, just smoothly.
pub struct Mono<V: Voice + Default + Clone> {
    id: usize,
    role: InstrumentRole,
    main: V,
    tail: V,
    tail_gain: f32,
    tail_decay: f32,
    sample_rate: f32,
    /// Pitch of the sounding note: repeating the same pitch is a fresh attack
    /// (repeat articulation), changing pitch on a legato voice is a glide
    /// without restart (a long line doesn't interrupt itself).
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

    /// Trigger with setup of the fresh voice before note start
    /// (808 portamento: where to slide from). The tail is as usual.
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
        // Tie: repeating the same pitch while the note sounds EXTENDS it
        // without a re-attack. Otherwise a long note on a mono layer is
        // pointless: the next onset cuts it to a step. The staccato pulse
        // remains where the composer didn't set a tie.
        if note.tie && self.main.is_active() && self.pitch == Some(note.midi_pitch) {
            self.main.extend(note);
            return;
        }
        // Legato: a ringing voice takes a new PITCH without restarting —
        // the line flows as a glide instead of being chopped on every eighth.
        // Same pitch — fresh attack (the repeat is articulated).
        // Voices without their own legato take the old path with a tail.
        if self.main.is_active() && self.main.has_legato() && self.pitch != Some(note.midi_pitch) {
            self.main.legato(note);
            self.pitch = Some(note.midi_pitch);
            return;
        }
        // Old state goes to the tail at full gain, the new strike starts
        // with a clean attack — the sum is continuous, there's no step.
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
        // A style/seed reset silences the voice: the "sounding pitch" no
        // longer sounds and must not take part in the next section's ties.
        self.pitch = None;
    }

    fn is_active(&self) -> bool {
        self.main.is_active() || self.tail_gain > 0.0
    }
}

/// Voice pool with oldest-voice stealing. Chords and overlapping notes
/// sound together instead of cutting each other off.
/// The `pads`/`plucks`/`keys` constructors set both timbre and role.
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
    /// Stereo allpass on the right channel: phase offset instead of hard pan.
    /// |H| = 1, only phase changes — mono compatibility is unaffected.
    ap_r_x: f32,
    ap_r_y: f32,
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
            ap_r_x: 0.0,
            ap_r_y: 0.0,
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

    pub fn pianos(id: usize, count: usize) -> Self {
        Self::with_voices(
            id,
            InstrumentRole::Chords,
            (0..count)
                .map(|_| AnyVoice::Piano(PianoVoice::new()))
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

    /// Lead guitar (picking/long lines): the same DSP core, the Lead
    /// character (sustain, top, light drive). A separate slot, since the lead
    /// has its own rhythm, register and envelopes — and it sounds together
    /// with the riff.
    pub fn guitar_leads(id: usize, count: usize) -> Self {
        Self::with_voices(
            id,
            InstrumentRole::Lead,
            (0..count)
                .map(|_| AnyVoice::Guitar(GuitarVoice::lead()))
                .collect(),
        )
    }

    /// Formulaic lead: a function of time instead of an oscillator.
    /// Wavetable lead: a circulating wave from a table instead of an oscillator.
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
            // The stolen voice goes to the tail: a cut without a click.
            self.tail = Some(self.voices[idx].clone());
            self.tail_gain = 1.0;
        }
        self.voices[idx].trigger(note);
    }

    /// Mono — the average of stereo (one source of truth, no double render).
    fn next_sample(&mut self) -> f32 {
        let (l, r) = self.next_sample_stereo();
        (l + r) * 0.5
    }

    /// Width without duplication: even voices left, odd voices right,
    /// plus cross-bleed for mono compatibility. Voices with their own
    /// positioning (guitar) give two channels. The steal tail —
    /// centered on top. Before output the right channel goes through an allpass:
    /// phase offset instead of hard L/R, |H|=1 — loudness doesn't change.
    fn next_sample_stereo(&mut self) -> (f32, f32) {
        let mut even_l = 0.0;
        let mut even_r = 0.0;
        let mut odd_l = 0.0;
        let mut odd_r = 0.0;
        for (i, voice) in self.voices.iter_mut().enumerate() {
            let (l, r) = voice.next_sample_stereo();
            if i.is_multiple_of(2) {
                even_l += l;
                even_r += r;
            } else {
                odd_l += l;
                odd_r += r;
            }
        }
        let tail = self.render_tail();
        let left = even_l + odd_l * STEREO_BLEED + tail;
        let right = odd_r + even_r * STEREO_BLEED + tail;
        // One-pole allpass: a = 0.55 → ~1.4 ms phase shift
        // in the middle of the spectrum. It creates no loudness mirror.
        const AP_A: f32 = 0.55;
        let out_r = AP_A * right + self.ap_r_x - AP_A * self.ap_r_y;
        self.ap_r_x = right;
        self.ap_r_y = out_r;
        (left, out_r)
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
        self.ap_r_x = 0.0;
        self.ap_r_y = 0.0;
    }

    fn is_active(&self) -> bool {
        self.voices.iter().any(|v| v.is_active()) || self.tail_gain > 0.0
    }
}

/// Guard against zero/negative sample rate.
pub(crate) fn sane_sample_rate(sample_rate: f32) -> f32 {
    if sample_rate.is_finite() && sample_rate > 0.0 {
        sample_rate
    } else {
        44_100.0
    }
}

/// Test spectral metric: the share of band `[lo, hi)` energy in the
/// 100 Hz – 10 kHz range. Goertzel over all bins of an 8192-sample window
/// with a Hann window: no FFT dependency, but real frequency resolution —
/// a sparse grid lies about harmonics.
#[cfg(test)]
pub(crate) fn band_ratio(samples: &[f32], sample_rate: f32, lo: f32, hi: f32) -> f32 {
    let n = samples.len().min(8192);
    if n < 16 {
        return 0.0;
    }
    // Hann: a transient doesn't smear across all bins.
    let windowed: Vec<f32> = samples[..n]
        .iter()
        .enumerate()
        .map(|(i, &x)| {
            let w = 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / (n - 1) as f32).cos();
            x * w
        })
        .collect();
    let bin_hz = sample_rate / n as f32;
    let first = (100.0 / bin_hz).ceil() as usize;
    let last = ((10_000.0 / bin_hz).floor() as usize).min(n / 2);
    let mut band = 0.0f32;
    let mut total = 0.0f32;
    for bin in first..=last {
        let omega = 2.0 * std::f32::consts::PI * bin as f32 / n as f32;
        let coeff = 2.0 * omega.cos();
        let (mut s1, mut s2) = (0.0f32, 0.0f32);
        for &x in &windowed {
            let s0 = x + coeff * s1 - s2;
            s2 = s1;
            s1 = s0;
        }
        let p = (s1 * s1 + s2 * s2 - coeff * s1 * s2).max(0.0);
        let hz = bin as f32 * bin_hz;
        if hz >= lo && hz < hi {
            band += p;
        }
        total += p;
    }
    band / total.max(1e-12)
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
        // Slot numbers are eternal: removing a slot leaves a hole,
        // the others don't shift. This test is nailed down:
        // any change to an id here is breaking and requires versioning.
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
            (Guitar, 14),
            (Rain, 15),
            (Shaker, 17),
            (GuitarLead, 20),
            (Piano, 23),
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
        // Without shaping, articulation is neutral (drums and old paths),
        // shaped is clamped to sanity and chained by the builder.
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
        // A 2-voice pool, 5 overlapping notes triggered —
        // oldest-voice stealing must not panic and silence everything.
        let mut pool = VoicePool::pads(3, 2);
        pool.set_sample_rate(44_100.0);
        for i in 0..5 {
            pool.trigger(&NoteEvent::new(3, 60 + i, 0.8, 1.0));
            assert!(pool.is_active());
        }
        // The sum of voices is finite and non-zero.
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

    /// Counter voice: distinguishes a re-attack from a tie extension.
    #[derive(Default, Clone)]
    struct CountVoice {
        triggers: u32,
        legatos: u32,
        active: bool,
    }

    impl Voice for CountVoice {
        fn set_sample_rate(&mut self, _sample_rate: f32) {}
        fn trigger(&mut self, _note: &NoteEvent) {
            self.triggers += 1;
            self.active = true;
        }
        fn next_sample(&mut self) -> f32 {
            if self.active {
                1.0
            } else {
                0.0
            }
        }
        fn is_active(&self) -> bool {
            self.active
        }
        fn reset(&mut self) {
            self.active = false;
        }
        fn legato(&mut self, _note: &NoteEvent) {
            self.legatos += 1;
            self.active = true;
        }
        fn has_legato(&self) -> bool {
            true
        }
    }

    #[test]
    fn mono_tie_extends_without_reattack() {
        // A tie isn't a new note: the same pitch while the voice sounds goes
        // through extend (extension), not trigger (re-attack). Otherwise
        // a long note on a mono layer is cut by the next onset.
        let mut mono: Mono<CountVoice> = Mono::new(1, InstrumentRole::Bass);
        mono.set_sample_rate(44_100.0);
        let base = NoteEvent::new(1, 60, 0.8, 0.5);
        mono.trigger(&base);
        assert_eq!(mono.main.triggers, 1);
        assert_eq!(mono.main.legatos, 0);

        let mut tie = base;
        tie.tie = true;
        mono.trigger(&tie);
        assert_eq!(mono.main.triggers, 1, "tie must not re-attack");
        assert_eq!(mono.main.legatos, 1, "tie must extend the voice");

        // Without a tie, repeating the same pitch is an honest re-attack (pulse):
        // the fresh voice strikes, the old one goes to the choke tail.
        mono.trigger(&base);
        assert_eq!(mono.main.triggers, 1, "plain repeat must re-attack");
        assert_eq!(mono.main.legatos, 0);
        assert_eq!(mono.tail.triggers, 1, "old voice goes to the choke tail");
        assert_eq!(mono.tail.legatos, 1);

        // A style/seed reset also silences the "sounding pitch": a tie across
        // a section boundary must not drag the old note along.
        mono.reset();
        assert_eq!(mono.pitch, None);
    }
}
