pub const FONT_NAME: &str = "Agave";
pub const PLANT_FONT_SIZE: f64 = 16.0;
pub const STATE_FILE: &str = "garden.json";

pub mod cmd {
    use druid::Selector;

    pub const BACK_TO_GARDEN: Selector = Selector::new("SOUND_GARDEN.BACK_TO_GARDEN");
    pub const ZOOM_TO_PLANT: Selector = Selector::new("SOUND_GARDEN.ZOOM_TO_PLANT");
}
