use crate::instruments::{EnvelopeConfig, Instrument, InstrumentState};
use crate::midi_to_freq;
use crate::NoteEvent;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_queue::ArrayQueue;
use std::sync::Arc;

const MAX_VOICES: usize = 16;

#[derive(PartialEq, Clone, Copy)]
enum EnvState { Idle, Attack, Decay, Sustain, Release }

struct Voice {
    freq:              f32,
    peak_amplitude:    f32,
    sustain_level:     f32,
    current_vol:       f32,
    state:             EnvState,
    samples_remaining: u32,
    attack_inc:        f32,
    decay_dec:         f32,
    release_dec:       f32,
    // Stored so we can recompute release_dec from actual current_vol
    // when entering Release (needed when sustain=0 and decay lands at 0).
    release_samples:   f32,
    instrument:        Instrument,
    istate:            InstrumentState,
}

impl Voice {
    fn new() -> Self {
        Self {
            freq:             440.0,
            peak_amplitude:   0.0,
            sustain_level:    0.0,
            current_vol:      0.0,
            state:            EnvState::Idle,
            samples_remaining: 0,
            attack_inc:       0.0,
            decay_dec:        0.0,
            release_dec:      0.0,
            release_samples:  1.0,
            instrument:       Instrument::Sine,
            istate:           InstrumentState::new(),
        }
    }

    fn trigger(&mut self, event: &NoteEvent, env: EnvelopeConfig, sample_rate: f32) {
        self.freq           = midi_to_freq(event.note);
        self.peak_amplitude = event.velocity;
        self.sustain_level  = event.velocity * env.sustain;
        self.instrument     = event.instrument;
        self.current_vol    = 0.0;
        self.samples_remaining = (event.duration / 1000.0 * sample_rate) as u32;

        let att = (env.attack_ms  / 1000.0 * sample_rate).max(1.0);
        let dec = (env.decay_ms   / 1000.0 * sample_rate).max(1.0);
        let rel = (env.release_ms / 1000.0 * sample_rate).max(1.0);

        self.attack_inc     = self.peak_amplitude / att;
        self.decay_dec      = (self.peak_amplitude - self.sustain_level) / dec;
        self.release_samples = rel;
        // release_dec is set properly when we actually enter Release,
        // because the starting level depends on where Decay/Sustain ended.
        self.release_dec    = self.peak_amplitude / rel;

        self.state = EnvState::Attack;
        self.istate.reset(self.freq, sample_rate, self.instrument);
    }

    fn enter_release(&mut self) {
        // Recompute release_dec from ACTUAL current volume so the
        // release always takes exactly release_ms regardless of
        // whether sustain was 0, 0.5, or 1.0.
        if self.current_vol > 0.0001 {
            self.release_dec = self.current_vol / self.release_samples;
        }
        self.state = EnvState::Release;
    }

    fn next_sample(&mut self, sample_rate: f32) -> f32 {
        if self.state == EnvState::Idle { return 0.0; }

        match self.state {
            EnvState::Idle => return 0.0,

            EnvState::Attack => {
                self.current_vol += self.attack_inc;
                if self.current_vol >= self.peak_amplitude {
                    self.current_vol = self.peak_amplitude;
                    // If decay is configured, go to Decay; else go directly to Sustain or Release
                    if self.decay_dec > 0.0001 {
                        self.state = EnvState::Decay;
                    } else if self.sustain_level > 0.0001 {
                        self.state = EnvState::Sustain;
                    } else {
                        self.enter_release();
                    }
                }
            }

            EnvState::Decay => {
                self.current_vol -= self.decay_dec;
                if self.current_vol <= self.sustain_level {
                    self.current_vol = self.sustain_level;
                    if self.sustain_level > 0.0001 {
                        self.state = EnvState::Sustain;
                    } else {
                        // sustain=0: Decay fell to 0, now release from here.
                        // current_vol is ~0 but we still run Release for
                        // correct duration (e.g. a slow Release with S=0
                        // is a "fade from decay endpoint" effect).
                        self.enter_release();
                    }
                }
            }

            EnvState::Sustain => {
                if self.samples_remaining > 0 {
                    self.samples_remaining -= 1;
                } else {
                    self.enter_release();
                }
            }

            EnvState::Release => {
                self.current_vol -= self.release_dec;
                if self.current_vol <= 0.0 {
                    self.current_vol = 0.0;
                    self.state = EnvState::Idle;
                }
            }
        }

        if self.current_vol <= 0.0 { return 0.0; }

        self.istate.next_sample(self.instrument, self.freq, self.current_vol, sample_rate)
    }
}

pub fn start_engine(queue: Arc<ArrayQueue<NoteEvent>>) {
    let host   = cpal::default_host();
    let device = host.default_output_device().expect("no output device found");
    let config = device.default_output_config().unwrap().config();
    let sample_rate  = config.sample_rate.0 as f32;
    let num_channels = config.channels as usize;

    let mut voices: [Voice; MAX_VOICES] = std::array::from_fn(|_| Voice::new());

    let stream = device
        .build_output_stream(
            &config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                while let Some(event) = queue.pop() {
                    let env = event.envelope;
                    if let Some(v) = voices.iter_mut().find(|v| v.state == EnvState::Idle) {
                        v.trigger(&event, env, sample_rate);
                    } else if let Some(v) = voices.iter_mut().min_by_key(|v| v.samples_remaining) {
                        v.trigger(&event, env, sample_rate);
                    }
                }

                for frame in data.chunks_mut(num_channels) {
                    let out: f32 = voices.iter_mut()
                        .map(|v| v.next_sample(sample_rate))
                        .sum();
                    let mixed     = out / MAX_VOICES as f32 * 2.0;
                    let saturated = mixed / (1.0 + mixed.abs());
                    for ch in frame.iter_mut() { *ch = saturated; }
                }
            },
            |err| eprintln!("[audio] stream error: {}", err),
            None,
        )
        .expect("failed to build output stream");

    stream.play().expect("failed to start audio stream");
    loop { std::thread::sleep(std::time::Duration::from_millis(100)); }
}
