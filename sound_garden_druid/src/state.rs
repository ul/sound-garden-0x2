use anyhow::Result;
use druid::Data;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct State {
    pub scene: Scene,
    // TODO Arc<Vec<...>> ?
    pub plants: Vec<Plant>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Plant {
    pub position: Position,
    // TODO Arc<Vec<...>> ?
    pub nodes: Vec<Node>,
    pub name: String,
}

#[derive(Clone, Data, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Node {
    pub op: String,
    pub position: Position,
}

#[derive(Clone, Data, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum Scene {
    Garden(GardenScene),
    Plant(PlantScene),
}

#[derive(Clone, Data, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct GardenScene {
    pub offset: Position,
}

#[derive(Clone, Data, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PlantScene {
    pub ix: PlantIx,
    pub cursor: Position,
    pub mode: PlantSceneMode,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum PlantSceneMode {
    Normal,
    Insert,
    // TODO Arc<Vec<...>> ?
    Move(Vec<NodeIx>),
}

pub type PlantIx = usize;
pub type NodeIx = usize;

#[derive(Clone, Copy, Data, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Position {
    pub x: i32,
    pub y: i32,
}

impl Into<(i32, i32)> for Position {
    fn into(self) -> (i32, i32) {
        (self.x, self.y)
    }
}

impl From<(i32, i32)> for Position {
    fn from(p: (i32, i32)) -> Self {
        Position { x: p.0, y: p.1 }
    }
}

impl State {
    pub fn new() -> Self {
        State {
            scene: Scene::Garden(GardenScene {
                offset: (0, 0).into(),
            }),
            plants: Vec::new(),
        }
    }

    pub fn save<P: AsRef<std::path::Path>>(&self, path: P) -> Result<()> {
        let f = std::fs::File::create(path)?;
        serde_json::to_writer_pretty(f, self)?;
        Ok(())
    }

    pub fn load<P: AsRef<std::path::Path>>(path: P) -> Result<Self> {
        let f = std::fs::File::open(path)?;
        Ok(serde_json::from_reader(f)?)
    }
}

impl Default for State {
    fn default() -> Self {
        State::new()
    }
}

impl Data for State {
    fn same(&self, other: &Self) -> bool {
        self == other
    }
}

impl Data for Plant {
    fn same(&self, other: &Self) -> bool {
        self == other
    }
}

impl Data for PlantSceneMode {
    fn same(&self, other: &Self) -> bool {
        self == other
    }
}
