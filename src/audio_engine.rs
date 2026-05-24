use crate::midi_to_freq;
use crate::NoteEvent;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_queue::ArrayQueue;
use std::sync::Arc;

// Maximum number of simultaneous voices (polyphony).
// 8 is sufficient for chords and arpeggios; raise to 16 for denser textures.
const MAX_VOICES: usize = 8;

// One synthesizer voice: an independent sine-wave oscillator.
struct Voice {
    phase: f32,
    freq: f32,
    target_freq: f32,
    amplitude: f32,
    target_amplitude: f32,
    // Samples remaining before the note's natural end and fade-out begins.
    samples_remaining: u32,
    // When false the voice is idle and can be claimed by a new note.
    active: bool,
}

impl Voice {
    fn new() -> Self {
        Self {
            phase: 0.0,
            freq: 440.0,
            target_freq: 440.0,
            amplitude: 0.0,
            target_amplitude: 0.0,
            samples_remaining: 0,
            active: false,
        }
    }

    // Assign a new note to this voice.
    // The phase is intentionally NOT reset to avoid a click on legato transitions.
    fn trigger(&mut self, event: &NoteEvent, sample_rate: f32) {
        self.target_freq = midi_to_freq(event.note);
        self.target_amplitude = event.velocity;
        self.samples_remaining = (event.duration / 1000.0 * sample_rate) as u32;
        self.active = true;
    }

    // Advance the oscillator by one sample and return the output value.
    fn next_sample(&mut self, sample_rate: f32, freq_smooth: f32, amp_smooth: f32) -> f32 {
        if !self.active {
            return 0.0;
        }

        // Count down the note duration; start fade-out when it reaches zero.
        if self.samples_remaining > 0 {
            self.samples_remaining -= 1;
        } else {
            self.target_amplitude = 0.0;
        }

        // Exponential smoothing — creates natural portamento and soft attacks.
        self.freq      += (self.target_freq      - self.freq)      * freq_smooth;
        self.amplitude += (self.target_amplitude - self.amplitude) * amp_smooth;

        // Release the voice slot once the fade-out is complete.
        if self.amplitude < 0.0001 && self.target_amplitude == 0.0 {
            self.active = false;
            return 0.0;
        }

        // Sine-wave oscillator.
        self.phase = (self.phase + self.freq / sample_rate) % 1.0;
        (self.phase * 2.0 * std::f32::consts::PI).sin() * self.amplitude
    }
}

pub fn start_engine(queue: Arc<ArrayQueue<NoteEvent>>) {
    let host   = cpal::default_host();
    let device = host.default_output_device().expect("no output device found");
    let config = device.default_output_config().unwrap().config();
    let sample_rate = config.sample_rate.0 as f32;

    // Smoothing coefficients derived from time constants, normalized by sample rate.
    // freq_smooth (~20 ms): smooth pitch slides between notes (portamento).
    // amp_smooth  (~8 ms):  soft attack and release to prevent clicks.
    let freq_smooth = 1.0 - (-1.0f32 / (0.020 * sample_rate)).exp();
    let amp_smooth  = 1.0 - (-1.0f32 / (0.008 * sample_rate)).exp();

    let mut voices: [Voice; MAX_VOICES] = std::array::from_fn(|_| Voice::new());

    let stream = device
        .build_output_stream(
            &config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                for sample in data.iter_mut() {
                    // Drain all pending events in one pass so chords start together.
                    while let Some(event) = queue.pop() {
                        if let Some(voice) = voices.iter_mut().find(|v| !v.active) {
                            // Claim a free voice.
                            voice.trigger(&event, sample_rate);
                        } else {
                            // All voices busy: steal the one closest to finishing.
                            if let Some(voice) = voices.iter_mut().min_by_key(|v| v.samples_remaining) {
                                voice.trigger(&event, sample_rate);
                            }
                        }
                    }

                    // Sum all active voices.
                    let out: f32 = voices
                        .iter_mut()
                        .map(|v| v.next_sample(sample_rate, freq_smooth, amp_smooth))
                        .sum();

                    // Normalize by MAX_VOICES to prevent clipping when all voices are active.
                    *sample = out / MAX_VOICES as f32;
                }
            },
            |err| eprintln!("[audio] stream error: {}", err),
            None,
        )
        .expect("failed to build output stream");

    stream.play().expect("failed to start audio stream");

    loop {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}
