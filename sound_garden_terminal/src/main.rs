mod audio;
mod event;
mod record;
mod ui;

use anyhow::{anyhow, Result};
use audio_vm::{Sample, VM};
use ringbuf::RingBuffer;
use std::sync::{Arc, Mutex};
use thread_worker::Worker;

const CHANNEL_CAPACITY: usize = 64;
/// It's about 500ms, should be more than enough for write cycle of ~10ms.
const RECORD_BUFFER_CAPACITY: usize = 48000;

pub fn main() -> Result<()> {
    let filename = std::env::args().skip(1).next();

    if filename.is_none() {
        return Err(anyhow!("Filename is required."));
    }

    let filename = filename.unwrap();

    let vm = Arc::new(Mutex::new(VM::new()));
    {
        vm.lock().unwrap().stop();
    }
    let rb = RingBuffer::<Sample>::new(RECORD_BUFFER_CAPACITY);
    let (producer, consumer) = rb.split();

    let audio_wrk = {
        let vm = Arc::clone(&vm);
        Worker::spawn("Audio", CHANNEL_CAPACITY, move |i, o| {
            audio::main(vm, producer, i, o).unwrap();
        })
    };

    let sample_rate = audio_wrk.receiver().recv()?;

    let record_wrk = {
        let filename = filename.clone();
        Worker::spawn("Record", CHANNEL_CAPACITY, move |i, o| {
            record::main(&filename, sample_rate, consumer, i, o).unwrap();
        })
    };

    ui::main(vm, sample_rate, &filename, record_wrk.sender())?;

    drop(record_wrk);
    drop(audio_wrk);

    Ok(())
}
