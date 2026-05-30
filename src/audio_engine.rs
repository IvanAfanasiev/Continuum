use crate::instruments::{EnvelopeConfig, Instrument, InstrumentState};
use crate::midi_to_freq;
use crate::NoteEvent;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_queue::ArrayQueue;
use std::sync::Arc;

const MAX_VOICES: usize = 16;

// ─────────────────────────────────────────────────────────────
//  ADSR STATE MACHINE
//
//  Attack  - volume rises from 0 to peak at `attack_inc` per sample
//  Decay   - volume falls from peak to `sustain_level` at `decay_dec`
//  Sustain - volume held at `sustain_level` until note duration ends
//  Release - volume falls from `sustain_level` to 0 at `release_dec`
//  Idle    - silent, voice slot is free
//
//  Percussion instruments (Kick, Hihat, Snare) manage their own
//  amplitude via `self.t` inside the synth function. Their ADSR
//  is Attack-only (opens the gate) then Release (closes it).
// ─────────────────────────────────────────────────────────────

#[derive(PartialEq, Clone, Copy)]
enum EnvState { Idle, Attack, Decay, Sustain, Release }

struct Voice {
    freq:              f32,
    peak_amplitude:    f32,   // velocity - the target for Attack
    sustain_level:     f32,   // peak * envelope.sustain
    current_vol:       f32,
    state:             EnvState,
    samples_remaining: u32,   // note duration countdown (starts at Sustain)
    attack_inc:        f32,   // +vol per sample in Attack
    decay_dec:         f32,   // -vol per sample in Decay
    release_dec:       f32,   // -vol per sample in Release
    instrument:        Instrument,
    istate:            InstrumentState,
}

impl Voice {
    fn new() -> Self {
        Self {
            freq:              440.0,
            peak_amplitude:    0.0,
            sustain_level:     0.0,
            current_vol:       0.0,
            state:             EnvState::Idle,
            samples_remaining: 0,
            attack_inc:        0.0,
            decay_dec:         0.0,
            release_dec:       0.0,
            instrument:        Instrument::Sine,
            istate:            InstrumentState::new(),
        }
    }

    // Trigger a new note using the provided envelope.
    // Call site passes either the layer override or instrument default.
    fn trigger(&mut self, event: &NoteEvent, env: EnvelopeConfig, sample_rate: f32) {
        self.freq           = midi_to_freq(event.note);
        self.peak_amplitude = event.velocity;
        self.sustain_level  = event.velocity * env.sustain;
        self.instrument     = event.instrument;
        self.current_vol    = 0.0;

        // Duration countdown: the note stays in Sustain for this many samples.
        // Attack + Decay happen before Sustain, so we don't subtract them here -
        // short notes may skip Sustain entirely if duration < attack+decay time.
        self.samples_remaining = (event.duration / 1000.0 * sample_rate) as u32;

        // Precompute per-sample increments (avoids hot-loop divisions)
        let att  = (env.attack_ms  / 1000.0 * sample_rate).max(1.0);
        let dec  = (env.decay_ms   / 1000.0 * sample_rate).max(1.0);
        let rel  = (env.release_ms / 1000.0 * sample_rate).max(1.0);
        self.attack_inc  = self.peak_amplitude / att;
        self.decay_dec   = (self.peak_amplitude - self.sustain_level) / dec;
        self.release_dec = self.sustain_level / rel;

        // If decay is negligible (0 ms), skip straight to Sustain
        self.state = EnvState::Attack;

        self.istate.reset(self.freq, sample_rate, self.instrument);
    }

    fn next_sample(&mut self, sample_rate: f32) -> f32 {
        match self.state {
            EnvState::Idle => return 0.0,

            EnvState::Attack => {
                self.current_vol += self.attack_inc;
                if self.current_vol >= self.peak_amplitude {
                    self.current_vol = self.peak_amplitude;
                    // Jump straight to Sustain if no decay configured
                    self.state = if self.decay_dec > 0.0001 {
                        EnvState::Decay
                    } else {
                        EnvState::Sustain
                    };
                }
            }

            EnvState::Decay => {
                self.current_vol -= self.decay_dec;
                if self.current_vol <= self.sustain_level {
                    self.current_vol = self.sustain_level;
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
                // Percussion instruments decay to 0 on their own via `t`.
                // We still run Release so the voice slot is freed correctly.
                self.current_vol -= self.release_dec;
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

// ─────────────────────────────────────────────────────────────
//  ENGINE
// ─────────────────────────────────────────────────────────────

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
                // Drain events once per buffer - not per sample.
                while let Some(event) = queue.pop() {
                    // Use the envelope embedded in the NoteEvent
                    // (set by composer from LayerConfig or instrument default).
                    let env = event.envelope;
                    if let Some(v) = voices.iter_mut()
                        .find(|v| v.state == EnvState::Idle)
                    {
                        v.trigger(&event, env, sample_rate);
                    } else if let Some(v) = voices.iter_mut()
                        .min_by_key(|v| v.samples_remaining)
                    {
                        v.trigger(&event, env, sample_rate);
                    }
                }

                for sample in data.iter_mut() {
                    let out: f32 = voices.iter_mut()
                        .map(|v| v.next_sample(sample_rate))
                        .sum();

                    let mixed = out / MAX_VOICES as f32 * 2.0;
                    // Soft clip: x / (1 + |x|) - gentle saturation, no harsh clipping
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
