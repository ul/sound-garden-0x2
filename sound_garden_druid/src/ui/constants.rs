pub const FONT_NAME: &str = "Agave";
pub const PLANT_FONT_SIZE: f64 = 16.0;
pub const STATE_FILE: &str = "garden.json";
pub const DOUBLE_CLICK_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(200);

pub mod cmd {
    use crate::state;
    use druid::{Command, MouseEvent, Selector};

    // NOTE: If selector has a payload then use a command creator fn to leverage typechecking.

    // App logic
    pub const REQUEST_FOCUS: Selector = Selector::new("SOUND_GARDEN.REQUEST_FOCUS");
    pub const BACK_TO_GARDEN: Selector = Selector::new("SOUND_GARDEN.BACK_TO_GARDEN");
    pub const ZOOM_TO_PLANT: Selector = Selector::new("SOUND_GARDEN.ZOOM_TO_PLANT");
    pub const REMOVE_NODE: Selector = Selector::new("SOUND_GARDEN.REMOVE_NODE");

    // Eventer extension
    pub const CLICK: Selector = Selector::new("SOUND_GARDEN.CLICK");
    pub const DOUBLE_CLICK: Selector = Selector::new("SOUND_GARDEN.DOUBLE_CLICK");

    pub fn back_to_garden() -> Command {
        Command::from(BACK_TO_GARDEN)
    }

    pub fn zoom_to_plant(ix: state::PlantIx) -> Command {
        Command::new(ZOOM_TO_PLANT, ix)
    }

    pub fn remove_node(ix: state::NodeIx) -> Command {
        Command::new(REMOVE_NODE, ix)
    }

    pub fn double_click(e: MouseEvent) -> Command {
        Command::new(DOUBLE_CLICK, e)
    }

    pub fn click(e: MouseEvent) -> Command {
        Command::new(CLICK, e)
    }
}
