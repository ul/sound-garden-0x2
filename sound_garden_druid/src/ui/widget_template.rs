use druid::{
    kurbo::Size, BaseState, BoxConstraints, Data, Env, Event, EventCtx, LayoutCtx, PaintCtx,
    UpdateCtx, WidgetPod,
};

pub struct Widget {}

#[derive(Clone, Data, Debug)]
pub struct State {}

impl druid::Widget<State> for Widget {
    fn event(&mut self, ctx: &mut EventCtx, event: &Event, data: &mut State, env: &Env) {}

    fn update(&mut self, ctx: &mut UpdateCtx, old_data: Option<&State>, data: &State, env: &Env) {}

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &State,
        env: &Env,
    ) -> Size {
        bc.max()
    }

    fn paint(&mut self, ctx: &mut PaintCtx, base_state: &BaseState, data: &State, env: &Env) {}
}

impl Widget {
    pub fn new() -> Self {
        Widget {}
    }
}

impl State {
    pub fn new() -> Self {
        State {}
    }
}
