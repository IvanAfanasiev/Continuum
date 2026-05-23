use crate::NoteEvent;
use std::sync::Arc;
use crossbeam_queue::ArrayQueue;
use std::thread;
use std::time::Duration;

pub fn start_composing(queue: Arc<ArrayQueue<NoteEvent>>){
    // (A, B, C, D)
    let notes = [440.0, 493.88, 523.25, 587.33];
    let mut i = 0;

    loop { 
        // pick a note (through modulo operation)
        let freq = notes[i % notes.len()];

        let note = NoteEvent {
            note: freq,
            duration: 500.0,
        };
        // try to push into the queue
        // if the queue is full, it's better to wait than to panic
        if queue.push(note).is_ok() {
            println!("composer: sent note {} Hz", freq);
        }

        i += 1;
        
        // wait before sending the next note
        thread::sleep(Duration::from_millis(500));
    }
}