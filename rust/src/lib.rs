pub mod api;
pub mod arranger;
pub mod composer;
pub mod core;
pub mod engine;
pub mod instruments;
pub mod mixer;
pub mod playback;
pub mod proto;
// На десктопе нативный поток — заглушки (там cpal); мертвый код
// андроидной половины здесь разрешен явно.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub mod pull_thread;
pub mod seed;
pub mod space;
pub mod styles;

#[cfg(target_os = "android")]
pub mod android;

mod frb_generated;
