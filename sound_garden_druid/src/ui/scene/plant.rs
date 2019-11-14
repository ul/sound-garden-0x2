use crate::state::State;

use druid::{
    kurbo::Size, BaseState, BoxConstraints, Env, Event, EventCtx, LayoutCtx, PaintCtx, UpdateCtx,
    Widget,
};

pub struct PlantScene {}

impl Widget<State> for PlantScene {
    fn paint(&mut self, _ctx: &mut PaintCtx, _base_state: &BaseState, _data: &State, _env: &Env) {}

    fn layout(
        &mut self,
        _ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        _data: &State,
        _env: &Env,
    ) -> Size {
        bc.max()
    }

    fn event(&mut self, _event: &Event, _ctx: &mut EventCtx, _data: &mut State, _env: &Env) {}

    fn update(
        &mut self,
        _ctx: &mut UpdateCtx,
        _old_data: Option<&State>,
        _data: &State,
        _env: &Env,
    ) {
    }
}

impl PlantScene {
    pub fn new() -> Self {
        PlantScene {}
    }
}
