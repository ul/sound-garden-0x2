use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct World {
    pub anima: Anima,
    pub garden: Garden,
    pub plants: Vec<Plant>,
    pub screen: Screen,
    pub cell_size: (u32, u32),
    pub sample_rate: u32,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Anima {}

#[derive(Serialize, Deserialize, Debug)]
pub struct Plant {
    pub position: Point,
    pub nodes: Vec<Node>, // SortedSet?
    pub edges: Vec<(usize, usize)>,
    pub symbol: char,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Node {
    pub op: String,
    pub position: Point,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Garden {
    pub anima_position: Point,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct PlantEditor {
    pub ix: usize,
    pub cursor_position: Point,
    pub mode: PlantEditorMode,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum PlantEditorMode {
    Normal,
    Insert,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum Screen {
    Garden,
    Plant(PlantEditor),
}

impl World {
    pub fn new() -> Self {
        World {
            anima: Anima {},
            garden: Garden {
                anima_position: Point { x: 0, y: 0 },
            },
            plants: Vec::new(),
            screen: Screen::Garden,
            cell_size: (0, 0),
            sample_rate: 48000,
        }
    }
}
