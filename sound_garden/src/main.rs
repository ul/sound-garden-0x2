extern crate slog;
#[macro_use]
extern crate slog_scope;

mod audio;
mod error;
mod logic;
mod video;
mod world;

use anyhow::Result;
use audio_vm::VM;
use crossbeam_channel::Sender;
use sloggers::terminal::{Destination, TerminalLoggerBuilder};
use sloggers::types::Severity;
use sloggers::Build;
use std::sync::{Arc, Mutex};
use thread_worker::Worker;
use world::World;

const CHANNEL_CAPACITY: usize = 60;

pub fn main() -> Result<()> {
    let _guard = set_logger()?;

    let vm = Arc::new(Mutex::new(VM::new()));
    let world = Arc::new(Mutex::new(World::new()));

    let _audio_wrk = {
        let vm = Arc::clone(&vm);
        let world = Arc::clone(&world);
        Worker::spawn("Audio", CHANNEL_CAPACITY, move |i, _: Sender<()>| {
            audio::main(vm, world, i).unwrap();
        })
    };

    let logic_wrk = {
        let vm = Arc::clone(&vm);
        let world = Arc::clone(&world);
        Worker::spawn("Logic", CHANNEL_CAPACITY, move |i, _: Sender<()>| {
            logic::main(vm, world, i).unwrap();
        })
    };

    // Video must be on the main thread, at least for macOS.
    video::main(Arc::clone(&world), logic_wrk.sender().clone()).unwrap();

    Ok(())
}

pub fn set_logger() -> Result<slog_scope::GlobalLoggerGuard> {
    let mut builder = TerminalLoggerBuilder::new();
    builder.level(Severity::Trace);
    builder.destination(Destination::Stderr);
    let logger = builder.build()?;
    Ok(slog_scope::set_global_logger(logger))
}
