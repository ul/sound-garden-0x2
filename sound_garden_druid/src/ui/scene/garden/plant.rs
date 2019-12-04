use crate::state;
use crate::ui::{constants::*, eventer, text_line};
use druid::{
    kurbo::{Point, Rect, Size},
    piet::Color,
    BaseState, BoxConstraints, Command, Data, Env, Event, EventCtx, LayoutCtx, PaintCtx, UpdateCtx,
    WidgetPod,Lens, LensWrap
};

pub struct Widget(eventer::Widget<State, InnerWidget>);

#[derive(Clone, Data, Debug)]
pub struct State {
    pub ix: state::PlantIx,
    pub name: String,
}

struct InnerWidget {
    name: WidgetPod<State, LensWrap<text_line::State, NameLens, text_line::Widget>>,
}

impl druid::Widget<State> for InnerWidget {
    fn event(&mut self, ctx: &mut EventCtx, event: &Event, data: &mut State, env: &Env) {
        self.name.event(ctx, event, data, env);
        match event {
            Event::Command(Command {
                selector: cmd::DOUBLE_CLICK,
                ..
            }) => {
                self.name.event(
                    ctx,
                    &Event::Command(Command::from(text_line::EDIT)),
                    data,
                    env,
                );
                ctx.set_handled();
            }
            Event::Command(Command {
                selector: cmd::CLICK,
                ..
            }) => {
                ctx.submit_command(cmd::zoom_to_plant(data.ix), None);
                ctx.set_handled();
            }
            _ => {}
        }
    }

    fn update(&mut self, ctx: &mut UpdateCtx, old_data: Option<&State>, data: &State, _env: &Env) {
        match old_data {
            Some(old_data) => {
                if old_data.name != data.name {
                    ctx.invalidate();
                }
            }
            None => ctx.invalidate(),
        }
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &State,
        env: &Env,
    ) -> Size {
        let size = self.name.layout(ctx, bc, data, env);
        self.name
            .set_layout_rect(Rect::from_origin_size(Point::ORIGIN, size));
        size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, _base_state: &BaseState, data: &State, env: &Env) {
        self.name.paint_with_offset(ctx, data, env);
    }
}

impl Widget {
    pub fn new() -> Self {
        Widget(eventer::Widget::new(InnerWidget {
            name: WidgetPod::new(LensWrap::new(text_line::Widget::new(), NameLens {})),
        }))
    }
}

impl State {
    pub fn new(ix: state::PlantIx, name: String) -> Self {
        State { ix, name }
    }
}

struct NameLens {}

impl Lens<State, text_line::State> for NameLens {
    fn with<V, F: FnOnce(&text_line::State) -> V>(&self, data: &State, f: F) -> V {
        f(&text_line::State::new(
            data.name.clone(),
            PLANT_FONT_SIZE,
            Color::BLACK,
        ))
    }

    fn with_mut<V, F: FnOnce(&mut text_line::State) -> V>(&self, data: &mut State, f: F) -> V {
        let mut lens = text_line::State::new(data.name.clone(), PLANT_FONT_SIZE, Color::BLACK);
        let result = f(&mut lens);
        data.name = lens.text;
        result
    }
}

impl druid::Widget<State> for Widget {
    fn event(&mut self, ctx: &mut EventCtx, event: &Event, data: &mut State, env: &Env) {
        self.0.event(ctx, event, data, env);
    }

    fn update(&mut self, ctx: &mut UpdateCtx, old_data: Option<&State>, data: &State, env: &Env) {
        self.0.update(ctx, old_data, data, env);
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &State,
        env: &Env,
    ) -> Size {
        self.0.layout(ctx, bc, data, env)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, base_state: &BaseState, data: &State, env: &Env) {
        self.0.paint(ctx, base_state, data, env)
    }
}
