use crate::lens2::{Lens2, Lens2Wrap};
use crate::state;
use crate::ui::{constants::*, text_line, util::EventExt};
use druid::{
    kurbo::{Point, Rect, Size},
    piet::Color,
    BaseState, BoxConstraints, Command, Data, Env, Event, EventCtx, LayoutCtx, PaintCtx,
    TimerToken, UpdateCtx, WidgetPod,
};

pub struct Widget {
    name: WidgetPod<State, Lens2Wrap<text_line::State, NameLens, text_line::Widget>>,
    click_cnt: u8,
    dbl_click_timer: TimerToken,
}

#[derive(Clone, Data, Debug)]
pub struct State {
    pub ix: state::PlantIx,
    pub name: String,
}

impl druid::Widget<State> for Widget {
    fn event(&mut self, ctx: &mut EventCtx, event: &Event, data: &mut State, env: &Env) {
        match event {
            Event::MouseDown(e) => {
                if e.inside_widget(&ctx) {
                    self.click_cnt += 1;
                    if self.click_cnt > 1 {
                        self.name.event(
                            ctx,
                            &Event::Command(Command::from(text_line::EDIT)),
                            data,
                            env,
                        );
                        self.click_cnt = 0;
                    }
                    self.dbl_click_timer =
                        ctx.request_timer(std::time::Instant::now() + DOUBLE_CLICK_TIMEOUT);
                } else {
                    self.click_cnt = 0;
                }
            }
            Event::Timer(t) => {
                if *t == self.dbl_click_timer {
                    if self.click_cnt == 1 {
                        ctx.submit_command(cmd::zoom_to_plant(data.ix), None);
                    }
                    self.click_cnt = 0;
                }
            }
            _ => {}
        }
        self.name.event(ctx, event, data, env);
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
        Widget {
            name: WidgetPod::new(Lens2Wrap::new(text_line::Widget::new(), NameLens {})),
            click_cnt: 0,
            dbl_click_timer: TimerToken::INVALID,
        }
    }
}

impl State {
    pub fn new(ix: state::PlantIx, name: String) -> Self {
        State { ix, name }
    }
}

struct NameLens {}

impl Lens2<State, text_line::State> for NameLens {
    fn get<V, F: FnOnce(&text_line::State) -> V>(&self, data: &State, f: F) -> V {
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
