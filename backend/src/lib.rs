pub mod audio_engine;
pub mod bridge;
pub mod composer;
pub mod controls;
pub mod instruments;
pub mod markov;
pub mod runtime;

pub mod phrase;
pub mod section;
pub mod step;
pub mod theory;

pub use controls::RuntimeControls;
pub use instruments::{EnvelopeConfig, Instrument};

#[derive(Debug, Clone, Copy)]
pub struct NoteEvent {
    pub note: u8,
    pub velocity: f32,
    pub duration: f32,
    pub start_delay_ms: f32,
    pub instrument: Instrument,
    pub envelope: EnvelopeConfig,
    pub is_phrase_start: bool,
    pub is_phrase_end: bool,
}

pub fn midi_to_freq(note: u8) -> f32 {
    440.0 * 2.0f32.powf((note as f32 - 69.0) / 12.0)
}
