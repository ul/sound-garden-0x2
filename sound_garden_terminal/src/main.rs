mod audio;
mod event;
mod ui;

use anyhow::{anyhow, Result};
use audio_vm::VM;
use std::sync::{Arc, Mutex};
use thread_worker::Worker;

const CHANNEL_CAPACITY: usize = 64;

pub fn main() -> Result<()> {
    let vm = Arc::new(Mutex::new(VM::new()));

    let audio_wrk = {
        let vm = Arc::clone(&vm);
        Worker::spawn("Audio", CHANNEL_CAPACITY, move |i, o| {
            audio::main(vm, i, o).unwrap();
        })
    };

    let sample_rate = audio_wrk.receiver().recv()?;

    let filename = std::env::args().skip(1).next();

    if filename.is_none() {
        return Err(anyhow!("Filename is required."));
    }

    ui::run(vm, sample_rate, filename.unwrap())?;

    Ok(())
}
