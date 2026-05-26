use crate::markov::{get_preset, MarkovGenerator};
use crate::NoteEvent;
use crossbeam_queue::ArrayQueue;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

const PRESET_NAME: &str = "Ambient";

struct LayerConfig {
    name: &'static str,
    octave_shift: i8,
    duration_mult: f32,
    overlap: f32,
}

pub fn start_composing(queue: Arc<ArrayQueue<NoteEvent>>) {
    println!("[composer] Starting multi-layer composer...");

    let layers = vec![
        LayerConfig { name: "Bass",   octave_shift: -18, duration_mult: 1.0, overlap: 1.0 },
    ];

    for config in layers {
        let q = queue.clone();
        thread::spawn(move || {
            run_layer(config, q);
        });
    }

    loop {
        thread::sleep(Duration::from_secs(1));
    }
}

fn run_layer(config: LayerConfig, queue: Arc<ArrayQueue<NoteEvent>>) {
    let preset = get_preset(PRESET_NAME);
    let mut gen = MarkovGenerator::new(preset);
    
    let mut buffer = Vec::with_capacity(50);

    loop {
        if buffer.len() < 10 {
            for _ in 0..40 {
                buffer.push(gen.next());
            }
        }

        if let Some(mut event) = buffer.pop() {
            let shifted_note = (event.note as i16 + config.octave_shift as i16).clamp(0, 127) as u8;
            event.note = shifted_note;
            
            if config.octave_shift < -12 {
                event.velocity *= 0.6; 
            }

            event.duration *= config.duration_mult;

            let sleep_ms = (event.duration * config.overlap) as u64;

            if queue.push(event).is_err() {
                // Queue is full, wait
            }

            thread::sleep(Duration::from_millis(sleep_ms));
        }
    }
}