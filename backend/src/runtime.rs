use crate::composer;
use crate::controls::RuntimeControls;
use crate::NoteEvent;
use crossbeam_queue::ArrayQueue;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

const EVENT_QUEUE_CAPACITY: usize = 1024;

pub struct CoreRuntime {
    queue: Arc<ArrayQueue<NoteEvent>>,
    controls: Arc<RuntimeControls>,
    stop: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    composer_thread: Option<JoinHandle<()>>,
}

impl CoreRuntime {
    pub fn start(preset_name: &str) -> Self {
        let queue = Arc::new(ArrayQueue::<NoteEvent>::new(EVENT_QUEUE_CAPACITY));
        let controls = Arc::new(RuntimeControls::new());
        let stop = Arc::new(AtomicBool::new(false));
        Self::start_with_parts(queue, controls, stop, preset_name)
    }

    pub fn start_with_parts(
        queue: Arc<ArrayQueue<NoteEvent>>,
        controls: Arc<RuntimeControls>,
        stop: Arc<AtomicBool>,
        preset_name: &str,
    ) -> Self {
        let composer_queue = queue.clone();
        let composer_controls = controls.clone();
        let composer_stop = stop.clone();
        let paused = Arc::new(AtomicBool::new(false));
        let composer_paused = paused.clone();
        let preset_name = preset_name.to_string();
        let composer_thread = thread::spawn(move || {
            composer::start_composing(
                composer_queue,
                &preset_name,
                composer_controls,
                composer_stop,
                composer_paused,
            );
        });

        Self {
            queue,
            controls,
            stop,
            paused,
            composer_thread: Some(composer_thread),
        }
    }

    pub fn queue(&self) -> Arc<ArrayQueue<NoteEvent>> {
        self.queue.clone()
    }

    pub fn controls(&self) -> Arc<RuntimeControls> {
        self.controls.clone()
    }

    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.paused.store(false, Ordering::Relaxed);
        if let Some(thread) = self.composer_thread.take() {
            let _ = thread.join();
        }
    }

    pub fn set_paused(&self, paused: bool) {
        self.paused.store(paused, Ordering::Relaxed);
        self.controls.set_paused(paused);
    }
}

impl Drop for CoreRuntime {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(feature = "desktop-audio")]
pub struct DesktopRuntime {
    core: CoreRuntime,
    _audio_engine: crate::audio_engine::AudioEngine,
}

#[cfg(feature = "desktop-audio")]
impl DesktopRuntime {
    pub fn start(preset_name: &str) -> Result<Self, String> {
        let queue = Arc::new(ArrayQueue::<NoteEvent>::new(EVENT_QUEUE_CAPACITY));
        let controls = Arc::new(RuntimeControls::new());
        let stop = Arc::new(AtomicBool::new(false));
        let audio_engine = crate::audio_engine::start_engine(queue.clone(), controls.clone())?;
        let core = CoreRuntime::start_with_parts(queue, controls, stop, preset_name);

        Ok(Self {
            core,
            _audio_engine: audio_engine,
        })
    }

    pub fn controls(&self) -> Arc<RuntimeControls> {
        self.core.controls()
    }

    pub fn stop(&mut self) {
        self.core.stop();
    }

    pub fn set_paused(&self, paused: bool) {
        self.core.set_paused(paused);
    }
}
