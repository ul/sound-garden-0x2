pub const FONT_NAME: &str = "Agave";
pub const PLANT_FONT_SIZE: f64 = 16.0;
pub const STATE_FILE: &str = "garden.json";
pub const DOUBLE_CLICK_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(200);

pub mod cmd {
    use crate::state;
    use druid::{Command, Selector};

    pub const REQUEST_FOCUS: Selector = Selector::new("SOUND_GARDEN.REQUEST_FOCUS");
    pub const BACK_TO_GARDEN: Selector = Selector::new("SOUND_GARDEN.BACK_TO_GARDEN");
    pub const ZOOM_TO_PLANT: Selector = Selector::new("SOUND_GARDEN.ZOOM_TO_PLANT");

    pub fn back_to_garden(offset: state::Position) -> Command {
        Command::new(BACK_TO_GARDEN, offset)
    }

    pub fn zoom_to_plant(ix: state::PlantIx) -> Command {
        Command::new(ZOOM_TO_PLANT, ix)
    }
}
