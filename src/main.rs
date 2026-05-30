mod audio_engine;
mod composer;
mod instruments;
mod markov;

use instruments::{EnvelopeConfig, Instrument};
use crossbeam_queue::ArrayQueue;
use std::sync::Arc;

pub struct NoteEvent {
    pub note:       u8,
    pub velocity:   f32,
    pub duration:   f32,
    pub instrument: Instrument,
    pub envelope:   EnvelopeConfig,
}

fn main() {
    println!("[main] Starting Continuum");

    let queue          = Arc::new(ArrayQueue::<NoteEvent>::new(512));
    let audio_queue    = queue.clone();
    let composer_queue = queue.clone();

    std::thread::spawn(move || audio_engine::start_engine(audio_queue));
    std::thread::spawn(move || composer::start_composing(composer_queue));

    loop { std::thread::sleep(std::time::Duration::from_secs(1)); }
}

pub fn midi_to_freq(note: u8) -> f32 {
    440.0 * 2.0f32.powf((note as f32 - 69.0) / 12.0)
}
