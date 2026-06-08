mod audio_engine;
mod composer;
mod instruments;
mod markov;

use crossbeam_queue::ArrayQueue;
use instruments::{EnvelopeConfig, Instrument};
use std::sync::Arc;

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

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.get(1).map(|s| s.as_str()) == Some("list") {
        println!("Available presets:");
        for name in markov::PRESET_NAMES {
            println!("  {}", name);
        }
        return;
    }

    let preset_name = args.get(1).map(|s| s.as_str()).unwrap_or("Ambient");
    let preset = markov::get_preset(preset_name);

    println!("[main] Continuum - preset: {}", preset.name);
    println!("[main] Layers: {}", preset.layers.len());
    println!("[main] Chords: {}", preset.chords.len());
    println!(
        "[main] Base step: {:.0}ms (Continuum LFO modulated)",
        preset.base_step_ms
    );
    println!();

    let queue = Arc::new(ArrayQueue::<NoteEvent>::new(512));
    let audio_queue = queue.clone();
    let composer_queue = queue.clone();

    let preset_name_owned = preset_name.to_string();

    let _audio_engine = match audio_engine::start_engine(audio_queue) {
        Ok(engine) => engine,
        Err(err) => {
            eprintln!("[audio] {}", err);
            return;
        }
    };

    composer::start_composing(composer_queue, &preset_name_owned);
}
