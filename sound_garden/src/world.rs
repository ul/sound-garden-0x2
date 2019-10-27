use sdl2::rect::Point;

// Design decision I may regret soon: clone and re-create World when changing.
// To optimize might use PDS (i.e. rpds) or ensure that most of the things (all?)
// are allocated on the stack (smallvec, stackvec, arrayvec might help).
#[derive(Clone)]
pub struct World {
    pub anima: Anima,
    pub plants: Vec<Plant>,
}

#[derive(Clone)]
pub struct Anima {
    pub position: Point,
}

#[derive(Clone)]
pub struct Plant {
    pub position: Point,
}

impl World {
    pub fn new() -> Self {
        World {
            anima: Anima {
                position: Point::new(0, 0),
            },
            plants: Vec::new(),
        }
    }
}
