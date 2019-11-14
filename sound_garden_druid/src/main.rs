mod state;
mod ui;

use anyhow::Result;

pub fn main() -> Result<()> {
    simple_logger::init()?;

    ui::run()?;

    Ok(())
}
