use crate::instruments::{Instrument, INSTRUMENT_COUNT};
use std::sync::atomic::{AtomicU32, Ordering};

pub struct RuntimeControls {
    tempo: AtomicU32,
    swing: AtomicU32,
    instrument_volumes: [AtomicU32; INSTRUMENT_COUNT],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlCommand {
    Applied,
    Stop,
    Unknown,
}

impl Default for RuntimeControls {
    fn default() -> Self {
        Self::new()
    }
}

impl RuntimeControls {
    pub fn new() -> Self {
        Self {
            tempo: atomic_f32(1.0),
            swing: atomic_f32(0.5),
            instrument_volumes: std::array::from_fn(|_| atomic_f32(1.0)),
        }
    }

    pub fn tempo(&self) -> f32 {
        load_f32(&self.tempo)
    }

    pub fn set_tempo(&self, value: f32) {
        store_f32(&self.tempo, value.clamp(0.25, 3.0));
    }

    pub fn swing(&self) -> f32 {
        load_f32(&self.swing)
    }

    pub fn set_swing(&self, value: f32) {
        store_f32(&self.swing, value.clamp(0.0, 1.0));
    }

    pub fn instrument_volume(&self, instrument: Instrument) -> f32 {
        load_f32(&self.instrument_volumes[instrument.control_index()])
    }

    pub fn set_instrument_volume(&self, instrument: Instrument, value: f32) {
        store_f32(
            &self.instrument_volumes[instrument.control_index()],
            value.clamp(0.0, 1.5),
        );
    }

    pub fn set_instrument_volume_by_index(&self, index: usize, value: f32) -> bool {
        let Some(instrument) = Instrument::from_control_index(index) else {
            return false;
        };
        self.set_instrument_volume(instrument, value);
        true
    }

    pub fn set_instrument_volume_by_name(&self, name: &str, value: f32) -> bool {
        let Some(instrument) = Instrument::from_name(name) else {
            return false;
        };
        self.set_instrument_volume(instrument, value);
        true
    }
}

pub fn apply_control_line(controls: &RuntimeControls, line: &str) -> ControlCommand {
    let mut parts = line.split_whitespace();
    let Some(command) = parts.next().map(|part| part.to_ascii_lowercase()) else {
        return ControlCommand::Unknown;
    };

    match command.as_str() {
        "quit" | "stop" | "exit" => ControlCommand::Stop,
        "tempo" => {
            let Some(value) = parse_value(parts.next()) else {
                return ControlCommand::Unknown;
            };
            controls.set_tempo(value);
            ControlCommand::Applied
        }
        "swing" => {
            let Some(value) = parse_value(parts.next()) else {
                return ControlCommand::Unknown;
            };
            controls.set_swing(value);
            ControlCommand::Applied
        }
        "instrument" | "inst" => {
            let Some(name) = parts.next() else {
                return ControlCommand::Unknown;
            };
            let Some(value) = parse_value(parts.next()) else {
                return ControlCommand::Unknown;
            };
            if controls.set_instrument_volume_by_name(name, value) {
                ControlCommand::Applied
            } else {
                ControlCommand::Unknown
            }
        }
        "set" => {
            let Some(target) = parts.next() else {
                return ControlCommand::Unknown;
            };
            let rest = parts.collect::<Vec<_>>().join(" ");
            apply_control_line(controls, &format!("{target} {rest}"))
        }
        _ => ControlCommand::Unknown,
    }
}

fn parse_value(value: Option<&str>) -> Option<f32> {
    value?.parse::<f32>().ok()
}

fn atomic_f32(value: f32) -> AtomicU32 {
    AtomicU32::new(value.to_bits())
}

fn load_f32(value: &AtomicU32) -> f32 {
    f32::from_bits(value.load(Ordering::Relaxed))
}

fn store_f32(target: &AtomicU32, value: f32) {
    target.store(value.to_bits(), Ordering::Relaxed);
}
