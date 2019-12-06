mod audio;
mod names;
mod state;
mod ui;

use anyhow::Result;
use audio_vm::VM;
use std::sync::{Arc, Mutex};
use thread_worker::Worker;

const CHANNEL_CAPACITY: usize = 64;

pub fn main() -> Result<()> {
    simple_logger::init()?;

    let vm = Arc::new(Mutex::new(VM::new()));

    let audio_wrk = {
        let vm = Arc::clone(&vm);
        Worker::spawn("Audio", CHANNEL_CAPACITY, move |i, o| {
            audio::main(vm, i, o).unwrap();
        })
    };

    let sample_rate = audio_wrk.receiver().recv()?;

    ui::run(vm, sample_rate)?;

    Ok(())
}
