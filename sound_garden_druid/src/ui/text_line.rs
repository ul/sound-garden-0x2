use crate::ui::constants::*;
use druid::{
    kurbo::{Point, Rect, Size},
    piet::{Color, FontBuilder, RenderContext, Text, TextLayout, TextLayoutBuilder, UnitPoint},
    BaseState, BoxConstraints, Command, Data, Env, Event, EventCtx, KeyCode, LayoutCtx, PaintCtx,
    Selector, UpdateCtx,
};
use piet_cairo::{CairoFont, CairoTextLayout};

pub const EDIT: Selector = Selector::new("SOUND_GARDEN.TEXT_LINE.EDIT");
pub const EDIT_END: Selector = Selector::new("SOUND_GARDEN.TEXT_LINE.EDIT_END");

pub struct Widget {
    font: Option<CairoFont>,
    layout: Option<CairoTextLayout>,
    uncommitted_text: Option<String>,
}

#[derive(Clone, Data, Debug)]
pub struct State {
    pub color: u32,
    pub font_size: f64,
    pub text: String,
}

impl druid::Widget<State> for Widget {
    fn event(&mut self, ctx: &mut EventCtx, event: &Event, data: &mut State, _env: &Env) {
        match event {
            Event::Command(Command { selector: EDIT, .. }) => {
                self.uncommitted_text = Some(String::new());
                self.layout = None;
                ctx.request_focus();
                ctx.set_handled();
                ctx.invalidate();
            }
            Event::FocusChanged(false) => {
                self.uncommitted_text = None;
                self.layout = None;
                ctx.submit_command(Command::from(EDIT_END), None);
                ctx.invalidate();
            }
            Event::KeyDown(e) => {
                match e.key_code {
                    KeyCode::Return => {
                        data.text = self.uncommitted_text.take().unwrap_or_default();
                        ctx.submit_command(Command::from(EDIT_END), None);
                    }
                    KeyCode::Escape => {
                        self.uncommitted_text = None;
                        ctx.submit_command(Command::from(EDIT_END), None);
                    }
                    KeyCode::Backspace => {
                        if let Some(text) = &mut self.uncommitted_text {
                            text.pop();
                        }
                    }
                    code if code.is_printable() => {
                        if let Some(text) = &mut self.uncommitted_text {
                            if let Some(t) = e.text() {
                                text.extend(t.chars());
                            }
                        }
                    }
                    _ => {}
                }
                self.layout = None;
                ctx.set_handled();
                ctx.invalidate();
            }
            _ => {}
        }
    }

    fn update(&mut self, ctx: &mut UpdateCtx, old_data: Option<&State>, data: &State, _env: &Env) {
        if let Some(old_data) = old_data {
            if data.font_size != old_data.font_size {
                self.font = None;
                self.layout = None;
            } else if data.text != old_data.text {
                self.layout = None;
            }
        }
        ctx.invalidate();
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        _bc: &BoxConstraints,
        data: &State,
        _env: &Env,
    ) -> Size {
        let t = ctx.text();
        if self.font.is_none() {
            self.font = Some(
                t.new_font_by_name(FONT_NAME, data.font_size)
                    .build()
                    .unwrap(),
            );
        }
        if self.layout.is_none() {
            self.layout = Some(
                t.new_text_layout(
                    self.font.as_ref().unwrap(),
                    self.uncommitted_text.as_ref().unwrap_or(&data.text),
                )
                .build()
                .unwrap(),
            );
        }
        let layout = &self.layout.as_ref().unwrap();
        // NOTE Comment below is copied from druid::widget::Label
        // This magical 1.2 constant helps center the text vertically in the rect it's given.
        Size::new(
            layout.width().max(data.font_size / 2.),
            data.font_size * 1.2,
        )
    }

    fn paint(&mut self, ctx: &mut PaintCtx, base_state: &BaseState, data: &State, _env: &Env) {
        let layout = self.layout.as_ref().unwrap();

        // Find the origin for the text
        let origin = UnitPoint::LEFT.resolve(Rect::from_origin_size(
            Point::ORIGIN,
            Size::new(
                base_state.size().width,
                base_state.size().height + (data.font_size * 1.2) / 2.,
            ),
        ));

        ctx.draw_text(&layout, origin, &Color::from_rgba32_u32(data.color));

        if self.uncommitted_text.is_some() {
            ctx.stroke(
                Rect::from_origin_size(
                    Point::ORIGIN,
                    Size::new(
                        layout.width().max(data.font_size / 2.),
                        data.font_size * 1.2,
                    ),
                ),
                &Color::rgb(1.0, 0.0, 0.0),
                1.0,
            );
        }
    }
}

impl Widget {
    pub fn new() -> Self {
        Widget {
            font: None,
            layout: None,
            uncommitted_text: None,
        }
    }
}

impl State {
    pub fn new(text: String, font_size: f64, color: Color) -> Self {
        State {
            color: color.as_rgba_u32(),
            font_size,
            text,
        }
    }
}
