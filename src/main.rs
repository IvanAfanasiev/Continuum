mod audio_engine;
mod composer;

use crossbeam_queue::ArrayQueue;
use std::sync::Arc;

pub struct NoteEvent {
    pub note: u8, // MIDI note number
    pub velocity: f32, // amplitude
    pub duration: f32,
}

fn main() {
    // 100 events
    let queue = Arc::new(ArrayQueue::new(100));

    // clone for both threads
    let audio_queue = queue.clone();
    let composer_queue = queue.clone();

    // start the audio engine (consumer)
    std::thread::spawn(move || {
        audio_engine::start_engine(audio_queue);
    });

    // start the composer (producer)
    composer::start_composing(composer_queue);
}
pub fn midi_to_freq(note: u8) -> f32 {
    440.0 * 2.0f32.powf((note as f32 - 69.0) / 12.0)
}