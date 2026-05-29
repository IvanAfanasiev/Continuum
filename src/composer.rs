use crate::markov::{get_preset, MarkovGenerator};
use crate::midi_to_freq;
use crate::NoteEvent;
use crossbeam_queue::ArrayQueue;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

// presets: "Ambient" "Jazz" "Minimal" "Classical" "Drone" "Chaos"
const PRESET_NAME: &str = "Jazz";

pub fn start_composing(queue: Arc<ArrayQueue<NoteEvent>>) {
    thread::sleep(Duration::from_millis(300));

    let preset  = get_preset(PRESET_NAME);
    let mut gen = MarkovGenerator::new(preset);

    println!("[composer] ready | preset: {}", preset.name);
    println!("[composer] grid step: {:.0}ms", preset.grid_step_ms);

    loop {
        if let Some(rest_ms) = gen.phrase_rest_ms() {
            println!("[composer] rest {:.0}ms", rest_ms);
            thread::sleep(Duration::from_millis(rest_ms as u64));
        }

        let event = gen.next();

        println!(
            "[composer] note {:>3}  {:.1} Hz  vel={:.2}  dur={:.0}ms",
            event.note,
            midi_to_freq(event.note),
            event.velocity,
            event.duration,
        );

        if queue.push(event).is_err() {
            eprintln!("[composer] queue full - note dropped");
        }

        // Sleep for the GRID STEP
        // This means notes overlap, next note starts before current one ends
        // That is what creates a legato, connected melody instead of
        // a sequence of isolated sounds separated by silence
        let step = gen.grid_step_ms();
        thread::sleep(Duration::from_millis(step as u64));
    }
}

#[allow(dead_code)]
pub fn play_note(queue: &ArrayQueue<NoteEvent>, note: u8, velocity: f32, duration_ms: f32) {
    if queue.push(NoteEvent { note, velocity, duration: duration_ms }).is_err() {
        eprintln!("[composer] queue full - note {} dropped", note);
    }
}
