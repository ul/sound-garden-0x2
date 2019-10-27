extern crate slog;
#[macro_use]
extern crate slog_scope;

mod audio;
mod error;
mod logic;
mod thread_worker;
mod video;
mod world;

use anyhow::Result;
use sloggers::terminal::{Destination, TerminalLoggerBuilder};
use sloggers::types::Severity;
use sloggers::Build;
use thread_worker::Worker;

const CHANNEL_CAPACITY: usize = 60;

pub fn main() -> Result<()> {
    let _guard = set_logger()?;

    let audio_wrk = Worker::spawn("Audio", CHANNEL_CAPACITY, move |i, o| {
        audio::main(i, o).unwrap();
    });

    let logic_wrk = Worker::spawn("Logic", CHANNEL_CAPACITY, move |i, o| {
        logic::main(i, o).unwrap();
    });

    let (video_tx, video_rx) = crossbeam_channel::bounded(CHANNEL_CAPACITY);

    let world_rx = logic_wrk.receiver().clone();
    let audio_tx = audio_wrk.sender().clone();
    let world_brodacaster = std::thread::spawn(move || {
        for world in world_rx {
            // Most likely audio should have a specialized subscription for sub-program modifications,
            // rather than process entire world by itself.
            let _ = audio_tx.try_send(world.clone());
            let _ = video_tx.try_send(world.clone());
        }
    });

    // Video must be on the main thread, at least for macOS.
    video::main(video_rx, logic_wrk.sender().clone())?;

    world_brodacaster.join().unwrap();

    Ok(())
}

pub fn set_logger() -> Result<slog_scope::GlobalLoggerGuard> {
    let mut builder = TerminalLoggerBuilder::new();
    builder.level(Severity::Trace);
    builder.destination(Destination::Stderr);
    let logger = builder.build()?;
    Ok(slog_scope::set_global_logger(logger))
}
