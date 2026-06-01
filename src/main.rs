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
    // Usage: cargo run -- [preset] [flags]
    //   cargo run                 → Ambient (default)
    //   cargo run -- jazz         → Jazz
    //   cargo run -- list         → print all preset names and exit
    //
    // Case-insensitive: "JAZZ", "Jazz", "jazz" - same.

    let args: Vec<String> = std::env::args().collect();

    // "list" prints available presets and exits
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
    println!("[main] Scale: {} tones in range {}-{}",
        preset.scale.tones_in_range(preset.note_min, preset.note_max).len(),
        preset.note_min,
        preset.note_max,
    );
    println!("[main] Chords: {}", preset.chords.len());
    println!("[main] Tempo grid: {:.0}ms ({:.1} BPM equivalent)",
        preset.grid_step_ms,
        60000.0 / preset.grid_step_ms / 2.0, // rough BPM at eighth-note grid
    );
    println!();

    let queue          = Arc::new(ArrayQueue::<NoteEvent>::new(512));
    let audio_queue    = queue.clone();
    let composer_queue = queue.clone();

    // Pass preset name to composer so it uses the CLI-chosen preset
    let preset_name_owned = preset_name.to_string();

    std::thread::spawn(move || audio_engine::start_engine(audio_queue));
    std::thread::spawn(move || composer::start_composing(composer_queue, &preset_name_owned));

    loop { std::thread::sleep(std::time::Duration::from_secs(1)); }
}

pub fn midi_to_freq(note: u8) -> f32 {
    440.0 * 2.0f32.powf((note as f32 - 69.0) / 12.0)
}
