use crate::NoteEvent;
use crate::midi_to_freq;
use crossbeam_queue::ArrayQueue;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

// ────────────────────────────────────────
//  THE REFERENCE BOOK OF MUSICAL CONSTANTS
// ────────────────────────────────────────
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

// ─────────────────
//  HELPER FUNCTIONS
// ─────────────────

// send one note to the queue.
fn play_note(queue: &ArrayQueue<NoteEvent>, note: u8, velocity: f32, duration_ms: f32) {
    let event = NoteEvent { note, velocity, duration: duration_ms };
    if queue.push(event).is_err() {
        eprintln!("composer: queue full, dropped note {}", note);
    } else {
        println!("▶ note {:>3}  {:.1} Hz  vel={:.2}  dur={}ms",
            note, midi_to_freq(note), velocity, duration_ms as u32);
    }
}

// send a chord
fn play_chord(queue: &ArrayQueue<NoteEvent>, chord: &[u8], velocity: f32, duration_ms: f32) {
    println!("≡ chord {:?}", chord);
    for &note in chord {
        play_note(queue, note, velocity, duration_ms);
    }
}

// send an arpeggio (the same notes but with a delay 'step_ms'  between them)
fn play_arpeggio(
    queue: &ArrayQueue<NoteEvent>,
    notes: &[u8],
    velocity: f32,
    duration_ms: f32,
    step_ms: u64,
) {
    println!("~ arpeggio {:?} step={}ms", notes, step_ms);
    for &note in notes {
        play_note(queue, note, velocity, duration_ms);
        thread::sleep(Duration::from_millis(step_ms));
    }
}

// transpose a set of notes into semitones of semitones.
// useful for changing the key without rewriting patterns.
fn transpose(notes: &[u8], semitones: i8) -> Vec<u8> {
    notes.iter()
        .map(|&n| (n as i16 + semitones as i16).clamp(0, 127) as u8)
        .collect()
}

// ───────────────────────────
//  THE MAIN COMPOSER FUNCTION 
// ───────────────────────────
pub fn start_composing(queue: Arc<ArrayQueue<NoteEvent>>) {
    use notes::*;

    // a short pause so that the audio engine can start up
    thread::sleep(Duration::from_millis(300));

    let chords: &[&[u8]] = &[
        &[C4, E4, G4],
        &[A3, C4, E4],
        &[F3, A3, C4],
        &[G3, B3, D4],
    ];
    let mut idx = 0usize;
    loop {
        let chord = chords[idx % chords.len()];
        play_chord(&queue, chord, 0.45, 900.0);
        idx += 1;
        thread::sleep(Duration::from_millis(1000));
    }
}
