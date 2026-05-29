use crate::midi_to_freq;
use crate::NoteEvent;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_queue::ArrayQueue;
use std::sync::Arc;

const MAX_VOICES: usize = 8;

// Simple ADSR-like envelope states
#[derive(PartialEq, Clone, Copy)]
enum EnvState { Idle, Attack, Sustain, Release }

struct Voice {
    phase:             f32,
    freq:              f32,
    target_amplitude:  f32,
    current_vol:       f32,
    state:             EnvState,
    samples_remaining: u32,
    attack_samples:    f32,
    release_samples:   f32,
}

impl Voice {
    fn new() -> Self {
        Self {
            phase:             0.0,
            freq:              440.0,
            target_amplitude:  0.0,
            current_vol:       0.0,
            state:             EnvState::Idle,
            samples_remaining: 0,
            attack_samples:    0.0,
            release_samples:   0.0,
        }
    }

    fn trigger(&mut self, event: &NoteEvent, sample_rate: f32) {
        self.freq             = midi_to_freq(event.note);
        self.target_amplitude = event.velocity;
        self.samples_remaining = (event.duration / 1000.0 * sample_rate) as u32;

        // Fixed attack ~30ms, release ~300ms
        self.attack_samples  = 0.030 * sample_rate;
        self.release_samples = 0.300 * sample_rate;
        self.state           = EnvState::Attack;
        // Do NOT reset phase, avoids click on new notes
    }

    fn next_sample(&mut self, sample_rate: f32) -> f32 {
        match self.state {
            EnvState::Idle => return 0.0,

            EnvState::Attack => {
                self.current_vol += self.target_amplitude / self.attack_samples;
                if self.current_vol >= self.target_amplitude {
                    self.current_vol = self.target_amplitude;
                    self.state = EnvState::Sustain;
                }
            }

            EnvState::Sustain => {
                if self.samples_remaining > 0 {
                    self.samples_remaining -= 1;
                } else {
                    self.state = EnvState::Release;
                }
            }

            EnvState::Release => {
                self.current_vol -= self.target_amplitude / self.release_samples;
                if self.current_vol <= 0.0 {
                    self.current_vol = 0.0;
                    self.state = EnvState::Idle;
                }
            }
        }

        self.phase = (self.phase + self.freq / sample_rate) % 1.0;
        (self.phase * 2.0 * std::f32::consts::PI).sin() * self.current_vol
    }
}

pub fn start_engine(queue: Arc<ArrayQueue<NoteEvent>>) {
    let host   = cpal::default_host();
    let device = host.default_output_device().expect("no output device found");
    let config = device.default_output_config().unwrap().config();
    let sample_rate = config.sample_rate.0 as f32;

    let mut voices: [Voice; MAX_VOICES] = std::array::from_fn(|_| Voice::new());

    let stream = device
        .build_output_stream(
            &config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                for sample in data.iter_mut() {
                    while let Some(event) = queue.pop() {
                        if let Some(v) = voices.iter_mut().find(|v| v.state == EnvState::Idle) {
                            v.trigger(&event, sample_rate);
                        } else if let Some(v) = voices.iter_mut()
                            .min_by_key(|v| v.samples_remaining)
                        {
                            v.trigger(&event, sample_rate);
                        }
                    }

                    let out: f32 = voices.iter_mut()
                        .map(|v| v.next_sample(sample_rate))
                        .sum();

                    *sample = (out / MAX_VOICES as f32 * 2.5).clamp(-1.0, 1.0);
                }
            },
            |err| eprintln!("[audio] stream error: {}", err),
            None,
        )
        .expect("failed to build output stream");

    stream.play().expect("failed to start audio stream");

    loop { std::thread::sleep(std::time::Duration::from_millis(100)); }
}
