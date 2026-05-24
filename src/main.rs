mod audio_engine;
mod composer;
mod llm_bridge;
mod note_buffer;
mod presets;

use crossbeam_queue::ArrayQueue;
use std::sync::Arc;

pub struct NoteEvent {
    pub note: u8,      // MIDI note number (0-127)
    pub velocity: f32, // amplitude (0.0-1.0)
    pub duration: f32, // duration in milliseconds
}

fn main() {
    // Change to any of: presets::JAZZ, MINIMAL, CHAOS, CLASSICAL, DRONE
    let preset = presets::AMBIENT;
    println!("[main] Starting Continuum | preset: {}", preset.name);

    // Shared buffer between LLM and composer (refill threshold defined per preset)
    let note_buffer = note_buffer::NoteBuffer::new(preset.refill_threshold);
    // Lock-free queue between composer and audio engine
    let audio_queue = Arc::new(ArrayQueue::new(256));

    // Clone all handles before the first move closure —
    // once a value is moved into a thread, it can no longer be cloned.
    let aq_audio     = audio_queue.clone();
    let aq_composer  = audio_queue.clone();
    let buf_composer = note_buffer.clone();
    let buf_llm      = note_buffer.clone();

    // Thread 1: audio engine — reads NoteEvents and synthesizes sound
    std::thread::spawn(move || {
        audio_engine::start_engine(aq_audio);
    });

    // Thread 2: composer — drains the note buffer and forwards to audio queue
    std::thread::spawn(move || {
        composer::start_composing(buf_composer, aq_composer);
    });

    // Thread 3: LLM — generates notes and fills the note buffer
    std::thread::spawn(move || {
        if let Err(e) = llm_bridge::run_llm(buf_llm, &preset) {
            eprintln!("[llm] Fatal error: {}", e);
            std::process::exit(1);
        }
    });

    // Keep the main thread alive
    loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}

// Convert a MIDI note number to its frequency in Hz.
// A4 = MIDI 69 = 440 Hz; each semitone multiplies by 2^(1/12).
pub fn midi_to_freq(note: u8) -> f32 {
    440.0 * 2.0f32.powf((note as f32 - 69.0) / 12.0)
}
