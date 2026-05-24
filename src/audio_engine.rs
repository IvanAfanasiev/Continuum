use crate::midi_to_freq;
use crate::NoteEvent;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_queue::ArrayQueue;
use std::sync::Arc;

// maximum simultaneous voices
// 8 is enough for chords and arpeggios
const MAX_VOICES: usize = 8;

// one synthesizer voice is an independent sine wave
struct Voice {
    phase: f32,
    freq: f32,
    target_freq: f32,
    amplitude: f32,
    target_amplitude: f32,
    samples_remaining: u32,
    // false = the voice is free and can be occupied by a new note
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

    // assign a new note to the voice
    fn trigger(&mut self, event: &NoteEvent, sample_rate: f32) {
        self.target_freq = midi_to_freq(event.note);
        self.target_amplitude = event.velocity;
        self.samples_remaining = (event.duration / 1000.0 * sample_rate) as u32;
        self.active = true;
    }

    // compute the next sample and update the internal state
    fn next_sample(&mut self, sample_rate: f32, freq_smooth: f32, amp_smooth: f32) -> f32 {
        if !self.active {
            return 0.0;
        }

        // count the remaining time
        // when it expires, start fade-out
        if self.samples_remaining > 0 {
            self.samples_remaining -= 1;
        } else {
            self.target_amplitude = 0.0;
        }

        // exponential smoothing (legato and soft attacks)
        self.freq += (self.target_freq - self.freq) * freq_smooth;
        self.amplitude += (self.target_amplitude - self.amplitude) * amp_smooth;

        // deactivate the voice when the amplitude is almost zero
        // free the slot
        if self.amplitude < 0.0001 && self.target_amplitude == 0.0 {
            self.active = false;
            return 0.0;
        }

        // sinusoidal oscillator
        self.phase = (self.phase + self.freq / sample_rate) % 1.0;
        (self.phase * 2.0 * std::f32::consts::PI).sin() * self.amplitude
    }
}

pub fn start_engine(queue: Arc<ArrayQueue<NoteEvent>>) {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .expect("Output device not found");
    let config = device.default_output_config().unwrap().config();
    let sample_rate = config.sample_rate.0 as f32;

    // exponential smoothing coefficients normalized by sample rate.
    // freq_smooth  ~20ms - smooth sliding between notes (portamento effect)
    // amp_smooth   ~8ms  - soft attack/release without clicks
    let freq_smooth = 1.0 - (-1.0f32 / (0.020 * sample_rate)).exp();
    let amp_smooth  = 1.0 - (-1.0f32 / (0.008 * sample_rate)).exp();

    let mut voices: [Voice; MAX_VOICES] = std::array::from_fn(|_| Voice::new());

    let stream = device
        .build_output_stream(
            &config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                for sample in data.iter_mut() {
                    // read all available events from the queue at once
                    // this allows chords to start in one buffer
                    while let Some(event) = queue.pop() {
                        // look for a free voice to play the note
                        if let Some(voice) = voices.iter_mut().find(|v| !v.active) {
                            voice.trigger(&event, sample_rate);
                        } else {
                            // all the voices are occupied - replace the one with
                            // the least amount of time left (least noticeable)
                            if let Some(voice) = voices
                                .iter_mut()
                                .min_by_key(|v| v.samples_remaining)
                            {
                                voice.trigger(&event, sample_rate);
                            }
                        }
                    }

                    // summarize all the active voices
                    let out: f32 = voices
                        .iter_mut()
                        .map(|v| v.next_sample(sample_rate, freq_smooth, amp_smooth))
                        .sum();

                    // normalization: divide by MAX_VOICES to prevent clipping
                    // when multiple voices are playing simultaneously
                    *sample = out / MAX_VOICES as f32;
                }
            },
            |err| eprintln!("audio error: {}", err),
            None,
        )
        .expect("output stream error");

    stream.play().expect("stream play error");

    loop {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}
