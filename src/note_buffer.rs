use crate::NoteEvent;
use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex};

// Thread-safe buffer between the LLM thread and the composer thread.
//
// The LLM writes notes in batches (8-16 at a time); the composer reads
// one note at a time. A Condvar is used so both threads sleep when idle
// instead of spinning, no busy-wait anywhere in this design.
pub struct NoteBuffer {
    inner: Mutex<VecDeque<NoteEvent>>,
    // Both threads wait on this condvar: the LLM waits when the buffer
    // is full enough; the composer waits when it is empty.
    condvar: Condvar,
    // When the buffer drops below this count the LLM is woken to refill.
    refill_threshold: usize,
}

impl NoteBuffer {
    pub fn new(refill_threshold: usize) -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(VecDeque::with_capacity(64)),
            condvar: Condvar::new(),
            refill_threshold,
        })
    }

    // Composer calls this to take the next note.
    // Blocks if the buffer is empty until the LLM pushes new notes.
    pub fn pop(&self) -> NoteEvent {
        let mut buf = self.inner.lock().unwrap();
        loop {
            if let Some(event) = buf.pop_front() {
                // Wake the LLM if we have fallen below the refill threshold.
                if buf.len() < self.refill_threshold {
                    self.condvar.notify_one();
                }
                return event;
            }
            // Buffer is empty = sleep until push_batch signals us.
            buf = self.condvar.wait(buf).unwrap();
        }
    }

    // LLM calls this to push a batch of generated notes.
    // Wakes the composer if it was waiting for notes.
    pub fn push_batch(&self, notes: Vec<NoteEvent>) {
        let mut buf = self.inner.lock().unwrap();
        buf.extend(notes);
        self.condvar.notify_all();
    }

    // Returns the current number of buffered notes.
    pub fn len(&self) -> usize {
        self.inner.lock().unwrap().len()
    }

    // LLM calls this to sleep while the buffer is sufficiently full.
    // Returns as soon as the buffer drops below the refill threshold.
    pub fn wait_until_refill_needed(&self) {
        let mut buf = self.inner.lock().unwrap();
        while buf.len() >= self.refill_threshold {
            buf = self.condvar.wait(buf).unwrap();
        }
    }
}
