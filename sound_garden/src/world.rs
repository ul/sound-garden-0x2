use sdl2::{pixels::Color, rect::Point};

// Design decision I may regret soon: clone and re-create World when changing.
// To optimize might use PDS (i.e. rpds) or ensure that most of the things (all?)
// are allocated on the stack (smallvec, stackvec, arrayvec might help).
#[derive(Debug)]
pub struct World {
    pub anima: Anima,
    pub garden: Garden,
    pub plants: Vec<Plant>,
    pub screen: Screen,
    pub cell_size: (u32, u32),
    pub sample_rate: u32,
}

#[derive(Debug)]
pub struct Anima {}

#[derive(Debug)]
pub struct Plant {
    pub position: Point,
    pub nodes: Vec<Node>, // SortedSet?
    pub edges: Vec<(usize, usize)>,
    pub symbol: char,
    pub color: Color,
}

#[derive(Debug)]
pub struct Node {
    pub op: String,
    pub position: Point,
}

#[derive(Debug)]
pub struct Garden {
    pub anima_position: Point,
}

#[derive(Debug)]
pub struct PlantEditor {
    pub ix: usize,
    pub cursor_position: Point,
    pub mode: PlantEditorMode,
}

#[derive(Debug)]
pub enum PlantEditorMode {
    Normal,
    Insert,
}

#[derive(Debug)]
pub enum Screen {
    Garden,
    Plant(PlantEditor),
}

impl World {
    pub fn new() -> Self {
        World {
            anima: Anima {},
            garden: Garden {
                anima_position: Point::new(0, 0),
            },
            plants: Vec::new(),
            screen: Screen::Garden,
            cell_size: (0, 0),
            sample_rate: 48000,
        }
    }
}
