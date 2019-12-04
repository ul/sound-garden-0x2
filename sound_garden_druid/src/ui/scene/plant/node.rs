use crate::state;
use crate::ui::{constants::*, text_line};
use druid::{
    kurbo::{Point, Rect, Size},
    piet::Color,
    BaseState, BoxConstraints, Command, Data, Env, Event, EventCtx, LayoutCtx, Lens, LensWrap,
    MouseEvent, PaintCtx, UpdateCtx, WidgetPod,
};

pub struct Widget {
    name: WidgetPod<State, LensWrap<text_line::State, OpLens, text_line::Widget>>,
}

#[derive(Clone, Data, Debug)]
pub struct State {
    pub ix: state::NodeIx,
    pub op: String,
}

impl druid::Widget<State> for Widget {
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
            Event::Command(c) if c.selector == cmd::CLICK => {
                let e = c.get_object::<MouseEvent>().unwrap();
                if e.mods.ctrl {
                    ctx.submit_command(cmd::remove_node(data.ix), None);
                }
                ctx.set_handled();
            }
            Event::MouseDown(e) => {
                if e.mods.meta {
                    // TODO move subtree
                } else {
                    // TODO add other cases
                    ctx.submit_command(cmd::drag_nodes(vec![data.ix]), None);
                }
            }
            _ => {}
        }
    }

    fn update(&mut self, ctx: &mut UpdateCtx, old_data: Option<&State>, data: &State, _env: &Env) {
        match old_data {
            Some(old_data) => {
                if !old_data.same(data) {
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
        Widget {
            name: WidgetPod::new(LensWrap::new(text_line::Widget::new(), OpLens {})),
        }
    }
}

impl State {
    pub fn new(ix: state::NodeIx, op: String) -> Self {
        State { ix, op }
    }
}

struct OpLens {}

impl Lens<State, text_line::State> for OpLens {
    fn with<V, F: FnOnce(&text_line::State) -> V>(&self, data: &State, f: F) -> V {
        f(&text_line::State::new(
            data.op.clone(),
            PLANT_FONT_SIZE,
            Color::BLACK,
        ))
    }

    fn with_mut<V, F: FnOnce(&mut text_line::State) -> V>(&self, data: &mut State, f: F) -> V {
        let mut lens = text_line::State::new(data.op.clone(), PLANT_FONT_SIZE, Color::BLACK);
        let result = f(&mut lens);
        data.op = lens.text;
        result
    }
}
