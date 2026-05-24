use crate::midi_to_freq;
use crate::note_buffer::NoteBuffer;
use crate::NoteEvent;
use crossbeam_queue::ArrayQueue;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

// ─────────────────────────────────────────────────────────────
//  MIDI NOTE CONSTANTS
// ─────────────────────────────────────────────────────────────

#[allow(dead_code)]
pub mod notes {
    // Octave 3
    pub const C3:  u8 = 48; pub const CS3: u8 = 49; pub const D3:  u8 = 50;
    pub const DS3: u8 = 51; pub const E3:  u8 = 52; pub const F3:  u8 = 53;
    pub const FS3: u8 = 54; pub const G3:  u8 = 55; pub const GS3: u8 = 56;
    pub const A3:  u8 = 57; pub const AS3: u8 = 58; pub const B3:  u8 = 59;
    // Octave 4 (middle)
    pub const C4:  u8 = 60; pub const CS4: u8 = 61; pub const D4:  u8 = 62;
    pub const DS4: u8 = 63; pub const E4:  u8 = 64; pub const F4:  u8 = 65;
    pub const FS4: u8 = 66; pub const G4:  u8 = 67; pub const GS4: u8 = 68;
    pub const A4:  u8 = 69; pub const AS4: u8 = 70; pub const B4:  u8 = 71;
    // Octave 5
    pub const C5:  u8 = 72; pub const CS5: u8 = 73; pub const D5:  u8 = 74;
    pub const DS5: u8 = 75; pub const E5:  u8 = 76; pub const F5:  u8 = 77;
    pub const FS5: u8 = 78; pub const G5:  u8 = 79; pub const GS5: u8 = 80;
    pub const A5:  u8 = 81; pub const AS5: u8 = 82; pub const B5:  u8 = 83;
}

// ─────────────────────────────────────────────────────────────
//  MAIN LOOP
// ─────────────────────────────────────────────────────────────

// Reads notes from the LLM buffer one at a time and forwards them to
// the audio engine queue, sleeping for the note's duration between each.
// This sleep is what turns durations into actual musical timing.
pub fn start_composing(
    buffer: Arc<NoteBuffer>,
    audio_queue: Arc<ArrayQueue<NoteEvent>>,
) {
    // Brief delay to let the audio engine initialize before the first note.
    thread::sleep(Duration::from_millis(300));
    println!("[composer] ready — waiting for notes from LLM");

    loop {
        // Block here until the LLM places a note in the buffer.
        let event = buffer.pop();

        println!(
            "[composer] note {:>3}  {:.1} Hz  vel={:.2}  dur={}ms",
            event.note,
            midi_to_freq(event.note),
            event.velocity,
            event.duration as u32,
        );

        let duration_ms = event.duration as u64;

        if audio_queue.push(event).is_err() {
            eprintln!("[composer] audio queue full — note dropped");
        }

        // Sleep for the note's duration; this drives the musical tempo.
        thread::sleep(Duration::from_millis(duration_ms));
    }
}

// ─────────────────────────────────────────────────────────────
//  TESTING HELPERS (manual playback without the LLM)
// ─────────────────────────────────────────────────────────────

#[allow(dead_code)]
pub fn play_note(queue: &ArrayQueue<NoteEvent>, note: u8, velocity: f32, duration_ms: f32) {
    let event = NoteEvent { note, velocity, duration: duration_ms };
    if queue.push(event).is_err() {
        eprintln!("[composer] queue full — note {} dropped", note);
    }
}

#[allow(dead_code)]
pub fn play_chord(queue: &ArrayQueue<NoteEvent>, chord: &[u8], velocity: f32, duration_ms: f32) {
    for &note in chord {
        play_note(queue, note, velocity, duration_ms);
    }
}

#[allow(dead_code)]
pub fn play_arpeggio(
    queue: &ArrayQueue<NoteEvent>,
    notes: &[u8],
    velocity: f32,
    duration_ms: f32,
    step_ms: u64,
) {
    for &note in notes {
        play_note(queue, note, velocity, duration_ms);
        thread::sleep(Duration::from_millis(step_ms));
    }
}

// Shift every note in a slice by the given number of semitones.
// Useful for transposing a pattern to a different key without rewriting it.
#[allow(dead_code)]
pub fn transpose(notes: &[u8], semitones: i8) -> Vec<u8> {
    notes
        .iter()
        .map(|&n| (n as i16 + semitones as i16).clamp(0, 127) as u8)
        .collect()
}
