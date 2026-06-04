// Multi-layer composer.
//
// Each layer runs in its own thread with its own MarkovGenerator.
// All layers share the same preset (scale, chords, harmony) but have
// independent note ranges, instruments, and rhythm roles.
//
// RhythmRole determines the generator's behaviour:
//   Melody     - full Markov with motif memory (standard behaviour)
//   Bass       - only plays root tones of the current chord
//   Pad        - holds one chord tone for a long duration
//   Percussion - plays a fixed note on a rhythmic beat pattern

use crate::instruments::Instrument;
use crate::markov::{get_preset, LayerConfig, MarkovGenerator, MarkovPreset, RhythmRole};
use crate::midi_to_freq;
use crate::NoteEvent;
use crossbeam_queue::ArrayQueue;
use rand::prelude::*;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

pub fn start_composing(queue: Arc<ArrayQueue<NoteEvent>>, preset_name: &str) {
    thread::sleep(Duration::from_millis(300));

    let preset = get_preset(preset_name);
    println!("[composer] preset: {} | {} layer(s)", preset.name, preset.layers.len());

    // Spawn one thread per layer; all share the audio queue
    let mut handles = Vec::new();
    for (i, layer) in preset.layers.iter().enumerate() {
        let q    = queue.clone();
        // We pass preset as &'static so threads can share it safely
        let handle = thread::spawn(move || {
            run_layer(i, layer, preset, q);
        });
        handles.push(handle);
    }

    for h in handles { h.join().unwrap_or(()); }
}

// ─────────────────────────────────────────────────────────────
//  LAYER RUNNER
// ─────────────────────────────────────────────────────────────

fn run_layer(
    idx:    usize,
    layer:  &'static LayerConfig,
    preset: &'static MarkovPreset,
    queue:  Arc<ArrayQueue<NoteEvent>>,
) {
    let grid_ms = preset.grid_step_ms * layer.grid_mult;

    match layer.role {
        RhythmRole::Melody     => run_melody(layer, preset, grid_ms, queue),
        RhythmRole::Bass       => run_bass(layer, preset, grid_ms, queue),
        RhythmRole::Pad        => run_pad(layer, preset, grid_ms, queue),
        RhythmRole::Percussion => run_percussion(layer, preset, grid_ms, queue),
        RhythmRole::Piano      => run_piano(layer, preset, queue),
    }
}

// ── MELODY ────────────────────────────────────────────────────
// Full Markov generator - same logic as the original single-layer composer.

fn run_melody(
    layer:  &'static LayerConfig,
    preset: &'static MarkovPreset,
    grid_ms: f32,
    queue:   Arc<ArrayQueue<NoteEvent>>,
) {
    // Build a local preset copy with the layer's note range
    // We reuse the global preset but override note_min/max via the generator
    let mut gen = MarkovGenerator::new_with_range(preset, layer.note_min, layer.note_max);

    println!("[melody] started | {}", instrument_name(layer.instrument));

    loop {
        if let Some(rest_ms) = gen.phrase_rest_ms() {
            thread::sleep(Duration::from_millis(rest_ms as u64));
        }

        let event = gen.next();
        let base_sleep = event.duration; // Read duration before moving ownership to push_note

        let mut event = event;
        event.velocity   = (event.velocity * layer.vel_scale).clamp(0.0, 1.0);
        event.instrument = layer.instrument;
        event.envelope   = resolve_envelope(layer);

        push_note(&queue, event, instrument_name(layer.instrument));
        thread::sleep(Duration::from_millis(base_sleep as u64));
    }
}

// ── BASS ──────────────────────────────────────────────────────
// Plays the root note of the current chord in the layer's range.
// Advances chord index in sync with the melody (same preset chords).

fn run_bass(
    layer:   &'static LayerConfig,
    preset:  &'static MarkovPreset,
    grid_ms: f32,
    queue:   Arc<ArrayQueue<NoteEvent>>,
) {
    let mut rng       = rand::rng();
    let mut chord_idx = 0usize;
    let mut step      = 0usize;
    // Bass phrase length mirrors the melody's range
    let phrase_len    = (preset.phrase_min + preset.phrase_max) / 2;

    println!("[bass] started | {}", instrument_name(layer.instrument));

    loop {
        let chord    = preset.chords[chord_idx];
        let root_pc  = chord.root % 12;

        // Find the root note closest to the middle of the layer's range
        let mid   = (layer.note_min + layer.note_max) / 2;
        let oct   = (mid / 12) * 12;
        let note  = (oct + root_pc).clamp(layer.note_min, layer.note_max);

        // Vary duration: long on beat 1, shorter on others
        let duration = if step % phrase_len == 0 {
            grid_ms * 3.5
        } else {
            grid_ms * 1.8
        };

        let velocity = rng.random_range(
            preset.vel_min * layer.vel_scale..preset.vel_max * layer.vel_scale
        );

        let is_phrase_start = step % phrase_len == 0;
        let is_phrase_end   = (step + 1) % phrase_len == 0;

        push_note(&queue, NoteEvent {
            note,
            velocity,
            duration,
            instrument: layer.instrument,
            envelope:   resolve_envelope(layer),
            is_phrase_start,
            is_phrase_end,
        }, instrument_name(layer.instrument));

        step += 1;
        if step % phrase_len == 0 {
            chord_idx = (chord_idx + 1) % preset.chords.len();
        }

        thread::sleep(Duration::from_millis(grid_ms as u64));
    }
}

// ── PAD ───────────────────────────────────────────────────────
// Holds one chord tone for a long time (grid_mult is large, e.g. 4-8x).
// Changes to a new chord tone when the grid step fires.

fn run_pad(
    layer:   &'static LayerConfig,
    preset:  &'static MarkovPreset,
    grid_ms: f32,
    queue:   Arc<ArrayQueue<NoteEvent>>,
) {
    let mut rng       = rand::rng();
    let mut chord_idx = 0usize;
    let mut step      = 0usize;
    let phrase_len    = (preset.phrase_min + preset.phrase_max) / 2;

    println!("[pad] started | {}", instrument_name(layer.instrument));

    loop {
        let chord   = preset.chords[chord_idx];
        // Pick a chord tone within the layer's range
        let tones: Vec<u8> = preset
            .scale
            .tones_in_range(layer.note_min, layer.note_max)
            .into_iter()
            .filter(|&n| chord.contains(n))
            .collect();

        if !tones.is_empty() {
            let note     = tones[rng.random_range(0..tones.len())];
            let velocity = rng.random_range(
                preset.vel_min * layer.vel_scale..preset.vel_max * layer.vel_scale
            );
            // Duration slightly longer than grid step for smooth overlap
            let duration = grid_ms * 1.2;

            let is_phrase_start = step % phrase_len == 0;
            let is_phrase_end   = (step + 1) % phrase_len == 0;

            push_note(&queue, NoteEvent {
                note, 
                velocity, 
                duration,
                instrument: layer.instrument,
                envelope:   resolve_envelope(layer),
                is_phrase_start,
                is_phrase_end,
                
            }, instrument_name(layer.instrument));
        }

        step += 1;
        if step % phrase_len == 0 {
            chord_idx = (chord_idx + 1) % preset.chords.len();
        }

        thread::sleep(Duration::from_millis(grid_ms as u64));
    }
}

// ── PERCUSSION ────────────────────────────────────────────────
// Plays a fixed note on a rhythmic beat pattern.

fn run_percussion(
    layer:   &'static LayerConfig,
    _preset: &'static MarkovPreset,
    grid_ms: f32,
    queue:   Arc<ArrayQueue<NoteEvent>>,
) {
    let mut rng = rand::rng();
    let mut pat_pos = 0usize;

    println!("[perc] started | {}", instrument_name(layer.instrument));

    loop {
        if layer.beat_pattern.is_empty() {
            thread::sleep(Duration::from_millis(grid_ms as u64));
            continue;
        }

        let idx = pat_pos % layer.beat_pattern.len();
        if layer.beat_pattern[idx] {
            let vel = rng.random_range(
                0.7 * layer.vel_scale..1.0 * layer.vel_scale
            ).clamp(0.0, 1.0);

            let is_phrase_start = idx == 0;
            let is_phrase_end   = idx == layer.beat_pattern.len() - 1;

            push_note(&queue, NoteEvent {
                note:       layer.fixed_note,
                velocity:   vel,
                duration:   grid_ms * 0.55,
                instrument: layer.instrument,
                envelope:   resolve_envelope(layer),
                is_phrase_start,
                is_phrase_end,
            }, instrument_name(layer.instrument));
        }

        pat_pos += 1;
        thread::sleep(Duration::from_millis(grid_ms as u64));
    }
}

// ── PIANO ──────────────────────────────────────

fn run_piano(
    layer:   &'static LayerConfig,
    preset:  &'static MarkovPreset,
    queue:   Arc<ArrayQueue<NoteEvent>>,
) {
    let mut rng = rand::rng();
    let mut gen = MarkovGenerator::new_with_range(preset, layer.note_min, layer.note_max);
    
    println!("[piano] started | Calm Melodic Phrasing Mode");

    loop {
        if let Some(rest_ms) = gen.phrase_rest_ms() {
            thread::sleep(Duration::from_millis(rest_ms as u64));
        }

        let base_event = gen.next();
        let chord = gen.current_chord();
        
        let is_start = base_event.is_phrase_start;
        let is_end   = base_event.is_phrase_end;
        let is_structural_anchor = is_start || is_end;
        
        let mut notes_to_play = Vec::new();

        if is_structural_anchor {
            let mut tones = preset.scale.tones_in_range(layer.note_min, layer.note_max);
            tones.retain(|&n| chord.contains(n));
            tones.shuffle(&mut rng);
            
            let extra_notes = if is_start { 1 } else { 2 };
            notes_to_play = tones.into_iter().take(extra_notes).collect();
            
            if !notes_to_play.contains(&base_event.note) {
                notes_to_play.push(base_event.note);
            }
            notes_to_play.sort_unstable();
        } else {
            notes_to_play.push(base_event.note);
        }

        let total_notes = notes_to_play.len();
        
        let strum_delay = if is_structural_anchor { rng.random_range(40..60) } else { 0 };

        let mut actual_dur = base_event.duration;
        if is_end {
            actual_dur *= 2.0;
        } else if is_start {
            actual_dur *= 1.2; 
        }

        for (i, &note) in notes_to_play.iter().enumerate() {
            let vel_mod = if is_structural_anchor {
                if i == total_notes - 1 { 1.0 } else { 0.45 } 
            } else {
                rng.random_range(0.90..1.05)
            };
            
            let velocity = (base_event.velocity * layer.vel_scale * vel_mod).clamp(0.0, 1.0);
            let sustain_overlap = if !is_structural_anchor { 1.10 } else { 1.0 };

            let ev = NoteEvent {
                note,
                velocity,
                duration: actual_dur * sustain_overlap,
                instrument: layer.instrument,
                envelope: resolve_envelope(layer),
                is_phrase_start: base_event.is_phrase_start,
                is_phrase_end: base_event.is_phrase_end,
            };

            push_note(&queue, ev, "piano");

            if strum_delay > 0 {
                thread::sleep(Duration::from_millis(strum_delay));
            }
        }

        let time_spent_strumming = (total_notes as u64).saturating_sub(1) * strum_delay;
        let remaining_sleep = (actual_dur as u64).saturating_sub(time_spent_strumming);
        
        let humanize = if is_structural_anchor { rng.random_range(-15i64..30i64) } else { 0 };
        let final_sleep = (remaining_sleep as i64 + humanize).max(20) as u64;

        thread::sleep(Duration::from_millis(final_sleep));
    }
}

// ─────────────────────────────────────────────────────────────
//  HELPERS
// ─────────────────────────────────────────────────────────────

fn push_note(queue: &ArrayQueue<NoteEvent>, event: NoteEvent, label: &str) {
    println!(
        "[{}] note {:>3}  {:.1} Hz  vel={:.2}  dur={:.0}ms",
        label,
        event.note,
        midi_to_freq(event.note),
        event.velocity,
        event.duration,
    );
    if queue.push(event).is_err() {
        eprintln!("[composer] queue full - note dropped");
    }
}

// Resolve the envelope: use LayerConfig override if set, else instrument default.
fn resolve_envelope(layer: &LayerConfig) -> crate::instruments::EnvelopeConfig {
    layer.envelope.unwrap_or_else(|| layer.instrument.default_envelope())
}

fn instrument_name(i: Instrument) -> &'static str {
    match i {
        Instrument::Sine  => "sine",
        Instrument::Piano => "piano",
        Instrument::Pluck => "pluck",
        Instrument::Pad   => "pad",
        Instrument::Bass  => "bass",
        Instrument::Organ => "organ",
        Instrument::Kick  => "kick",
        Instrument::Hihat => "hihat",
        Instrument::Snare => "snare",
    }
}