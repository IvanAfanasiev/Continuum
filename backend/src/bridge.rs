use crate::audio_engine::AudioRenderer;
use crate::instruments::{Instrument, INSTRUMENT_COUNT};
use crate::markov;
use crate::runtime::CoreRuntime;
use std::ffi::CStr;
use std::os::raw::{c_char, c_float, c_uint};
use std::ptr;

pub struct ContinuumMobileRuntime {
    core: CoreRuntime,
    renderer: AudioRenderer,
    channels: usize,
}

#[no_mangle]
pub extern "C" fn continuum_mobile_create(
    preset_name: *const c_char,
    sample_rate: c_float,
    channels: c_uint,
) -> *mut ContinuumMobileRuntime {
    let preset = read_preset_name(preset_name).unwrap_or("Ambient");
    create_mobile_runtime(preset, sample_rate, channels)
}

#[no_mangle]
pub extern "C" fn continuum_mobile_create_preset(
    preset_id: c_uint,
    sample_rate: c_float,
    channels: c_uint,
) -> *mut ContinuumMobileRuntime {
    let preset = markov::PRESET_NAMES
        .get(preset_id as usize)
        .copied()
        .unwrap_or("Ambient");
    create_mobile_runtime(preset, sample_rate, channels)
}

fn create_mobile_runtime(
    preset_name: &'static str,
    sample_rate: c_float,
    channels: c_uint,
) -> *mut ContinuumMobileRuntime {
    let core = CoreRuntime::start(preset_name);
    let channels = channels.max(1) as usize;
    let renderer = AudioRenderer::new(
        core.queue(),
        core.controls(),
        sample_rate.max(8000.0),
        channels,
    );

    Box::into_raw(Box::new(ContinuumMobileRuntime {
        core,
        renderer,
        channels,
    }))
}

#[no_mangle]
pub unsafe extern "C" fn continuum_mobile_destroy(runtime: *mut ContinuumMobileRuntime) {
    if runtime.is_null() {
        return;
    }

    let mut runtime = Box::from_raw(runtime);
    runtime.core.stop();
}

#[no_mangle]
pub unsafe extern "C" fn continuum_mobile_render(
    runtime: *mut ContinuumMobileRuntime,
    output: *mut c_float,
    frames: c_uint,
) -> c_uint {
    if runtime.is_null() || output.is_null() {
        return 0;
    }

    let runtime = &mut *runtime;
    let sample_count = frames as usize * runtime.channels;
    let output = std::slice::from_raw_parts_mut(output, sample_count);
    runtime.renderer.render_interleaved_f32(output);
    frames
}

#[no_mangle]
pub unsafe extern "C" fn continuum_mobile_set_paused(
    runtime: *mut ContinuumMobileRuntime,
    paused: bool,
) {
    if let Some(runtime) = runtime.as_ref() {
        runtime.core.set_paused(paused);
    }
}

#[no_mangle]
pub unsafe extern "C" fn continuum_mobile_set_tempo(
    runtime: *mut ContinuumMobileRuntime,
    value: c_float,
) {
    if let Some(controls) = controls(runtime) {
        controls.set_tempo(value);
    }
}

#[no_mangle]
pub unsafe extern "C" fn continuum_mobile_set_swing(
    runtime: *mut ContinuumMobileRuntime,
    value: c_float,
) {
    if let Some(controls) = controls(runtime) {
        controls.set_swing(value);
    }
}

#[no_mangle]
pub unsafe extern "C" fn continuum_mobile_set_instrument_volume(
    runtime: *mut ContinuumMobileRuntime,
    instrument_id: c_uint,
    value: c_float,
) -> bool {
    controls(runtime).is_some_and(|controls| {
        controls.set_instrument_volume_by_index(instrument_id as usize, value)
    })
}

#[no_mangle]
pub extern "C" fn continuum_instrument_count() -> c_uint {
    INSTRUMENT_COUNT as c_uint
}

#[no_mangle]
pub extern "C" fn continuum_preset_count() -> c_uint {
    markov::PRESET_NAMES.len() as c_uint
}

#[no_mangle]
pub extern "C" fn continuum_preset_name(preset_id: c_uint) -> *const c_char {
    match markov::PRESET_NAMES.get(preset_id as usize).copied() {
        Some("Ambient") => c"Ambient".as_ptr(),
        Some("Jazz") => c"Jazz".as_ptr(),
        _ => ptr::null(),
    }
}

#[no_mangle]
pub extern "C" fn continuum_instrument_name(instrument_id: c_uint) -> *const c_char {
    match Instrument::from_control_index(instrument_id as usize) {
        Some(Instrument::Piano) => c"Piano".as_ptr(),
        Some(Instrument::Pad) => c"Pad".as_ptr(),
        Some(Instrument::Bass) => c"Bass".as_ptr(),
        Some(Instrument::Triangle) => c"Triangle".as_ptr(),
        Some(Instrument::Kick) => c"Kick".as_ptr(),
        Some(Instrument::Ride) => c"Ride".as_ptr(),
        Some(Instrument::Hihat) => c"Hihat".as_ptr(),
        None => ptr::null(),
    }
}

unsafe fn controls(
    runtime: *mut ContinuumMobileRuntime,
) -> Option<std::sync::Arc<crate::RuntimeControls>> {
    runtime.as_ref().map(|runtime| runtime.core.controls())
}

fn read_preset_name(value: *const c_char) -> Option<&'static str> {
    if value.is_null() {
        return None;
    }

    let value = unsafe { CStr::from_ptr(value) }.to_str().ok()?;
    match value.to_ascii_lowercase().as_str() {
        "ambient" => Some("Ambient"),
        "jazz" => Some("Jazz"),
        _ => None,
    }
}
