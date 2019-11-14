#[macro_use]
extern crate slog_scope;

mod state;
mod ui;

use anyhow::Result;
use sloggers::terminal::{Destination, TerminalLoggerBuilder};
use sloggers::types::Severity;
use sloggers::Build;

pub fn main() -> Result<()> {
    let _guard = set_logger()?;

    ui::run()?;

    Ok(())
}

pub fn set_logger() -> Result<slog_scope::GlobalLoggerGuard> {
    let mut builder = TerminalLoggerBuilder::new();
    builder.level(Severity::Trace);
    builder.destination(Destination::Stderr);
    let logger = builder.build()?;
    Ok(slog_scope::set_global_logger(logger))
}
