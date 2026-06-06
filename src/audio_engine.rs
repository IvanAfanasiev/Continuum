use crate::instruments::{EnvelopeConfig, Instrument, InstrumentState};
use crate::midi_to_freq;
use crate::NoteEvent;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SampleFormat, SizedSample};
use crossbeam_queue::ArrayQueue;
use std::cmp::Ordering;
use std::sync::Arc;

const MAX_VOICES: usize = 24;
const MASTER_GAIN: f32 = 0.85;

pub struct AudioEngine {
    _stream: cpal::Stream,
}

#[derive(PartialEq, Clone, Copy)]
enum EnvState {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

struct Voice {
    freq: f32,
    peak_amplitude: f32,
    sustain_level: f32,
    current_vol: f32,
    state: EnvState,
    note_samples_remaining: u32,
    attack_inc: f32,
    decay_dec: f32,
    release_dec: f32,
    release_samples: f32,
    pan: f32,
    instrument: Instrument,
    istate: InstrumentState,
}

impl Voice {
    fn new() -> Self {
        Self {
            freq: 440.0,
            peak_amplitude: 0.0,
            sustain_level: 0.0,
            current_vol: 0.0,
            state: EnvState::Idle,
            note_samples_remaining: 0,
            attack_inc: 0.0,
            decay_dec: 0.0,
            release_dec: 0.0,
            release_samples: 1.0,
            pan: 0.0,
            instrument: Instrument::Sine,
            istate: InstrumentState::new(),
        }
    }

    fn is_active(&self) -> bool {
        self.state != EnvState::Idle
    }

    fn steal_score(&self) -> f32 {
        let state_cost = match self.state {
            EnvState::Idle => -1.0,
            EnvState::Release => 0.0,
            EnvState::Sustain => 0.15,
            EnvState::Decay => 0.25,
            EnvState::Attack => 0.35,
        };

        self.current_vol + state_cost
    }

    fn trigger(&mut self, event: &NoteEvent, env: EnvelopeConfig, sample_rate: f32) {
        let velocity = event.velocity.clamp(0.0, 1.0);
        let sustain = env.sustain.clamp(0.0, 1.0);
        let duration_ms = event.duration.max(1.0);

        self.freq = midi_to_freq(event.note);
        self.peak_amplitude = velocity;
        self.sustain_level = velocity * sustain;
        self.instrument = event.instrument;
        self.current_vol = 0.0;
        self.note_samples_remaining = (duration_ms / 1000.0 * sample_rate).max(1.0) as u32;
        self.pan = pan_for(event.instrument, event.note);

        let attack = (env.attack_ms / 1000.0 * sample_rate).max(1.0);
        let decay = (env.decay_ms / 1000.0 * sample_rate).max(1.0);
        let release = (env.release_ms / 1000.0 * sample_rate).max(1.0);

        self.attack_inc = self.peak_amplitude / attack;
        self.decay_dec = (self.peak_amplitude - self.sustain_level).max(0.0) / decay;
        self.release_samples = release;
        self.release_dec = self.peak_amplitude / release;
        self.state = EnvState::Attack;
        self.istate.reset(self.freq, sample_rate, self.instrument);
    }

    fn enter_release(&mut self) {
        if self.current_vol <= 0.0001 {
            self.current_vol = 0.0;
            self.state = EnvState::Idle;
            return;
        }

        self.release_dec = self.current_vol / self.release_samples.max(1.0);
        self.state = EnvState::Release;
    }

    fn next_frame(&mut self, sample_rate: f32) -> (f32, f32) {
        let mono = self.next_sample(sample_rate);
        if mono == 0.0 {
            return (0.0, 0.0);
        }

        let left_gain = ((1.0 - self.pan) * 0.5).sqrt();
        let right_gain = ((1.0 + self.pan) * 0.5).sqrt();
        (mono * left_gain, mono * right_gain)
    }

    fn next_sample(&mut self, sample_rate: f32) -> f32 {
        if self.state == EnvState::Idle {
            return 0.0;
        }

        if self.state != EnvState::Release {
            if self.note_samples_remaining == 0 {
                self.enter_release();
            } else {
                self.note_samples_remaining -= 1;
            }
        }

        match self.state {
            EnvState::Idle => return 0.0,
            EnvState::Attack => {
                self.current_vol += self.attack_inc;
                if self.current_vol >= self.peak_amplitude {
                    self.current_vol = self.peak_amplitude;
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
                        self.enter_release();
                    }
                }
            }
            EnvState::Sustain => {}
            EnvState::Release => {
                self.current_vol -= self.release_dec;
                if self.current_vol <= 0.0 {
                    self.current_vol = 0.0;
                    self.state = EnvState::Idle;
                }
            }
        }

        if self.current_vol <= 0.0 {
            return 0.0;
        }

        self.istate
            .next_sample(self.instrument, self.freq, self.current_vol, sample_rate)
    }
}

pub fn start_engine(queue: Arc<ArrayQueue<NoteEvent>>) -> Result<AudioEngine, String> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or_else(|| "output audio device was not found".to_string())?;
    let supported_config = device
        .default_output_config()
        .map_err(|err| format!("could not read default output config: {err}"))?;

    let sample_format = supported_config.sample_format();
    let config = supported_config.config();
    let sample_rate = config.sample_rate.0 as f32;
    let num_channels = config.channels as usize;

    if num_channels == 0 {
        return Err("output device reports zero channels".to_string());
    }

    let stream = match sample_format {
        SampleFormat::I8 => build_stream::<i8>(&device, &config, queue, sample_rate, num_channels),
        SampleFormat::I16 => {
            build_stream::<i16>(&device, &config, queue, sample_rate, num_channels)
        }
        SampleFormat::I32 => {
            build_stream::<i32>(&device, &config, queue, sample_rate, num_channels)
        }
        SampleFormat::I64 => {
            build_stream::<i64>(&device, &config, queue, sample_rate, num_channels)
        }
        SampleFormat::U8 => build_stream::<u8>(&device, &config, queue, sample_rate, num_channels),
        SampleFormat::U16 => {
            build_stream::<u16>(&device, &config, queue, sample_rate, num_channels)
        }
        SampleFormat::U32 => {
            build_stream::<u32>(&device, &config, queue, sample_rate, num_channels)
        }
        SampleFormat::U64 => {
            build_stream::<u64>(&device, &config, queue, sample_rate, num_channels)
        }
        SampleFormat::F32 => {
            build_stream::<f32>(&device, &config, queue, sample_rate, num_channels)
        }
        SampleFormat::F64 => {
            build_stream::<f64>(&device, &config, queue, sample_rate, num_channels)
        }
        other => Err(format!("unsupported sample format: {other}")),
    }?;

    stream
        .play()
        .map_err(|err| format!("failed to start audio stream: {err}"))?;

    println!(
        "[audio] output: {} Hz, {} channel(s), {}",
        sample_rate, num_channels, sample_format
    );

    Ok(AudioEngine { _stream: stream })
}

fn build_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    queue: Arc<ArrayQueue<NoteEvent>>,
    sample_rate: f32,
    num_channels: usize,
) -> Result<cpal::Stream, String>
where
    T: SizedSample + Sample + FromSample<f32>,
{
    let mut voices: Vec<Voice> = (0..MAX_VOICES).map(|_| Voice::new()).collect();

    device
        .build_output_stream(
            config,
            move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
                write_output(data, num_channels, sample_rate, queue.as_ref(), &mut voices);
            },
            |err| eprintln!("[audio] stream error: {err}"),
            None,
        )
        .map_err(|err| format!("failed to build output stream: {err}"))
}

fn write_output<T>(
    output: &mut [T],
    num_channels: usize,
    sample_rate: f32,
    queue: &ArrayQueue<NoteEvent>,
    voices: &mut [Voice],
) where
    T: Sample + FromSample<f32>,
{
    while let Some(event) = queue.pop() {
        trigger_voice(voices, &event, sample_rate);
    }

    for frame in output.chunks_mut(num_channels) {
        let active_count = voices
            .iter()
            .filter(|voice| voice.is_active())
            .count()
            .max(1) as f32;
        let voice_gain = MASTER_GAIN / active_count.sqrt();
        let mut left = 0.0f32;
        let mut right = 0.0f32;

        for voice in voices.iter_mut() {
            let (voice_left, voice_right) = voice.next_frame(sample_rate);
            left += voice_left;
            right += voice_right;
        }

        left = soft_limit(left * voice_gain);
        right = soft_limit(right * voice_gain);

        if num_channels == 1 {
            frame[0] = T::from_sample((left + right) * 0.5);
        } else {
            frame[0] = T::from_sample(left);
            frame[1] = T::from_sample(right);
            for sample in frame.iter_mut().skip(2) {
                *sample = T::from_sample((left + right) * 0.5);
            }
        }
    }
}

fn trigger_voice(voices: &mut [Voice], event: &NoteEvent, sample_rate: f32) {
    if let Some(voice) = voices.iter_mut().find(|voice| !voice.is_active()) {
        voice.trigger(event, event.envelope, sample_rate);
        return;
    }

    if let Some(voice) = voices.iter_mut().min_by(|a, b| {
        a.steal_score()
            .partial_cmp(&b.steal_score())
            .unwrap_or(Ordering::Equal)
    }) {
        voice.trigger(event, event.envelope, sample_rate);
    }
}

fn pan_for(instrument: Instrument, note: u8) -> f32 {
    match instrument {
        Instrument::Bass | Instrument::Kick | Instrument::Snare => 0.0,
        Instrument::Pad => {
            if note.is_multiple_of(2) {
                -0.35
            } else {
                0.35
            }
        }
        Instrument::Piano => ((note as f32 - 66.0) / 36.0).clamp(-0.35, 0.35),
        Instrument::Pluck | Instrument::Sine => ((note as f32 - 64.0) / 40.0).clamp(-0.45, 0.45),
        Instrument::Organ => -0.15,
        Instrument::Hihat => 0.28,
    }
}

fn soft_limit(sample: f32) -> f32 {
    let driven = sample * 1.35;
    (driven / (1.0 + driven.abs())) * 0.95
}
