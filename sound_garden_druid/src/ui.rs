mod app;
mod constants;
mod eventer;
mod scene;
mod text_line;
mod util;

use anyhow::Result;

use crate::state::State;
use druid::{AppLauncher, LocalizedString, WindowDesc};

pub fn run() -> Result<()> {
    let window = WindowDesc::new(app::Widget::new).title(LocalizedString::new("window-title"));

    let state = State::load(constants::STATE_FILE).unwrap_or_default();

    AppLauncher::with_window(window)
        .use_simple_logger()
        .launch(state)
        .map_err(|_| anyhow::anyhow!("Launch failed."))?;

    Ok(())
}
