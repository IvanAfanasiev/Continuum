use crate::NoteEvent;
use crate::midi_to_freq;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_queue::ArrayQueue;
use std::sync::Arc;


struct SynthState {
    phase: f32,
    freq: f32,
    target_freq: f32,
    amplitude: f32, // current amplitude for smooth transitions
    target_amplitude: f32,
}
impl SynthState {
    fn new() -> Self {
        Self {
            phase: 0.0,
            freq: 440.0,
            target_freq: 440.0,
            amplitude: 0.0,
            target_amplitude: 0.0,
        }
    }
}

pub fn start_engine(queue: Arc<ArrayQueue<NoteEvent>>) {
    let host = cpal::default_host();
    let device = host.default_output_device().expect("Output device not found");
    let config = device.default_output_config().unwrap().config();

    let mut state = SynthState::new();
    let sample_rate = config.sample_rate.0 as f32;

    // create output stream
    let stream = device.build_output_stream(
        &config,
        move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
            for sample in data.iter_mut() {
                // If a new note arrives from the queue, update the frequency
                if let Some(event) = queue.pop() {
                    state.target_freq = midi_to_freq(event.note);
                    state.target_amplitude = event.velocity;
                }
                state.freq += (state.target_freq - state.freq) * 0.001;
                state.amplitude += (state.target_amplitude - state.amplitude) * 0.005;


                // generate sine wave sample
                state.phase = (state.phase + state.freq / sample_rate) % 1.0;
                *sample = (state.phase * 2.0 * std::f32::consts::PI).sin() * state.amplitude;
            }
        },
        |err| eprintln!("audio error: {}", err),
        None
    ).expect("output stream error");

    stream.play().expect("stream play error");
    
    // keep the stream running
    loop { std::thread::sleep(std::time::Duration::from_millis(100)); }
}