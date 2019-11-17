use crate::state::{self, Scene, State};

use druid::{
    kurbo::Size, BaseState, BoxConstraints, Env, Event, EventCtx, KeyCode, KeyEvent, LayoutCtx,
    PaintCtx, UpdateCtx, Widget,
};

pub struct PlantScene {}

impl Widget<State> for PlantScene {
    fn event(&mut self, event: &Event, ctx: &mut EventCtx, data: &mut State, _env: &Env) {
        if let Scene::Plant(scene_data) = &mut data.scene {
            match event {
                Event::MouseDown(e) => {
                    ctx.request_focus();
                    scene_data.cursor.x = e.pos.x as _;
                    scene_data.cursor.y = e.pos.y as _;
                    ctx.invalidate();
                }
                Event::KeyDown(KeyEvent { key_code, .. }) => match key_code {
                    KeyCode::Escape => {
                        data.scene = Scene::Garden(state::GardenScene {
                            cursor: data.plants[scene_data.ix].position,
                        });
                        ctx.invalidate();
                    }
                    _ => {}
                },
                _ => {}
            }
        } else {
            unreachable!();
        }
    }

    fn update(
        &mut self,
        _ctx: &mut UpdateCtx,
        _old_data: Option<&State>,
        _data: &State,
        _env: &Env,
    ) {
    }

    fn layout(
        &mut self,
        _ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        _data: &State,
        _env: &Env,
    ) -> Size {
        bc.max()
    }
    fn paint(&mut self, _ctx: &mut PaintCtx, _base_state: &BaseState, _data: &State, _env: &Env) {}
}

impl PlantScene {
    pub fn new() -> Self {
        PlantScene {}
    }
}
