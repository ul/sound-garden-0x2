use crate::{commands::*, theme::*};
use audio_ops::pure::linlin;
use druid::{
    kurbo::BezPath,
    piet::{FontFamily, PietText, Text, TextLayout, TextLayoutBuilder},
    Event, Point, RenderContext, Size,
};
use std::collections::VecDeque;

pub struct Widget {
    font: Option<FontFamily>,
    grid_unit: Option<Size>,
    values: VecDeque<f64>,
    min: f64,
    max: f64,
}

#[derive(Clone, druid::Data, Default)]
pub struct Data {
    pub zoom: i16,
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

impl druid::Widget<Data> for Widget {
    fn event(
        &mut self,
        ctx: &mut druid::EventCtx,
        event: &druid::Event,
        _data: &mut Data,
        _env: &druid::Env,
    ) {
        match event {
            Event::Command(cmd) if cmd.is(OSCILLOSCOPE) => {
                self.values.push_back(cmd.get_unchecked(OSCILLOSCOPE)[0]);
                ctx.request_paint();
            }
            _ => {}
        }
    }

    fn lifecycle(
        &mut self,
        _ctx: &mut druid::LifeCycleCtx,
        _event: &druid::LifeCycle,
        _data: &Data,
        _env: &druid::Env,
    ) {
    }

    fn update(
        &mut self,
        ctx: &mut druid::UpdateCtx,
        _old_data: &Data,
        _data: &Data,
        _env: &druid::Env,
    ) {
        ctx.request_paint();
    }

    fn layout(
        &mut self,
        _ctx: &mut druid::LayoutCtx,
        bc: &druid::BoxConstraints,
        _data: &Data,
        _env: &druid::Env,
    ) -> druid::Size {
        bc.max()
    }

    fn paint(&mut self, ctx: &mut druid::PaintCtx, data: &Data, _env: &druid::Env) {
        let size = ctx.size();

        ctx.fill(size.to_rect(), &OSCILLOSCOPE_BACKGROUND_COLOR);

        let zoom = data.zoom + data.zoom.signum();

        let max_len = size.width as usize * if zoom >= 0 { 1 } else { -zoom as _ };

        if self.values.len() > max_len {
            self.values.drain(..(self.values.len() - max_len));
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

        let screen_step = if zoom > 0 { zoom as _ } else { 1 };
        let values_step = if zoom < 0 { -zoom as _ } else { 1 };
        let values_width = values_step * size.width as usize / screen_step;
        for (x, &y) in (0..(size.width as usize)).step_by(screen_step).zip(
            self.values
                .iter()
                .rev()
                .take(values_width)
                .rev()
                .step_by(values_step),
        ) {
            let y = linlin(y, max, min, 16.0, size.height - 16.0);
            path.line_to(Point::new(x as f64, y));
        }

        ctx.stroke(path, &OSCILLOSCOPE_FOREGROUND_COLOR, 0.75);

        let grid_unit = self.get_grid_unit(&mut ctx.text());
        let font = self.get_font(&mut ctx.text());
        let layout = ctx
            .text()
            .new_text_layout(format!("{}", max))
            .font(font.clone(), OSCILLOSCOPE_FONT_SIZE)
            .text_color(OSCILLOSCOPE_FOREGROUND_COLOR)
            .build()
            .unwrap();
        ctx.draw_text(&layout, Point::new(0.0, 0.0));
        let layout = ctx
            .text()
            .new_text_layout(format!("{}", min))
            .font(font.clone(), OSCILLOSCOPE_FONT_SIZE)
            .text_color(OSCILLOSCOPE_FOREGROUND_COLOR)
            .build()
            .unwrap();
        ctx.draw_text(&layout, Point::new(0.0, size.height - grid_unit.height));
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
                .new_text_layout("Q")
                .font(font.clone(), OSCILLOSCOPE_FONT_SIZE)
                .text_color(FOREGROUND_COLOR)
                .build()
                .unwrap();
            // self.grid_unit = Some(Size::new(
            //     layout.size().width,
            //     layout.line_metric(0).unwrap().height,
            // ));
            self.grid_unit = Some(layout.size())
        }
        self.grid_unit.unwrap()
    }
    fn get_font(&mut self, text: &mut PietText) -> &FontFamily {
        if self.font.is_none() {
            self.font = Some(text.font_family(FONT_NAME).unwrap_or(FontFamily::MONOSPACE));
        }
        self.font.as_ref().unwrap()
    }
}
