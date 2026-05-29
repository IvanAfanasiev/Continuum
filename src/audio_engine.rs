use crate::instruments::{Instrument, InstrumentState};
use crate::midi_to_freq;
use crate::NoteEvent;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_queue::ArrayQueue;
use std::sync::Arc;

const MAX_VOICES: usize = 16;

#[derive(PartialEq, Clone, Copy)]
enum EnvState { Idle, Attack, Sustain, Release }

struct Voice {
    freq:              f32,
    target_amplitude:  f32,
    current_vol:       f32,
    state:             EnvState,
    samples_remaining: u32,
    attack_samples:    f32,
    release_samples:   f32,
    instrument:        Instrument,
    istate:            InstrumentState,
}

impl Voice {
    fn new() -> Self {
        Self {
            freq:              440.0,
            target_amplitude:  0.0,
            current_vol:       0.0,
            state:             EnvState::Idle,
            samples_remaining: 0,
            attack_samples:    0.0,
            release_samples:   0.0,
            instrument:        Instrument::Sine,
            istate:            InstrumentState::new(),
        }
    }

    fn trigger(&mut self, event: &NoteEvent, sample_rate: f32) {
        self.freq             = midi_to_freq(event.note);
        self.target_amplitude = event.velocity;
        self.instrument       = event.instrument;
        self.samples_remaining = (event.duration / 1000.0 * sample_rate) as u32;

        let (att, rel) = match event.instrument {
            Instrument::Pad => (0.600, 1.500),
            Instrument::Piano => (0.005, 0.400),
            Instrument::Pluck => (0.001, 0.120),
            Instrument::Bass => (0.015, 0.200),
            Instrument::Organ => (0.020, 0.080),
            Instrument::Sine => (0.025, 0.280),
            Instrument::Kick | Instrument::Hihat | Instrument::Snare => (0.002, 0.050),
        };

        self.attack_samples  = att * sample_rate;
        self.release_samples = rel * sample_rate;
        self.current_vol = 0.0;
        self.state = EnvState::Attack;

        // Reset instrument state for new note (phase kept for legato on pitched instruments)
        self.istate.reset(self.freq, sample_rate, self.instrument);
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

                match self.instrument {
                    Instrument::Piano => {
                        self.current_vol *= 0.99994;
                    }
                    Instrument::Pluck => {
                        self.current_vol *= 0.99985;
                    }
                    Instrument::Sine => {
                        self.current_vol *= 0.99998;
                    }
                    Instrument::Pad => {
                        self.current_vol *= 0.99998;
                    }
                    Instrument::Bass => {
                        self.current_vol *= 0.99998;
                    }
                    _ => {}
                }
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

        self.istate.next_sample(
            self.instrument,
            self.freq,
            self.current_vol,
            sample_rate,
        )
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
                while let Some(event) = queue.pop() {
                    if let Some(v) = voices.iter_mut()
                        .find(|v| v.state == EnvState::Idle)
                    {
                        v.trigger(&event, sample_rate);
                    } else if let Some(v) = voices.iter_mut()
                        .min_by_key(|v| v.samples_remaining)
                    {
                        v.trigger(&event, sample_rate);
                    }
                }
                for sample in data.iter_mut() {
                    let out: f32 = voices.iter_mut()
                        .map(|v| v.next_sample(sample_rate))
                        .sum();

                    // Normalise by voice count; *2.0 compensates for
                    // the fact that usually only a few voices are active.
                    let mixed = out / MAX_VOICES as f32 * 2.0;
                    *sample = mixed / (1.0 + mixed.abs());
                }
            },
            |err| eprintln!("[audio] stream error: {}", err),
            None,
        )
        .expect("failed to build output stream");

    stream.play().expect("failed to start audio stream");
    loop { std::thread::sleep(std::time::Duration::from_millis(100)); }
}
