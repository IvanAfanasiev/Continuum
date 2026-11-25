//! Playback controller for Flutter and CLI.
//!
//! Thread ownership: the engine and composer live inside the controller.
//! All control calls (pause, tempo, style, seed, slot volumes)
//! only change flags and shared objects — the backend's main loop and streams
//! are NOT terminated or recreated.
//!
//! Presence semantics: **slot volume > 0 = in use**.
//! A zero slot is excluded from generation (composer) and spends no DSP
//! once its voice has faded (engine). There is no separate on/off flag.

use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, Mutex, OnceLock, RwLock,
};
use std::thread::JoinHandle;

use crossbeam::queue::ArrayQueue;

use crate::composer::{Composer, LiveTempo};
use crate::core::{normalize_tempo, snapshot, DEFAULT_SEED, NOTE_QUEUE_CAPACITY};
use crate::engine::{f32_identity, AudioEngine, MasterFx, Pump, SharedFx};
use crate::instruments::{
    BassGuitar, Group, HiHat, Instrument, Kick, Kick808, NoteEvent, Rain, Shaker, Slot, SnareDrum,
    Vinyl, VoicePool,
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
pub const VINYL_ID: usize = 10;
pub const GUITAR_ID: usize = 14;
pub const RAIN_ID: usize = 15;
pub const SHAKER_ID: usize = 17;
pub const GUITAR_LEAD_ID: usize = 20;
pub const PIANO_ID: usize = 23;

/// All ensemble ids — for validating `set_track` from Flutter.
pub const ENSEMBLE_IDS: [usize; Slot::COUNT] = [
    BASS_ID,
    KICK_ID,
    PAD_ID,
    SNARE_ID,
    HAT_ID,
    PLUCK_ID,
    KEYS_ID,
    KICK808_ID,
    VINYL_ID,
    GUITAR_ID,
    RAIN_ID,
    SHAKER_ID,
    GUITAR_LEAD_ID,
    PIANO_ID,
];

/// State snapshot for UI. This is the type sent through flutter_rust_bridge.
#[derive(Debug, Clone)]
pub struct PlaybackStatus {
    pub running: bool,
    pub paused: bool,
    pub style_name: String,
    pub seed: u64,
    /// Canonical seed code `XXX-XXX`: the frontend displays and enters it.
    pub seed_code: String,
    /// Effective tempo: the frontend knob or the style default.
    pub tempo_bpm: f32,
    /// Frontend intensity knob (the "temperature" of generation).
    pub intensity: f32,
    pub engine_error: Option<String>,
    /// Audio engine version (see ENGINE_VERSION): explains
    /// why a favorite seed sounds different after an update.
    pub engine_version: u32,
    /// The listener edited the instrument list: style and seed don't touch it,
    /// the reset button lights up in the mixer.
    pub instrument_list_edited: bool,
    /// Master effects: currently active values (for UI sliders).
    pub reverb_wet: f32,
    pub reverb_size: f32,
    pub treble_hz: f32,
    pub sidechain: f32,
    /// Filter-openness offset on top of the form (-0.5..0.5).
    pub tone_open: f32,
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
            instrument_list_edited: false,
            reverb_wet: MasterFx::default().reverb_wet,
            reverb_size: MasterFx::default().reverb_size,
            treble_hz: MasterFx::default().treble_hz,
            sidechain: MasterFx::default().sidechain,
            tone_open: 0.0,
        }
    }
}

impl PlaybackStatus {
    /// Status without a session, but with the listener's already chosen effects —
    /// UI sliders don't jump to defaults before the first Play press.
    fn default_with_fx(fx: MasterFx) -> Self {
        Self {
            reverb_wet: fx.reverb_wet,
            reverb_size: fx.reverb_size,
            treble_hz: fx.treble_hz,
            sidechain: fx.sidechain,
            tone_open: crate::core::tone_open_offset(),
            ..Self::default()
        }
    }
}

/// Live frame for the circular visualizer: position in the bar,
/// energy, section, seed motif and the theme position in it + masks
/// of the main voices by beats (wheel dots: kick, snare+hat,
/// bass, theme). A concrete type without generics — safe for the FFI parser.
#[derive(Debug, Clone)]
pub struct LiveVisual {
    pub bar_phase: f32,
    pub energy: f32,
    pub section: u8,
    pub motif: Vec<u8>,
    pub motif_pos: u8,
    /// Mask of sounding beat positions (bit = 16th note).
    pub bar_hits: u16,
    /// Masks by voice (the same bit = the same beat).
    pub kick_hits: u16,
    pub snare_hits: u16,
    pub bass_hits: u16,
    pub theme_hits: u16,
    /// Phrase notes (bit = phrase beat): real notes, the hand makes
    /// one revolution per phrase.
    pub phrase_len: u8,
    pub phrase_pos: u8,
    /// Exact phrase phase 0..1 (the wheel's anchor).
    pub phrase_phase: f32,
    /// Wheel window phase 0..1 (window = 4 bars): the hand moves by it.
    pub window_phase: f32,
    pub pkick: u64,
    pub psnare: u64,
    pub pbass: u64,
    pub ptheme: u64,
    /// Phrase beats where something sounded (bit = phrase beat).
    pub pplayed: u32,
}

impl Default for LiveVisual {
    fn default() -> Self {
        Self {
            bar_phase: 0.0,
            energy: 0.0,
            section: 0,
            motif: vec![0u8; 16],
            motif_pos: 0,
            bar_hits: 0,
            kick_hits: 0,
            snare_hits: 0,
            bass_hits: 0,
            theme_hits: 0,
            phrase_len: 32,
            phrase_pos: 0,
            phrase_phase: 0.0,
            window_phase: 0.0,
            pkick: 0,
            psnare: 0,
            pbass: 0,
            ptheme: 0,
            pplayed: 0,
        }
    }
}

/// One mixer slot for UI: id, names and live volume.
/// `volume` — effective (slot × group), i.e. audible.
/// `base_volume` — the style's canonical level (100% of the slot slider).
/// `group` — the slot's macro-fader name for the frontend.
#[derive(Debug, Clone)]
pub struct TrackInfo {
    pub id: u64,
    pub name: String,
    pub role: String,
    pub group: String,
    pub volume: f32,
    pub base_volume: f32,
}

/// One macro fader for UI: group name and its multiplier.
#[derive(Debug, Clone)]
pub struct GroupInfo {
    pub name: String,
    pub volume: f32,
}

/// Catalog style for UI: name and shelf (`club`/`rock`/`chill`).
#[derive(Debug, Clone)]
pub struct StyleMeta {
    pub name: String,
    pub group: String,
}

/// The engine's instrument set: one voice per slot.
pub fn build_instruments() -> Vec<Box<dyn Instrument>> {
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
                Slot::Vinyl => Box::new(Vinyl::new(slot.id())),
                Slot::Guitar => Box::new(VoicePool::guitars(slot.id(), 6)),
                Slot::Rain => Box::new(Rain::new(slot.id())),
                Slot::Shaker => Box::new(Shaker::new(slot.id())),
                Slot::GuitarLead => Box::new(VoicePool::guitar_leads(slot.id(), 4)),
                Slot::Piano => Box::new(VoicePool::pianos(slot.id(), 6)),
            }
        })
        .collect()
}

/// Live pipeline counters (shared between threads).
/// frames — frames given to the audio device; pushed — notes from the generator;
/// consumed — notes drained by the engine from the queue;
/// dropped — notes that didn't fit in the queue (the generator outruns
/// the engine: the rhythm starts developing holes — visible in the counter);
/// restarts — stream recreations (route change, callback break).
/// Public for the engine constructor; outside, only EngineStats is interesting.
#[derive(Debug, Clone, Default)]
pub struct EngineCounters {
    pub frames: Arc<AtomicU64>,
    pub pushed: Arc<AtomicU64>,
    pub consumed: Arc<AtomicU64>,
    pub dropped: Arc<AtomicU64>,
    /// Notes that fired later than their beat (audible as a rhythm glitch).
    pub late: Arc<AtomicU64>,
    /// Grid re-anchors on a live stream: the generator fell behind
    /// by more than a step, the schedule moved to the current frame.
    pub reanchors: Arc<AtomicU64>,
    pub restarts: Arc<AtomicU64>,
}

/// Live pipeline diagnostics for the frontend. Zeros without a session.
/// Lets one tell "stream doesn't start" from "generator is silent"
/// from "everything works, check the phone's volume".
#[derive(Debug, Clone, Default)]
pub struct EngineStats {
    pub frames: u64,
    pub pushed: u64,
    pub consumed: u64,
    /// Notes lost because the queue was full: the generator outruns
    /// the engine. A nonzero counter = the rhythm has holes (beats are missing).
    pub dropped: u64,
    /// Notes that sounded later than their beat: the rhythm stumbles.
    pub late: u64,
    /// Grid re-anchors: the generator lagged, the schedule moved.
    pub reanchors: u64,
    pub queued: u64,
    /// Selected output (`48000 Hz, F32, 2ch`): visible in the banner,
    /// which config the engine took on this phone.
    pub audio_config: String,
    /// How many times the stream was recreated live (route change,
    /// callback break). Visible in the banner on a stall.
    pub restarts: u64,
    /// Output latency (track buffer), ms: how far the sound lags
    /// behind generation. The frontend wheel shifts its display by it (otherwise the hand
    /// and the dots run ahead of what's audible). Zero = unknown/not playing.
    pub output_latency_ms: f32,
}

/// Starting a background thread with a guard against silent death: a panic used to
/// kill the loop without a trace (frames=0 with no engine_error). Now a panic
/// turns into text — a red banner instead of guesswork.
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

/// A style's cast for a seed: canonical volumes of the selected slots,
/// the rest at zero. Shared by the controller and CLI.
/// Slots outside the cast are NOT forbidden: the user can raise any
/// manually, a seed/style change just resets the defaults again.
/// But only if the user hasn't edited the list themselves: the manual choice
/// survives seed and style changes (reset is a separate button in the mixer).
pub fn apply_style_mix(style: &dyn Style, seed: u64, mixer: &mut MixerState) {
    // The Lead group was removed from the product: its voices don't match
    // the name and sound bad. We disable them ALWAYS, even if the listener
    // edited the list manually (otherwise an old choice would resurrect them).
    for slot in Slot::ALL {
        if slot.group() == Group::Lead {
            mixer.set_track(slot.id(), 0.0);
        }
    }
    if mixer.list_edited {
        // The listener assembled their own lineup: foreign casts don't touch it.
        return;
    }
    let cast = style.cast(seed);
    for slot in Slot::ALL {
        let vol = if cast.contains(&slot) && slot.group() != Group::Lead {
            slot.base_volume()
        } else {
            0.0
        };
        mixer.set_track(slot.id(), vol);
    }
}
/// The style's master effects in shared ownership for the engine.
fn fx_for_style(style: &dyn Style) -> MasterFx {
    MasterFx {
        reverb_wet: style.reverb_wet(),
        reverb_size: style.reverb_size(),
        treble_hz: style.treble_cutoff(),
        sidechain: style.sidechain_depth().clamp(0.0, 2.0),
    }
}

/// The listener's manual master effects live on top of the style defaults
/// and survive style/seed/session changes (reset is a separate button).
fn fx_override() -> &'static RwLock<Option<MasterFx>> {
    static FX: OnceLock<RwLock<Option<MasterFx>>> = OnceLock::new();
    FX.get_or_init(|| RwLock::new(None))
}

fn effective_fx(style: &dyn Style) -> MasterFx {
    fx_override()
        .read()
        .map(|g| g.unwrap_or_else(|| fx_for_style(style)))
        .unwrap_or_else(|e| e.into_inner().unwrap_or_else(|| fx_for_style(style)))
}

struct RunningSession {
    queue: Arc<ArrayQueue<NoteEvent>>,
    /// Step stamps (the length of every step): the engine builds the time
    /// grid from them — without stamps gaps are estimated linearly and the timing
    /// drifts (the rhythm stumbles).
    marks: Arc<ArrayQueue<crate::composer::StepMark>>,
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
    /// Live snapshot for the visualizer (written by the composer every step).
    visual: Arc<RwLock<crate::composer::VisualSnap>>,
    /// Old-generation break flag (seed/style change): shared between
    /// the composer and the engine.
    voice_reset: Arc<AtomicBool>,
}

impl RunningSession {
    fn launch(style_name: &str, seed: u64, with_audio: bool) -> Self {
        let style_box = styles::make_style(style_name)
            .unwrap_or_else(|| styles::make_style("club").expect("builtin style"));
        let bpm_init = style_box.bpm();
        let fx_init = effective_fx(&*style_box);
        let style: Arc<RwLock<Box<dyn Style>>> = Arc::new(RwLock::new(style_box));
        let seed_lock = Arc::new(RwLock::new(seed));
        let tempo_lock: LiveTempo = Arc::new(RwLock::new(None));
        let intensity_lock = Arc::new(RwLock::new(1.0f32));
        let fx_lock: SharedFx = Arc::new(RwLock::new(fx_init));
        let bpm_lock = Arc::new(RwLock::new(bpm_init));
        let filter_open = Arc::new(RwLock::new(0.4f32));
        let mixer = Arc::new(RwLock::new(MixerState::new()));
        let queue = Arc::new(ArrayQueue::new(NOTE_QUEUE_CAPACITY));
        let marks = Arc::new(ArrayQueue::new(NOTE_QUEUE_CAPACITY));
        let paused = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));
        let engine_error = Arc::new(Mutex::new(None));
        let counters = EngineCounters::default();
        let audio_config: Arc<RwLock<String>> = Arc::new(RwLock::new(String::new()));
        let visual = Arc::new(RwLock::new(crate::composer::VisualSnap::default()));
        // Old-generation break flag: a seed or style change silences
        // voices and the room tail (an abrupt cut is appropriate).
        let voice_reset = Arc::new(AtomicBool::new(false));

        let instruments = build_instruments();
        {
            let style_guard = style.read().unwrap_or_else(|e| e.into_inner());
            let mut mix_guard = mixer.write().unwrap_or_else(|e| e.into_inner());
            apply_style_mix(&**style_guard, seed, &mut mix_guard);
        }

        let composer = Composer::new_shared(
            Arc::clone(&queue),
            Arc::clone(&marks),
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
            Arc::clone(&counters.dropped),
            Arc::clone(&visual),
            Arc::clone(&voice_reset),
        );
        let composer_handle = spawn_guarded("composer", &engine_error, || composer.run());

        // The closure always captures the instruments; if audio isn't needed,
        // it's simply dropped along with them — no stream is created.
        let engine_handle = with_audio.then(|| {
            let engine = AudioEngine::new(
                Arc::clone(&queue),
                Arc::clone(&marks),
                instruments,
                Arc::clone(&mixer),
                Arc::clone(&stop),
                Arc::clone(&paused),
                Arc::clone(&fx_lock),
                Arc::clone(&bpm_lock),
                counters.clone(),
                Arc::clone(&audio_config),
                Arc::clone(&filter_open),
                Arc::clone(&voice_reset),
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
            marks,
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
            visual,
            voice_reset,
        }
    }

    /// Hot style change. The same style is an idempotent no-op:
    /// starting again over a running loop must not erase
    /// manual slot volumes. A new style brings its cast for the
    /// current seed, room and highs; group faders are
    /// the listener's setting and survive the transition (details in mixer.rs).
    /// Emergency valve: if inherited faders would mute the
    /// new style entirely, groups are reset — silence never happens.
    fn set_style_live(&self, name: &str) -> bool {
        let Some(next) = styles::make_style(name) else {
            return false;
        };
        let already = match self.style.read() {
            Ok(g) => g.name() == next.name(),
            Err(poisoned) => poisoned.into_inner().name() == next.name(),
        };
        if already {
            return true;
        }
        // We write locks without poisoning panics: style/mix are loop
        // state, recovery matters more than strictness.
        let seed = snapshot(&self.seed);
        let mut guard = self.style.write().unwrap_or_else(|e| e.into_inner());
        let fx_next = effective_fx(&*next);
        *guard = next;
        let mut mixer = self.mixer.write().unwrap_or_else(|e| e.into_inner());
        apply_style_mix(&**guard, seed, &mut mixer);
        if !mixer.has_audible() {
            mixer.reset_groups();
            apply_style_mix(&**guard, seed, &mut mixer);
        }
        drop(mixer);
        *self.fx.write().unwrap_or_else(|e| e.into_inner()) = fx_next;
        true
    }

    /// Rebuild the cast for a seed without touching the listener's manual list.
    /// A seed change takes effect from the next step; the mixer updates immediately
    /// so the composer can't generate notes for a muted slot.
    fn refresh_mix_for_seed(&self, seed: u64) {
        let style = self.style.read().unwrap_or_else(|e| e.into_inner());
        let mut mixer = self.mixer.write().unwrap_or_else(|e| e.into_inner());
        apply_style_mix(&**style, seed, &mut mixer);
        if !mixer.has_audible() {
            mixer.reset_groups();
            apply_style_mix(&**style, seed, &mut mixer);
        }
    }

    /// Reading the mixer without panicking on a poisoned lock.
    fn with_mixer<T>(&self, f: impl FnOnce(&MixerState) -> T) -> T {
        match self.mixer.read() {
            Ok(g) => f(&g),
            Err(poisoned) => f(&poisoned.into_inner()),
        }
    }

    /// Mix edit with last-audible-slot protection:
    /// a change that would mute everything is silently rejected.
    fn update_mix(&self, apply: impl FnOnce(&mut MixerState)) {
        // We write without panicking on a poisoned lock: the mix is state,
        // not an invariant; refusing an edit is worse than recovery.
        let mut mixer = self.mixer.write().unwrap_or_else(|e| e.into_inner());
        let mut probe = mixer.clone();
        apply(&mut probe);
        if probe.has_audible() {
            *mixer = probe;
        }
    }

    fn effective_tempo(&self) -> f32 {
        // The frontend knob and the style default pass through the same validator:
        // status never shows a BPM the engine doesn't play.
        if let Some(bpm) = snapshot(&self.tempo).and_then(normalize_tempo) {
            return bpm;
        }
        let style_bpm = match self.style.read() {
            Ok(g) => g.bpm(),
            Err(poisoned) => poisoned.into_inner().bpm(),
        };
        normalize_tempo(style_bpm).unwrap_or(style_bpm)
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
        let list_edited = self.with_mixer(|mixer| mixer.list_edited);
        let fx = self
            .fx
            .read()
            .map(|g| *g)
            .unwrap_or_else(|e| *e.into_inner());
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
            instrument_list_edited: list_edited,
            reverb_wet: fx.reverb_wet,
            reverb_size: fx.reverb_size,
            treble_hz: fx.treble_hz,
            sidechain: fx.sidechain,
            tone_open: crate::core::tone_open_offset(),
        }
    }

    fn tracks(&self) -> Vec<TrackInfo> {
        self.with_mixer(|mixer| {
            Slot::ALL
                .iter()
                .filter(|slot| slot.group() != Group::Lead)
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
                .filter(|gr| **gr != Group::Lead)
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
    /// Pull-mode pump (the frontend plays on its own): created lazily by the first
    /// render_pull, survives calls (voices/tails intact). It lives SEPARATELY
    /// from the session mutex on purpose: rendering a block must NOT hold
    /// the session lock, otherwise every block serializes behind all UI calls
    /// (status, sliders, tracks) — the frontend lags, and more so
    /// the more voices there are in the mix.
    pull_pump: Mutex<Option<Pump>>,
}

impl PlaybackController {
    pub fn new() -> Self {
        Self {
            session: Mutex::new(None),
            pull_pump: Mutex::new(None),
        }
    }

    /// Starting the backend. Idempotent: if the loop is already running —
    /// it only applies style/seed live, streams aren't recreated.
    pub fn start(&self, style_name: &str, seed: u64, with_audio: bool) -> PlaybackStatus {
        let mut guard = self.session.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(session) = guard.as_mut() {
            if let Ok(mut s) = session.seed.write() {
                *s = seed;
            }
            // The style reads the already-new seed, and refresh finishes the cast if
            // the style didn't change and set_style_live returned an early no-op.
            session.set_style_live(style_name);
            session.refresh_mix_for_seed(seed);
            if with_audio && session.engine_handle.is_none() {
                let engine = AudioEngine::new(
                    Arc::clone(&session.queue),
                    Arc::clone(&session.marks),
                    build_instruments(),
                    Arc::clone(&session.mixer),
                    Arc::clone(&session.stop),
                    Arc::clone(&session.paused),
                    Arc::clone(&session.fx),
                    Arc::clone(&session.live_bpm),
                    session.counters.clone(),
                    Arc::clone(&session.audio_config),
                    Arc::clone(&session.filter_open),
                    Arc::clone(&session.voice_reset),
                );
                let sink = Arc::clone(&session.engine_error);
                session.engine_handle =
                    Some(spawn_guarded("engine", &session.engine_error, move || {
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
        // A new session — new shared objects: the old pump is dropped,
        // it will be recreated lazily by the first render_pull.
        *self.pull_pump.lock().unwrap_or_else(|e| e.into_inner()) = None;
        status
    }

    /// Stop: raises the stop flag and joins the threads.
    /// The only function that terminates the loop — on explicit request.
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

    /// Hot style change: swaps the object behind an RwLock and applies its
    /// default slot set. The loop keeps running.
    /// The same style is a no-op: manual volumes aren't erased.
    /// An unknown name is ignored — status returns the actual style.
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

    /// Hot seed change: the composer picks it up on the next step,
    /// and the mix is rebuilt for the new cast. The listener's manual list
    /// is untouched; group faders aren't touched — only slots.
    pub fn set_seed(&self, seed: u64) -> PlaybackStatus {
        let guard = self.session.lock().unwrap_or_else(|e| e.into_inner());
        match guard.as_ref() {
            Some(s) => {
                if let Ok(mut g) = s.seed.write() {
                    *g = seed;
                }
                s.refresh_mix_for_seed(seed);
                s.status()
            }
            None => PlaybackStatus::default(),
        }
    }

    /// Seed by code `XXX-XXX` for the frontend: garbage is sanitized, short
    /// input is rejected (status returns the old code — the frontend rolls the field back).
    pub fn set_seed_code(&self, code: &str) -> PlaybackStatus {
        match crate::seed::parse_seed_code(code) {
            Some(seed) => self.set_seed(seed),
            None => self.status(),
        }
    }

    /// Random seed for the "dice" button: the new code in the status.
    pub fn random_seed(&self) -> PlaybackStatus {
        use rand::Rng;
        let seed: u64 = rand::thread_rng().gen::<u64>() % crate::seed::CODE_SPACE;
        self.set_seed(seed)
    }

    /// Live tempo in BPM. Non-positive = reset to the style default.
    /// Applied from the next step, the loop isn't restarted.
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

    /// Live intensity 0..2 (the "temperature" of generation): higher — denser
    /// and brighter, lower — sparser and quieter. Default 1.0 = the style's character.
    /// Applied from the next step, the loop isn't restarted.
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

    /// The listener's master effects: room, high-cut, pump depth.
    /// Values are clamped to safe limits and live on top of the style
    /// defaults (reset via `reset_master_fx`). Applied live.
    pub fn set_master_fx(&self, wet: f32, size: f32, treble_hz: f32, sidechain: f32) -> PlaybackStatus {
        let next = MasterFx {
            reverb_wet: if wet.is_finite() { wet.clamp(0.0, 0.6) } else { 0.0 },
            reverb_size: if size.is_finite() { size.clamp(0.0, 1.0) } else { 0.5 },
            treble_hz: if treble_hz.is_finite() { treble_hz.clamp(2000.0, 20000.0) } else { 19000.0 },
            sidechain: if sidechain.is_finite() { sidechain.clamp(0.0, 2.0) } else { 1.0 },
        };
        if let Ok(mut g) = fx_override().write() {
            *g = Some(next);
        }
        let guard = self.session.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(s) = guard.as_ref() {
            if let Ok(mut g) = s.fx.write() {
                *g = next;
            }
            return s.status();
        }
        PlaybackStatus::default_with_fx(next)
    }

    /// Reset master effects to the current style's authored values.
    /// Filter openness is part of the style's sound, so it resets
    /// along with the room, timbre and sidechain.
    pub fn reset_master_fx(&self) -> PlaybackStatus {
        if let Ok(mut g) = fx_override().write() {
            *g = None;
        }
        crate::core::set_tone_open_offset(0.0);
        let guard = self.session.lock().unwrap_or_else(|e| e.into_inner());
        match guard.as_ref() {
            Some(s) => {
                let style = s.style.read().unwrap_or_else(|e| e.into_inner());
                let next = fx_for_style(&**style);
                drop(style);
                if let Ok(mut g) = s.fx.write() {
                    *g = next;
                }
                s.status()
            }
            None => PlaybackStatus::default(),
        }
    }

    /// Filter openness: a constant offset on top of the section form (-0.5..0.5).
    pub fn set_tone_open(&self, offset: f32) -> PlaybackStatus {
        crate::core::set_tone_open_offset(offset);
        self.status()
    }

    /// Slot volume live. Zero = the slot is excluded from generation and DSP.
    /// An unknown id is ignored. Muting the last audible
    /// slot is rejected (there must be no silence — the frontend rolls the slider back).
    pub fn set_track(&self, instrument_id: u64, volume: f32) -> PlaybackStatus {
        let guard = self.session.lock().unwrap_or_else(|e| e.into_inner());
        match guard.as_ref() {
            Some(s) => {
                let id = instrument_id as usize;
                let not_lead = Slot::from_id(id)
                    .is_some_and(|slot| slot.group() != Group::Lead);
                if ENSEMBLE_IDS.contains(&id) && not_lead {
                    s.update_mix(|mixer| mixer.set_track_by_user(id, volume));
                }
                s.status()
            }
            None => PlaybackStatus::default(),
        }
    }

    /// Reset the instrument list to the current style's and seed's defaults
    /// (the reset button in the mixer): the cast is rebuilt from the palette.
    /// The listener's manual choice is cleared, and the style can affect
    /// the lineup again.
    pub fn reset_instruments(&self) -> PlaybackStatus {
        let guard = self.session.lock().unwrap_or_else(|e| e.into_inner());
        match guard.as_ref() {
            Some(s) => {
                let style = match s.style.read() {
                    Ok(g) => g,
                    Err(poisoned) => poisoned.into_inner(),
                };
                let seed = snapshot(&s.seed);
                let mut mixer = s.mixer.write().unwrap_or_else(|e| e.into_inner());
                mixer.list_edited = false;
                apply_style_mix(&**style, seed, &mut mixer);
                drop(mixer);
                s.status()
            }
            None => PlaybackStatus::default(),
        }
    }

    /// Group macro fader live ("kick", "lead", …). A multiplier on top of
    /// the slots: zero mutes the whole group (excludes it from generation).
    /// An unknown name is ignored. Survives style changes.
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

    /// Live mix for UI: slots with names, roles and volumes.
    pub fn tracks(&self) -> Vec<TrackInfo> {
        let guard = self.session.lock().unwrap_or_else(|e| e.into_inner());
        match guard.as_ref() {
            Some(s) => s.tracks(),
            None => Vec::new(),
        }
    }

    /// Macro faders for UI: group name and its multiplier.
    pub fn groups(&self) -> Vec<GroupInfo> {
        let guard = self.session.lock().unwrap_or_else(|e| e.into_inner());
        match guard.as_ref() {
            Some(s) => s.groups(),
            None => Vec::new(),
        }
    }

    /// Live pipeline diagnostics for the UI banner. Zeros without a session.
    pub fn engine_stats(&self) -> EngineStats {
        let guard = self.session.lock().unwrap_or_else(|e| e.into_inner());
        match guard.as_ref() {
            Some(s) => EngineStats {
                frames: s.counters.frames.load(Ordering::Relaxed),
                pushed: s.counters.pushed.load(Ordering::Relaxed),
                consumed: s.counters.consumed.load(Ordering::Relaxed),
                dropped: s.counters.dropped.load(Ordering::Relaxed),
                late: s.counters.late.load(Ordering::Relaxed),
                reanchors: s.counters.reanchors.load(Ordering::Relaxed),
                queued: s.queue.len() as u64,
                audio_config: snapshot(&s.audio_config),
                restarts: s.counters.restarts.load(Ordering::Relaxed),
                output_latency_ms: crate::pull_thread::output_latency_ms(),
            },
            None => EngineStats::default(),
        }
    }

    /// Pull render for the frontend (Android plays on its own via AudioTrack):
    /// stereo interleaved f32, `frames` frames. The pump is created lazily
    /// and survives calls; a rate change retunes without losing voices.
    /// We hold the session lock only to clone the shared parts —
    /// the render itself goes without it (otherwise UI lags). Without a session — empty.
    /// A 4-second cap per call, as a safety against fools.
    /// The scheduling quantum lives inside: the block is cut into ~12 ms subblocks,
    /// the queue is drained at the start of each (not once per whole block).
    /// Otherwise all notes of a block start at once — a volley instead of a groove.
    /// Desktop callbacks are already small, this changes nothing for them.
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
            marks: Arc<ArrayQueue<crate::composer::StepMark>>,
            mixer: Arc<RwLock<MixerState>>,
            paused: Arc<AtomicBool>,
            fx: SharedFx,
            live_bpm: Arc<RwLock<f32>>,
            filter_open: Arc<RwLock<f32>>,
            counters: EngineCounters,
            voice_reset: Arc<AtomicBool>,
        }
        let parts: Option<Parts> = {
            let guard = self.session.lock().unwrap_or_else(|e| e.into_inner());
            guard.as_ref().map(|s| Parts {
                queue: Arc::clone(&s.queue),
                marks: Arc::clone(&s.marks),
                mixer: Arc::clone(&s.mixer),
                paused: Arc::clone(&s.paused),
                fx: Arc::clone(&s.fx),
                live_bpm: Arc::clone(&s.live_bpm),
                filter_open: Arc::clone(&s.filter_open),
                counters: s.counters.clone(),
                voice_reset: Arc::clone(&s.voice_reset),
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
                    p.marks,
                    build_instruments(),
                    p.mixer,
                    p.paused,
                    p.fx,
                    p.live_bpm,
                    sr,
                    p.counters,
                    p.filter_open,
                    p.voice_reset.clone(),
                ));
                pump_guard.as_mut().expect("pump just created")
            }
        };
        let mut out = vec![0.0f32; n * 2];
        // Subblocks of ~12 ms (like desktop callbacks): onsets are more accurate
        // than the grid step for any frontend request size.
        const SUB_FRAMES: usize = 600;
        for chunk in out.chunks_mut(SUB_FRAMES * 2) {
            pump.fill(chunk, 2, f32_identity);
        }
        out
    }

    /// Length of the pump's due schedule (how many notes wait for their frame).
    /// For diagnostics: growing — the schedule is holding notes hostage.
    /// try_lock, so as not to stall the audio stream for a measurement.
    pub fn pump_pending_len(&self) -> usize {
        self.pull_pump
            .try_lock()
            .map(|g| g.as_ref().map(|p| p.pending_len()).unwrap_or(0))
            .unwrap_or(0)
    }

    pub fn status(&self) -> PlaybackStatus {
        let guard = self.session.lock().unwrap_or_else(|e| e.into_inner());
        match guard.as_ref() {
            Some(s) => s.status(),
            None => PlaybackStatus::default(),
        }
    }

    /// Visualizer frame from the live session. Zeros without a session.
    pub fn live_visual(&self) -> LiveVisual {
        let guard = self.session.lock().unwrap_or_else(|e| e.into_inner());
        let snap = guard.as_ref().map(|s| snapshot(&s.visual));
        match snap {
            Some(v) => LiveVisual {
                bar_phase: v.step_phase,
                energy: v.energy,
                section: v.section,
                motif: v.motif.to_vec(),
                motif_pos: v.motif_pos,
                bar_hits: v.bar_hits,
                kick_hits: v.kick_hits,
                snare_hits: v.snare_hits,
                bass_hits: v.bass_hits,
                theme_hits: v.theme_hits,
                phrase_len: v.phrase_len,
                phrase_pos: v.phrase_pos,
                phrase_phase: v.phrase_phase,
                window_phase: v.window_phase,
                pkick: v.pkick,
                psnare: v.psnare,
                pbass: v.pbass,
                ptheme: v.ptheme,
                pplayed: v.pplayed,
            },
            None => LiveVisual::default(),
        }
    }
}

impl Default for PlaybackController {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Offline render: generation without an audio device. Used by the CLI's
// `--render N` flag to check that style/seed really affect the music.
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
    /// (instrument_id, note_count), sorted by id.
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
        apply_style_mix(&*style, seed, &mut m);
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
// Soak run: generation + the whole DSP pipeline offline for N seconds.
// Proof of endless stability: all memory is fixed
// (arrays, ring buffers, bounded queue), threads don't multiply,
// counters don't overflow in the lifetime of the universe. It runs the same
// RenderState and the same Composer::next_step as the live pipeline.
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

    let mixer = Arc::new(RwLock::new(MixerState::new()));
    {
        let mut m = mixer.write().unwrap_or_else(|e| e.into_inner());
        apply_style_mix(&*style, seed, &mut m);
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
// WAV export: the first N seconds of generation into a PCM16 file for listening.
// The same pipeline as soak (Composer + RenderState), plus sample writing.
// No external dependencies: the 44-byte WAV header is written by hand.
// ---------------------------------------------------------------------------

/// Renders `seconds` seconds of a style/seed into WAV PCM16 stereo.
/// `extra` forces slots on at base volume
/// (auditioning manual voices: `--with 19`). Unknown ids are silently
/// skipped. Returns a report (as soak does).
/// `solo`: if non-empty — ONLY the listed slots sound
/// (the style's default mix isn't applied): a spotlight render for
/// sound design (`--solo 6`). Unknown ids are skipped.
pub fn render_wav(
    style_name: &str,
    seed: u64,
    seconds: f32,
    sample_rate: f32,
    path: &str,
    extra: &[usize],
    solo: &[usize],
) -> Result<SoakReport, String> {
    use crate::core::MAX_ACTIVE_TRACKS;
    use crate::engine::RenderState;

    let style =
        styles::make_style(style_name).ok_or_else(|| format!("unknown style '{style_name}'"))?;
    let style_name_out = style.name().to_string();
    let sr = if sample_rate.is_finite() && sample_rate > 0.0 {
        sample_rate
    } else {
        44_100.0
    };
    let wet = style.reverb_wet();
    let size = style.reverb_size();
    let treble = style.treble_cutoff();

    let mixer = Arc::new(RwLock::new(MixerState::new()));
    {
        let mut m = mixer.write().unwrap_or_else(|e| e.into_inner());
        if solo.is_empty() {
            apply_style_mix(&*style, seed, &mut m);
            for id in extra {
                if let Some(slot) = Slot::from_id(*id) {
                    m.set_track(slot.id(), slot.base_volume());
                }
            }
        } else {
            for id in solo {
                if let Some(slot) = Slot::from_id(*id) {
                    m.set_track(slot.id(), slot.base_volume());
                }
            }
        }
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

    write_wav_pcm16(path, &pcm, 2, sr as u32).map_err(|e| format!("cannot write '{path}': {e}"))?;

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

/// Minimal WAV writer: PCM16 stereo, 44-byte header.
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

        // Start without audio (no audio device in CI).
        let s = c.start("club", 42, false);
        assert!(s.running);
        assert_eq!(s.style_name, "club");
        assert_eq!(s.seed, 42);
        assert_eq!(s.tempo_bpm, 126.0);

        // The default club cast — deep house: punch, sub, snare, shaker,
        // the theme's pluck and Rhodes stabs. Pad is a palette color:
        // a seed may take them or keep the club dry.
        let tracks = c.tracks();
        // The Lead group was removed: the mix has no pluck, guitar and guitar lead.
        let visible = Slot::ALL
            .iter()
            .filter(|s| s.group() != Group::Lead)
            .count();
        assert_eq!(tracks.len(), visible);
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
        // There's no hat in club: the shaker holds the rhythm (a bright "triangle" on
        // top killed the character).
        assert_eq!(vol(HAT_ID), 0.0);
        assert_eq!(vol(KICK808_ID), 0.0);
        assert!(vol(KEYS_ID) > 0.0);
        assert!(
            tracks.iter().all(|t| t.id != PLUCK_ID as u64),
            "lead group must be absent from the mixer"
        );
        assert!(vol(SHAKER_ID) > 0.0);
        assert_eq!(vol(VINYL_ID), 0.0);
        // Beyond the core — only the club palette's colors, no foreign guests.
        let guests: Vec<u64> = tracks
            .iter()
            .filter(|t| {
                t.volume > 0.0
                    && ![
                        BASS_ID, KICK_ID, SNARE_ID, HAT_ID, PLUCK_ID, KEYS_ID, SHAKER_ID,
                    ]
                    .contains(&(t.id as usize))
            })
            .map(|t| t.id)
            .collect();
        for id in &guests {
            let slot = Slot::from_id(*id as usize).expect("known slot");
            assert!(
                [Slot::Pad, Slot::Piano].contains(&slot),
                "stranger guest: {slot:?}"
            );
        }

        // The loop actually generates notes.
        assert!(wait_for_notes(&c, Duration::from_secs(3)));

        // Groups: Lead removed, 7 left; one Kick group for two kicks.
        assert_eq!(c.groups().len(), 6);
        let s = c.set_group_volume("kick", 0.5);
        assert!(s.running);
        let kick = c
            .tracks()
            .iter()
            .find(|t| t.id == KICK_ID as u64)
            .unwrap()
            .clone();
        assert_eq!(kick.group, "kick");
        assert_eq!(kick.volume, 0.95 * 0.5); // 0.95 slot × 0.5 group
        assert!(c.set_group_volume("nope", 0.5).running); // foreign name ignored
        assert!(c.status().running);

        // A repeated start doesn't spawn streams, only applies parameters.
        let s = c.start("club", 42, false);
        assert!(s.running);

        // Pause/play don't stop the loop.
        assert!(c.set_paused(true).paused);
        assert!(!c.set_paused(false).paused);
        assert!(c.status().running);

        // Live tempo: clamping and reset to default, loop stays alive.
        assert_eq!(c.set_tempo_bpm(100.0).tempo_bpm, 100.0);
        assert_eq!(
            c.set_tempo_bpm(1000.0).tempo_bpm,
            crate::core::MAX_TEMPO_BPM
        );
        assert_eq!(c.set_tempo_bpm(0.0).tempo_bpm, 126.0);
        // Intensity: clamped 0..2, default 1.0.
        assert_eq!(c.set_intensity(1.5).intensity, 1.5);
        assert_eq!(c.set_intensity(99.0).intensity, 2.0);
        assert_eq!(c.set_intensity(-1.0).intensity, 0.0);
        assert!(c.status().running);

        // Hot style change: lofi removes the 808 and laser, turns on vinyl;
        // group faders are the listener's setting and survive the transition
        // (kick stays at 0.5, no reset).
        let s = c.set_style("lofi");
        assert_eq!(s.style_name, "lofi");
        let tracks = c.tracks();
        let kick808 = tracks.iter().find(|t| t.id == KICK808_ID as u64).unwrap();
        assert_eq!(kick808.volume, 0.0);
        let vinyl = tracks.iter().find(|t| t.id == VINYL_ID as u64).unwrap();
        assert!(vinyl.volume > 0.0);
        assert_eq!(
            c.groups().iter().find(|g| g.name == "kick").unwrap().volume,
            0.5
        );
        // A repeated set_style with the same name is a no-op: manual edits
        // to slots aren't erased.
        let before: Vec<f32> = c.tracks().iter().map(|t| t.volume).collect();
        assert_eq!(c.set_style("lofi").style_name, "lofi");
        let after: Vec<f32> = c.tracks().iter().map(|t| t.volume).collect();
        assert_eq!(before, after);
        assert!(c.status().running);
        // An unknown style is ignored.
        assert_eq!(c.set_style("nope").style_name, "lofi");
        assert_eq!(c.set_seed(7).seed, 7);

        // Volume 0 excludes a slot; unknown ids are ignored.
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
        // The last audible slot can't be muted: we restore the bass,
        // mute all groups except bass, then try that too —
        // rejected, sound remained.
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

        // Stop is the only point where the loop ends.
        let s = c.stop();
        assert!(!s.running);
        assert!(!c.status().running);
    }

    #[test]
    fn pull_pump_lives_outside_session_lock() {
        // Rendering a block does NOT hold the session lock: otherwise each block
        // serializes status/sliders/tracks behind it and the frontend lags
        // the more voices there are (a regression from a phone).
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
        // Status/stats from 4 threads during renders:
        // it completes (lock order is consistent), doesn't hang.
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
        // Legacy without a stamp (tests, direct calls): the backlog is drained
        // in portions by subblocks (cap 32), not all at once.
        // The composer adds more on top, so the bounds have headroom,
        // but the gap is an exact multiple.
        let c = PlaybackController::new();
        c.start("club", 42, false);
        let push100 = || {
            let guard = c.session.lock().unwrap_or_else(|e| e.into_inner());
            let s = guard.as_ref().expect("session");
            for i in 0..100 {
                let _ = s
                    .queue
                    .push(NoteEvent::new(PLUCK_ID, 60 + (i % 12) as u8, 0.8, 0.2));
            }
        };
        push100();
        let before = c.engine_stats().consumed;
        let out = c.render_pull(600, 44_100.0);
        assert_eq!(out.len(), 1200);
        assert!(out.iter().all(|s| s.is_finite()));
        let eaten = c.engine_stats().consumed - before;
        assert!(eaten < 60, "backlog volley in one subblock: {eaten}");
        // And the whole 4800 is drained by all eight portions.
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
        // Frontend knobs reach generation: status reflects
        // note density following intensity.
        let c = PlaybackController::new();
        c.start("club", 11, false);
        let s = c.set_intensity(0.1);
        assert!((s.intensity - 0.1).abs() < 1e-5, "intensity ignored");
        let s = c.set_tempo_bpm(100.0);
        assert!((s.tempo_bpm - 100.0).abs() < 1e-3, "tempo ignored");
        // Quiet generation gives fewer notes than hot generation.
        std::thread::sleep(Duration::from_millis(300));
        c.set_intensity(0.1);
        let _ = c.render_pull(4800, 44_100.0);
        let cold = c.engine_stats().pushed;
        c.set_intensity(2.0);
        std::thread::sleep(Duration::from_millis(1500));
        let _ = c.render_pull(4800, 44_100.0);
        let hot = c.engine_stats().pushed;
        assert!(
            hot > cold,
            "intensity does not drive density: {cold} vs {hot}"
        );
        let _ = c.stop();
    }

    #[test]
    fn set_seed_rebuilds_cast_until_user_edits_list() {
        // The seed is part of the composition: a seed change rebuilds the lineup, until
        // the listener has edited the list themselves. A manual list freezes the cast.
        use crate::instruments::Slot;
        let c = PlaybackController::new();
        c.start("club", 1, false);
        let style = styles::make_style("club").expect("club");
        let active = |c: &PlaybackController| -> Vec<usize> {
            c.tracks()
                .into_iter()
                .filter(|t| t.volume > 0.0)
                .map(|t| t.id as usize)
                .collect()
        };
        let base = style.cast(1);
        let next = (2u64..256)
            .find(|seed| style.cast(*seed) != base)
            .expect("no other cast");
        c.set_seed(next);
        assert_eq!(c.status().seed, next);
        let expected: Vec<usize> = style
            .cast(next)
            .iter()
            .filter(|s| s.group() != Group::Lead)
            .map(|s| s.id())
            .collect();
        assert_eq!(active(&c), expected, "set_seed didn't rebuild the cast");
        for slot in style.cast(next).into_iter().filter(|s| s.group() != Group::Lead) {
            let vol = c
                .tracks()
                .iter()
                .find(|t| t.id == slot.id() as u64)
                .map(|t| t.volume)
                .unwrap_or(-1.0);
            assert_eq!(vol, slot.base_volume(), "{slot:?}: not the base volume");
        }

        // A manual edit freezes the list: a new seed doesn't touch volumes.
        // Lead slots are no longer available — we edit the pad.
        let pad = Slot::Pad.id() as u64;
        c.set_track(pad, 0.0);
        let before: Vec<(u64, f32)> = c.tracks().into_iter().map(|t| (t.id, t.volume)).collect();
        let last = (next + 1..next + 256)
            .find(|seed| style.cast(*seed) != style.cast(next))
            .expect("no third cast");
        c.set_seed(last);
        let after: Vec<(u64, f32)> = c.tracks().into_iter().map(|t| (t.id, t.volume)).collect();
        assert_eq!(after, before, "manual list overwritten by seed change");
        let _ = c.stop();
    }

    #[test]
    fn user_list_survives_style_change_until_reset() {
        // Listener's rule: editing the instrument list takes priority.
        // A style (and seed) change doesn't touch the manual list until
        // reset is pressed: reset restores style defaults and clears the flag.
        let c = PlaybackController::new();
        c.start("club", 11, false);
        let pad_id = crate::instruments::Slot::Pad.id() as u64;
        let st = c.set_track(pad_id, 0.0);
        assert!(st.instrument_list_edited, "edit not marked");
        // Style change: the list stays the listener's.
        c.set_style("lofi");
        let tracks = c.tracks();
        let pad = tracks
            .iter()
            .find(|t| t.id == pad_id)
            .expect("pad in list");
        assert_eq!(pad.volume, 0.0, "manual list overwritten by style");
        // A seed change — too (a seed's cast isn't imposed over a manual one).
        c.set_seed(999);
        let tracks = c.tracks();
        let pad = tracks
            .iter()
            .find(|t| t.id == pad_id)
            .expect("pad in list");
        assert_eq!(pad.volume, 0.0, "manual list overwritten by seed");
        // Reset: style defaults and the flag cleared.
        let st = c.reset_instruments();
        assert!(!st.instrument_list_edited, "flag not cleared by reset");
        let tracks = c.tracks();
        let bass = tracks.iter().find(|t| t.id == BASS_ID as u64).unwrap();
        assert!(bass.volume > 0.0, "reset didn't restore the style's lineup");
        assert!(
            tracks.iter().all(|t| t.group != "lead"),
            "lead group must not return after reset"
        );
        let _ = c.stop();
    }

    #[test]
    fn style_change_rewrites_the_list_when_untouched() {
        // The list wasn't touched: style and seed govern it as before.
        let c = PlaybackController::new();
        c.start("club", 11, false);
        c.set_style("lofi");
        let tracks = c.tracks();
        // Lead is removed from the product entirely: no slot of that group
        // should appear in the mix in any style.
        assert!(
            tracks.iter().all(|t| t.group != "lead"),
            "lead group still in list"
        );
        let bass = tracks.iter().find(|t| t.id == BASS_ID as u64).unwrap();
        assert!(bass.volume > 0.0, "style bass missing from lineup");
        let _ = c.stop();
    }

    #[test]
    fn live_visual_tracks_song() {
        // Snapshot for the visualizer: the live session gives the bar phase,
        // energy and masks of sounding beats (overall and kick);
        // after stop — zeros without a panic.
        let c = PlaybackController::new();
        c.start("club", 11, false);
        std::thread::sleep(Duration::from_millis(400));
        let v = c.live_visual();
        assert_eq!(v.motif.len(), 16);
        assert!(v.energy > 0.0, "snapshot stuck at default");
        assert!((0.0..=1.0).contains(&v.bar_phase));
        assert!(v.motif_pos < 16);
        assert_ne!(v.bar_hits, 0, "no sounded steps in snapshot");
        assert_ne!(v.kick_hits, 0, "kick mask empty: four-floor lost");
        // Phrase notes accumulate as they sound: length 32, position in it,
        // kick quarters appear as played (all poles hold beats).
        assert_eq!(v.phrase_len, 32, "phrase plan broken");
        assert!(v.phrase_pos < 32, "phrase pos out of plan");
        assert_ne!(
            v.pkick & 0b00010001000100010001000100010001,
            0,
            "no downbeats in the phrase"
        );
        let _ = c.stop();
        let v = c.live_visual();
        assert_eq!(v.energy, 0.0);
    }

    #[test]
    fn status_carries_engine_version() {
        // Engine version travels in status: explains seeds' sound changes.
        let c = PlaybackController::new();
        assert_eq!(c.status().engine_version, crate::core::ENGINE_VERSION);
        c.start("club", 11, false);
        assert_eq!(c.status().engine_version, crate::core::ENGINE_VERSION);
        let _ = c.stop();
    }

    #[test]
    fn pull_renders_finite_blocks() {
        // Frontend pull mode: start without audio, then sample blocks.
        // Size = frames × 2 channels, everything finite.
        let c = PlaybackController::new();
        let s = c.start("club", 42, false);
        assert!(s.running);
        // We let the composer put notes into the queue.
        std::thread::sleep(Duration::from_secs(2));
        let out = c.render_pull(44_100, 44_100.0);
        assert_eq!(out.len(), 88_200);
        assert!(out.iter().all(|s| s.is_finite()));
        // A rate change retunes the pump without losing state.
        let out2 = c.render_pull(48_000, 48_000.0);
        assert_eq!(out2.len(), 96_000);
        assert!(out2.iter().all(|s| s.is_finite()));
        // Without a session — empty, without a panic.
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
        // A user complaint: the first bars of seeds 123 and 321 sounded
        // the same. The groove's poles, scale rotation and progression must give
        // different music from the start of the groove (first two bars: the heartbeat
        // is the same, then the patterns diverge).
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
                // We drive the song like the live engine: otherwise all 64 steps
                // stick out in the first bar of the intro and the test lies.
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
        // "Endless" variations: scale rotation, modal variant,
        // density and RNG stream — all from the seed. Two seeds of one style
        // must give noticeably different music. We compare the full
        // distribution across instruments, not totals (totals can coincide
        // by chance while the music differs).
        let a = render_offline("club", 11, 64).expect("renders");
        let b = render_offline("club", 12, 64).expect("renders");
        assert_ne!(
            a.notes_per_instrument, b.notes_per_instrument,
            "seeds 11 and 12 sound identical"
        );
    }

    #[test]
    fn pole_grooves_engage_on_some_seeds() {
        // Poles really switch: a kick syncopation on step 14
        // occurs for some seeds and is absent for others.
        // (No more morphing — variety comes only from poles and gates.)
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
        // Regression "no bass in the bass": bass notes of any style must
        // lie in 28..=52 (A0..E3 ≈ 41..165 Hz), not in the lower midrange.
        // The upper bound is inclusive: octave jumps are bass too.
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
    fn wav_solo_renders_only_listed_slots() {
        // Spotlight: only the pluck sounds (id 6), the pipeline is the same.
        let path = std::env::temp_dir().join("cont_wav_solo_test.wav");
        let path_str = path.to_string_lossy().to_string();
        let report =
            render_wav("club", 11, 2.0, 22_050.0, &path_str, &[], &[6]).expect("renders wav");
        assert_eq!(report.non_finite, 0);
        assert!(report.notes > 0, "solo pluck silent");
        assert!(report.peak > 0.01, "solo pluck inaudible");
    }

    #[test]
    fn wav_export_writes_valid_file() {
        // 2 seconds at a reduced rate: fast, but the whole real pipeline.
        let path = std::env::temp_dir().join("cont_wav_export_test.wav");
        let path_str = path.to_string_lossy().to_string();
        let report =
            render_wav("club", 11, 2.0, 22_050.0, &path_str, &[], &[]).expect("renders wav");
        assert_eq!(report.samples, 44_100);
        assert_eq!(report.non_finite, 0);

        let bytes = std::fs::read(&path).expect("wav readable");
        // RIFF/WAVE magic + size = header + samples × 2 channels × 2 bytes.
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        assert_eq!(&bytes[12..16], b"fmt ");
        assert_eq!(&bytes[36..40], b"data");
        assert_eq!(bytes.len(), 44 + 44_100 * 2 * 2);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn mix_brightness_stays_in_ref_bands() {
        // Mix brightness in reference bands: lofi is dark (dust, not sharpness),
        // club is bright (hat and clap audible). The metric is the high-frequency energy fraction
        // (first-difference) over 20 seconds of generation with the default mix.
        // Measurements: lofi 0.0008 (Autumn 0.0107 — we're darker, that's fine),
        // club 0.0019. The bands are wide: they catch regressions, not taste.
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
                apply_style_mix(&*style, seed, &mut m);
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
        // Lo-fi stays duller than club, but not duller than the reference:
        // returned to the lineup (atmosphere), and the band became 0.004 with
        // the reference at 0.0107.
        assert!(lofi < 0.006, "lofi too bright: {lofi}");
        let club = hf_ratio("club", 11);
        assert!(club > 0.001, "club too dark: {club}");
    }

    #[test]
    fn soak_proves_long_run_stability() {
        // The whole pipeline (generation + DSP) for 6 seconds at a reduced rate:
        // finiteness, level, generation doesn't go silent. The first bar is
        // already the theme at full strength (no build-up), the note threshold holds density.
        // Days/time — via the CLI's --soak flag, the code is the very same.
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
        // Mastering contract: rms in the reference band (commercial ~0.3,
        // ours was ~0.1), peaks under the limiter ceiling, crest factor alive
        // (transients aren't squashed: below 3.5 — mush, above 13 — quiet).
        // Floor 3.5 (was 4.0): hats landed on the grid — the mix is denser,
        // the crest is lower, that's an even beat, not squashing.
        let report = soak_render("club", 11, 12.0, 22050.0).expect("soaks");
        assert_eq!(report.non_finite, 0);
        assert!(report.peak <= 1.0, "limiter leaks: {}", report.peak);
        assert!(
            (0.12..=0.30).contains(&report.rms),
            "rms out of band: {}",
            report.rms
        );
        let crest = report.peak / report.rms.max(1e-6);
        assert!(
            (3.5..=13.0).contains(&crest),
            "transients squashed or mix dead: crest {crest}"
        );
    }

    #[test]
    fn style_palettes_match_character() {
        // The palette is the style's colors, not the default lineup. Signature
        // instruments are in the palette, foreign ones didn't sneak in; each
        // seed's cast keeps the foundation and signatures but changes the colors.
        use Slot::*;
        let style_of = |name: &str| styles::make_style(name).expect("known style");

        let club = style_of("club");
        let club_palette = club.ensemble().to_vec();
        for s in [Bass, Kick, Snare, Pluck, Keys, Shaker] {
            assert!(club_palette.contains(&s), "club: {s:?} missing");
        }
        // Hat and foreign instruments aren't in the club palette; pad is
        // rare colors, only some seeds take them.
        for s in [Hat, Kick808, Guitar, Vinyl, GuitarLead, Rain] {
            assert!(!club_palette.contains(&s), "club: {s:?} should be off");
        }
        assert!(club_palette.contains(&Pad), "club: Pad color missing");

        let lofi = style_of("lofi");
        let lofi_palette = lofi.ensemble().to_vec();
        for s in [Bass, Kick, Pad, Snare, Hat, Keys, Vinyl, Guitar] {
            assert!(lofi_palette.contains(&s), "lofi: {s:?} missing");
        }
        // Pluck is outside the palette: over Rhodes and guitar it makes mush.
        for s in [Pluck, Kick808, GuitarLead, Shaker, Rain] {
            assert!(!lofi_palette.contains(&s), "lofi: {s:?} should be off");
        }

        // All styles: a nonempty palette of known slots.
        for name in styles::style_names() {
            let palette = style_of(name).ensemble().to_vec();
            assert!(!palette.is_empty(), "{name}: empty palette");
            for s in &palette {
                assert!(Slot::ALL.contains(s), "{name}: unknown slot {s:?}");
            }
        }

        // A seed's cast: only palette colors, but the foundation, signatures and
        // carriers of harmony/theme are in place.
        for name in styles::style_names() {
            let style = style_of(name);
            let palette = style.ensemble();
            for seed in [1u64, 2, 3, 42, 777] {
                let cast = style.cast(seed);
                for s in &cast {
                    assert!(palette.contains(s), "{name}/{seed}: {s:?} outside palette");
                }
                for s in [Bass, Kick, Snare] {
                    assert!(cast.contains(&s), "{name}/{seed}: {s:?} lost");
                }
                for s in style.signature_slots() {
                    assert!(cast.contains(s), "{name}/{seed}: signature {s:?} missing");
                }
                for group in [Group::Hat, Group::Chords, Group::Lead] {
                    assert!(
                        cast.iter().any(|s| s.group() == group),
                        "{name}/{seed}: group {group:?} empty ({cast:?})"
                    );
                }
            }
        }
    }

    #[test]
    fn mixes_match_seed_cast_with_base_volumes() {
        // The mix = the seed's cast: selected slots at canonical volume,
        // everything else strictly at zero. Another seed — another cast.
        for name in styles::style_names() {
            let style = styles::make_style(name).expect("known style");
            let mut casts = std::collections::HashSet::new();
            for seed in [1u64, 2, 3, 42, 777] {
                let cast = style.cast(seed);
                casts.insert(cast.clone());
                let mut mixer = MixerState::new();
                apply_style_mix(&*style, seed, &mut mixer);
                for slot in Slot::ALL {
                    let vol = mixer.volume_for(slot.id());
                    if slot.group() == Group::Lead {
                        assert_eq!(vol, 0.0, "{name}/{seed}: {slot:?} lead must stay off");
                        continue;
                    }
                    if cast.contains(&slot) {
                        assert_eq!(vol, slot.base_volume(), "{name}/{seed}: {slot:?}");
                    } else {
                        assert_eq!(vol, 0.0, "{name}/{seed}: {slot:?} should be muted");
                    }
                }
            }
            assert!(casts.len() > 1, "{name}: cast doesn't change with seed");
        }
    }
}
