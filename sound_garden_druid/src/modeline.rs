use crate::{commands::*, theme::*};
use audio_program::get_help;
use druid::{
    kurbo::{BezPath, Line},
    piet::{FontFamily, PietText, Text, TextLayout, TextLayoutBuilder},
    Event, Point, Rect, RenderContext, Size, TimerToken,
};
use sound_garden_types::*;
use std::{collections::HashMap, time::Duration};

pub struct Widget {
    font: Option<FontFamily>,
    grid_unit: Option<Size>,
    op_help: HashMap<String, String>,
    /// Is record marker animation is in visible phase?
    record_phase: bool,
    record_timer: TimerToken,
}

#[derive(Clone, druid::Data, Default)]
pub struct Data {
    pub draft: bool,
    pub mode: Mode,
    pub op_at_cursor: Option<String>,
    pub play: bool,
    pub record: bool,
}

impl Default for Widget {
    fn default() -> Self {
        Widget {
            font: None,
            grid_unit: None,
            op_help: get_help(),
            record_phase: true,
            record_timer: TimerToken::INVALID,
        }
    }
}

impl druid::Widget<Data> for Widget {
    fn event(
        &mut self,
        ctx: &mut druid::EventCtx,
        event: &druid::Event,
        data: &mut Data,
        _env: &druid::Env,
    ) {
        match event {
            Event::Timer(token) if token == &self.record_timer => {
                self.record_phase = !self.record_phase;
                if data.record {
                    self.record_timer = ctx.request_timer(Duration::from_secs(1));
                }
                ctx.request_paint();
            }
            Event::MouseDown(e) => {
                let play_button = Rect::new(11.0, 5.0, 31.0, 23.0);
                if play_button.contains(e.pos) {
                    ctx.submit_command(PLAY_PAUSE);
                }
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
        old_data: &Data,
        data: &Data,
        _env: &druid::Env,
    ) {
        use druid::Data;
        if !data.same(old_data) {
            if data.record && data.record != old_data.record {
                self.record_phase = true;
                self.record_timer = ctx.request_timer(Duration::from_secs(1));
            }
            ctx.request_paint();
        }
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
        let frame = size.to_rect();

        // Clean.
        ctx.fill(frame, &BACKGROUND_COLOR);
        // Border.
        let color = match data.mode {
            Mode::Normal => {
                if data.draft {
                    MODELINE_DRAFT_COLOR
                } else {
                    MODELINE_NORMAL_COLOR
                }
            }
            Mode::Insert => MODELINE_INSERT_COLOR,
        };
        ctx.stroke(
            Line::new(
                Point::new(frame.min_x(), frame.min_y() + 2.0),
                Point::new(frame.max_x(), frame.min_y() + 2.0),
            ),
            &color,
            4.0,
        );
        // TODO Extract drawing to helpers, use transform.
        // Play/pause + record.
        let color = if data.record && self.record_phase {
            MODELINE_RECORD_COLOR
        } else {
            FOREGROUND_COLOR
        }
        .with_alpha(0.66);
        // Play/pause.
        if data.play {
            ctx.fill(&Rect::new(15.0, 7.0, 19.0, 21.0), &color);
            ctx.fill(&Rect::new(23.0, 7.0, 27.0, 21.0), &color);
        } else {
            let mut path = BezPath::new();
            path.move_to(Point::new(15.0, 7.0));
            path.line_to(Point::new(27.0, 14.0));
            path.line_to(Point::new(15.0, 21.0));
            path.close_path();
            ctx.fill(&path, &color);
        }

        // Op help.
        if let Some(help) = data
            .op_at_cursor
            .as_ref()
            .and_then(|op| self.op_help.get(op))
            .cloned()
        {
            let grid_unit = self.get_grid_unit(&mut ctx.text());
            let font = self.get_font(&mut ctx.text());
            let layout = ctx
                .text()
                .new_text_layout(help)
                .font(font.clone(), MODELINE_FONT_SIZE)
                .text_color(FOREGROUND_COLOR)
                .build()
                .unwrap();
            ctx.draw_text(&layout, Point::new(35.0, 0.4 * grid_unit.height));
        }
    }
}

impl Widget {
    fn get_grid_unit(&mut self, text: &mut PietText) -> Size {
        if self.grid_unit.is_none() {
            let font = self.get_font(text);
            let layout = text
                .new_text_layout("Q")
                .font(font.clone(), MODELINE_FONT_SIZE)
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
