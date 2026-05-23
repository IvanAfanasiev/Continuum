mod audio_engine;
mod composer;

use crossbeam_queue::ArrayQueue;
use std::sync::Arc;

pub struct NoteEvent {
    pub note: f32, // frequency
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