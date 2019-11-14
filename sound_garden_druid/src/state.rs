use druid::Data;

#[derive(Clone, Debug, Data)]
pub struct State {
    pub scene: Scene,
}

#[derive(Clone, Debug, Eq, PartialEq, Data)]
pub enum Scene {
    Garden(GardenScene),
    Plant,
}

#[derive(Clone, Debug, Eq, PartialEq, Data)]
pub struct GardenScene {
    pub cursor: (i32, i32),
}

impl State {
    pub fn new() -> Self {
        State {
            scene: Scene::Garden(GardenScene { cursor: (0, 0) }),
        }
    }
}
