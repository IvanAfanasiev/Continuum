use crate::NoteEvent;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_queue::ArrayQueue;
use std::sync::Arc;

pub fn start_engine(queue: Arc<ArrayQueue<NoteEvent>>) {
    let host = cpal::default_host();
    let device = host.default_output_device().expect("Output device not found");
    let config = device.default_output_config().unwrap().config();
    let sample_rate = config.sample_rate.0 as f32;

    let mut phase = 0.0;
    let mut current_freq = 0.0;

    // create output stream
    let stream = device.build_output_stream(
        &config,
        move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
            for sample in data.iter_mut() {
                // If a new note arrives from the queue, update the frequency
                if let Some(event) = queue.pop() {
                    current_freq = event.note;
                }

                // generate sine wave sample
                *sample = (phase * 2.0 * std::f32::consts::PI).sin() * 0.1;
                phase = (phase + current_freq / sample_rate) % 1.0;
            }
        },
        |err| eprintln!("audio error: {}", err),
        None
    ).expect("output stream error");

    stream.play().expect("stream play error");
    
    // keep the stream running
    loop { std::thread::sleep(std::time::Duration::from_millis(100)); }
}