use anyhow::Result;
use druid::Data;
use serde::{Deserialize, Serialize};

#[derive(Clone, Data, Debug, Deserialize, Serialize)]
pub struct State {
    pub scene: Scene,
}

#[derive(Clone, Data, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum Scene {
    Garden(GardenScene),
    Plant,
}

#[derive(Clone, Data, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct GardenScene {
    pub cursor: (i32, i32),
}

impl State {
    pub fn new() -> Self {
        State {
            scene: Scene::Garden(GardenScene { cursor: (0, 0) }),
        }
    }

    pub fn save<P: AsRef<std::path::Path>>(&self, path: P) -> Result<()> {
        let f = std::fs::File::create(path)?;
        serde_json::to_writer_pretty(f, self)?;
        Ok(())
    }

    pub fn load<P: AsRef<std::path::Path>>(path: P) -> Result<Self> {
        let f = std::fs::File::create(path)?;
        Ok(serde_json::from_reader(f)?)
    }
}

impl Default for State {
    fn default() -> Self {
        State::new()
    }
}
