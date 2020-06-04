use crate::theme::*;
use audio_ops::pure::linlin;
use audio_vm::Frame;
use druid::{
    kurbo::BezPath,
    piet::{FontBuilder, PietFont, PietText, Text, TextLayout, TextLayoutBuilder},
    Point, RenderContext, Size,
};
use std::collections::VecDeque;

pub struct Widget {
    font: Option<PietFont>,
    grid_unit: Option<Size>,
    values: VecDeque<f64>,
    min: f64,
    max: f64,
}

impl Default for Widget {
    fn default() -> Self {
        Widget {
            font: None,
            grid_unit: None,
            values: VecDeque::new(),
            min: -1.0,
            max: 1.0,
        }
    }
}

impl druid::Widget<(usize, Frame)> for Widget {
    fn event(
        &mut self,
        _ctx: &mut druid::EventCtx,
        _event: &druid::Event,
        _data: &mut (usize, Frame),
        _env: &druid::Env,
    ) {
    }

    fn lifecycle(
        &mut self,
        _ctx: &mut druid::LifeCycleCtx,
        _event: &druid::LifeCycle,
        _data: &(usize, Frame),
        _env: &druid::Env,
    ) {
    }

    fn update(
        &mut self,
        ctx: &mut druid::UpdateCtx,
        _old_data: &(usize, Frame),
        _data: &(usize, Frame),
        _env: &druid::Env,
    ) {
        ctx.request_paint();
    }

    fn layout(
        &mut self,
        _ctx: &mut druid::LayoutCtx,
        bc: &druid::BoxConstraints,
        _data: &(usize, Frame),
        _env: &druid::Env,
    ) -> druid::Size {
        bc.max()
    }

    fn paint(&mut self, ctx: &mut druid::PaintCtx, data: &(usize, Frame), _env: &druid::Env) {
        let size = ctx.size();

        self.values.push_back(data.1[0]);
        if self.values.len() > size.width as usize {
            self.values
                .drain(..(self.values.len() - size.width as usize));
        }

        let mut min = self
            .values
            .iter()
            .min_by(|x, y| f64_cmp(x, y))
            .copied()
            .unwrap_or(self.min);
        let mut max = self
            .values
            .iter()
            .max_by(|x, y| f64_cmp(x, y))
            .copied()
            .unwrap_or(self.max);

        if min == max {
            min -= 1.0;
            max += 1.0;
        }

        self.min = 0.5 * (self.min + min);
        self.max = 0.5 * (self.max + max);

        let min = self.min;
        let max = self.max;

        let mut path = BezPath::new();
        path.move_to(Point::new(0.0, 0.5 * size.height));

        for (x, &y) in self.values.iter().enumerate() {
            let y = linlin(y, max, min, 0.0, size.height);
            path.line_to(Point::new(x as f64, y));
        }

        ctx.stroke(path, &OSCILLOSCOPE_COLOR.with_alpha(0.25), 1.0);

        let grid_unit = self.get_grid_unit(&mut ctx.text());
        let font = self.get_font(&mut ctx.text());
        let layout = ctx
            .text()
            .new_text_layout(font, &format!("{}", max), f64::INFINITY)
            .build()
            .unwrap();
        ctx.draw_text(
            &layout,
            Point::new(0.0, 0.8 * grid_unit.height),
            &OSCILLOSCOPE_COLOR,
        );
        let layout = ctx
            .text()
            .new_text_layout(font, &format!("{}", min), f64::INFINITY)
            .build()
            .unwrap();
        ctx.draw_text(&layout, Point::new(0.0, size.height), &OSCILLOSCOPE_COLOR);
    }
}

fn f64_cmp(x: &f64, y: &f64) -> std::cmp::Ordering {
    if x < y {
        std::cmp::Ordering::Less
    } else {
        std::cmp::Ordering::Greater
    }
}

impl Widget {
    fn get_grid_unit(&mut self, text: &mut PietText) -> Size {
        if self.grid_unit.is_none() {
            let font = self.get_font(text);
            let layout = text
                .new_text_layout(font, "Q", f64::INFINITY)
                .build()
                .unwrap();
            self.grid_unit = Some(Size::new(
                layout.width(),
                layout.line_metric(0).unwrap().height,
            ));
        }
        self.grid_unit.unwrap()
    }
    fn get_font(&mut self, text: &mut PietText) -> &PietFont {
        if self.font.is_none() {
            self.font = Some(
                text.new_font_by_name(FONT_NAME, OSCILLOSCOPE_FONT_SIZE)
                    .build()
                    .unwrap(),
            );
        }
        self.font.as_ref().unwrap()
    }
}
