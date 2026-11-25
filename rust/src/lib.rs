pub mod api;
pub mod arranger;
pub mod composer;
pub mod core;
pub mod engine;
pub mod instruments;
pub mod mixer;
pub mod piece;
pub mod playback;
pub mod proto;
// On desktop the native stream is stubbed out (cpal is used there); dead code
// in the Android half is explicitly allowed here.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub mod pull_thread;
pub mod seed;
pub mod space;
pub mod styles;

#[cfg(target_os = "android")]
pub mod android;

mod frb_generated;
