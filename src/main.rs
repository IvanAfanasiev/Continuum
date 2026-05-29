mod audio_engine;
mod composer;
mod markov;

use crossbeam_queue::ArrayQueue;
use std::sync::Arc;

pub struct NoteEvent {
    pub note:     u8,  // MIDI note number (0-127)
    pub velocity: f32, // amplitude (0.0-1.0)
    pub duration: f32, // milliseconds
}

fn main() {
    println!("[main] Starting Continuum");

    let queue          = Arc::new(ArrayQueue::<NoteEvent>::new(256));
    let audio_queue    = queue.clone();
    let composer_queue = queue.clone();

    std::thread::spawn(move || audio_engine::start_engine(audio_queue));
    std::thread::spawn(move || composer::start_composing(composer_queue));

    loop { std::thread::sleep(std::time::Duration::from_secs(1)); }
}

pub fn midi_to_freq(note: u8) -> f32 {
    440.0 * 2.0f32.powf((note as f32 - 69.0) / 12.0)
}
