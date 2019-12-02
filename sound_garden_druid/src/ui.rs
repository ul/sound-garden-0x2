mod app;
mod constants;
mod delegate;
mod eventer;
mod scene;
mod text_line;
mod util;

use anyhow::Result;

use crate::state::State;
use audio_vm::VM;
use druid::{AppLauncher, LocalizedString, WindowDesc};
use std::sync::{Arc, Mutex};

pub fn run(vm: Arc<Mutex<VM>>, sample_rate: u32) -> Result<()> {
    let window = WindowDesc::new(app::Widget::new).title(LocalizedString::new("window-title"));

    let mut state = State::load(constants::STATE_FILE).unwrap_or_default();
    state.sample_rate = sample_rate;

    AppLauncher::with_window(window)
        .delegate(delegate::Delegate::new(vm))
        .use_simple_logger()
        .launch(state)
        .map_err(|_| anyhow::anyhow!("Launch failed."))?;

    Ok(())
}
