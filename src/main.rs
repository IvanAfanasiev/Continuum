mod audio_engine;
mod composer;
mod markov;

use crossbeam_queue::ArrayQueue;
use std::sync::Arc;

pub struct NoteEvent {
    pub note:     u8,   // MIDI note number (0-127)
    pub velocity: f32,  // amplitude (0.0-1.0)
    pub duration: f32,  // duration in milliseconds
}

fn main() {
    println!("[main] Starting Continuum");

    let audio_queue    = Arc::new(ArrayQueue::<NoteEvent>::new(256));
    let composer_queue = audio_queue.clone();

    // Thread 1: audio engine — synthesizes sound from NoteEvents
    std::thread::spawn(move || {
        audio_engine::start_engine(audio_queue);
    });

    // Thread 2: composer — Markov chain generates notes and paces them
    std::thread::spawn(move || {
        composer::start_composing(composer_queue);
    });

    // Keep the main thread alive
    loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}

// Convert a MIDI note number to frequency in Hz.
// A4 = MIDI 69 = 440 Hz; each semitone multiplies by 2^(1/12).
pub fn midi_to_freq(note: u8) -> f32 {
    440.0 * 2.0f32.powf((note as f32 - 69.0) / 12.0)
}
