use sdl2::{pixels::Color, rect::Point};

// Design decision I may regret soon: clone and re-create World when changing.
// To optimize might use PDS (i.e. rpds) or ensure that most of the things (all?)
// are allocated on the stack (smallvec, stackvec, arrayvec might help).
#[derive(Clone, Debug)]
pub struct World {
    pub anima: Anima,
    pub garden: Garden,
    pub plants: Vec<Plant>,
    pub screen: Screen,
}

#[derive(Clone, Debug)]
pub struct Anima {}

#[derive(Clone, Debug)]
pub struct Plant {
    pub position: Point,
    pub nodes: Vec<Node>, // SortedSet?
    pub edges: Vec<(usize, usize)>,
    pub symbol: char,
    pub color: Color,
}

#[derive(Clone, Debug)]
pub struct Node {
    pub op: String,
    pub position: Point,
}

#[derive(Clone, Debug)]
pub struct Garden {
    pub anima_position: Point,
}

#[derive(Clone, Debug)]
pub struct PlantEditor {
    pub ix: usize,
    pub cursor_position: Point,
    pub mode: PlantEditorMode,
}

#[derive(Clone, Debug)]
pub enum PlantEditorMode {
    Normal,
    Insert,
}

#[derive(Clone, Debug)]
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
            plants: vec![Plant {
                position: Point::new(4, 4),
                nodes: Vec::new(),
                edges: Vec::new(),
                symbol: 'F',
                color: Color::from((0x22, 0x88, 0x11)),
            }],
            screen: Screen::Garden,
        }
    }
}
