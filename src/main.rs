mod audio_engine;
mod composer;

use crossbeam_queue::ArrayQueue;
use std::sync::Arc;

pub struct NoteEvent {
    pub note: u8,       // MIDI note number (0–127)
    pub velocity: f32,  // amplitude (0.0–1.0)
    pub duration: f32,  // duration in milliseconds
}

fn main() {
    let queue = Arc::new(ArrayQueue::new(256));

    let audio_queue = queue.clone();
    let composer_queue = queue.clone();

    let audio_thread = std::thread::spawn(move || {
        audio_engine::start_engine(audio_queue);
    });

    let composer_thread = std::thread::spawn(move || {
        composer::start_composing(composer_queue);
    });

    audio_thread.join().unwrap();
    composer_thread.join().unwrap();
}

pub fn midi_to_freq(note: u8) -> f32 {
    440.0 * 2.0f32.powf((note as f32 - 69.0) / 12.0)
}
