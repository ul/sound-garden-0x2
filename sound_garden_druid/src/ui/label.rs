use druid::{
    kurbo::{Point, Rect, Size},
    piet::{Color, FontBuilder, RenderContext, Text, TextLayout, TextLayoutBuilder, UnitPoint},
    BaseState, BoxConstraints, Env, Event, EventCtx, LayoutCtx, PaintCtx, UpdateCtx, Widget,
};
use piet_cairo::{CairoFont, CairoTextLayout};

pub struct Label {
    color: Color,
    font: Option<CairoFont>,
    font_size: f64,
    layout: Option<CairoTextLayout>,
    text: String,
}

type State = ();

impl Widget<()> for Label {
    fn paint(&mut self, ctx: &mut PaintCtx, _base_state: &BaseState, _data: &State, _env: &Env) {
        let layout = self.layout.as_ref().unwrap();

        // Find the origin for the text
        let origin = UnitPoint::CENTER.resolve(Rect::from_origin_size(
            Point::ORIGIN,
            Size::new(-layout.width() / 2., (self.font_size * 1.2) / 2.),
        ));

        ctx.draw_text(layout, origin, &self.color);
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        _bc: &BoxConstraints,
        _data: &State,
        _env: &Env,
    ) -> Size {
        let t = ctx.text();
        if self.font.is_none() {
            self.font = Some(t.new_font_by_name("Agave", self.font_size).build().unwrap());
        }
        let font = self.font.as_ref().unwrap();
        if self.layout.is_none() {
            self.layout = Some(t.new_text_layout(font, &self.text).build().unwrap());
        }
        let layout = self.layout.as_ref().unwrap();
        // NOTE Comment below is copied from druid::widget::Label
        // This magical 1.2 constant helps center the text vertically in the rect it's given.
        Size::new(layout.width(), self.font_size * 1.2)
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

impl Label {
    pub fn new(text: String, font_size: f64, color: Color) -> Self {
        Label {
            color,
            font: None,
            font_size,
            layout: None,
            text,
        }
    }
}
