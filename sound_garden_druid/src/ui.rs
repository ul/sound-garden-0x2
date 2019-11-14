mod app;
mod constants;
mod label;
mod scene;

use anyhow::Result;

use crate::state::State;
use druid::{AppLauncher, LocalizedString, WindowDesc};

pub fn run() -> Result<()> {
    let window = WindowDesc::new(app::App::new).title(LocalizedString::new("window-title"));

    AppLauncher::with_window(window)
        .use_simple_logger()
        .launch(State::new())
        .map_err(|_| anyhow::anyhow!("Launch failed."))?;

    Ok(())
}
