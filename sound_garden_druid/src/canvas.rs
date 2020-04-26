use crate::types::*;
use druid::{
    piet::{FontBuilder, Text, TextLayoutBuilder},
    BoxConstraints, Color, Env, Event, EventCtx, KeyCode, LayoutCtx, LifeCycle, LifeCycleCtx,
    PaintCtx, Point, Rect, RenderContext, Size, UpdateCtx,
};
use std::sync::Arc;

// TODO Move those constants to druid::Data or Env.
const FONT_NAME: &str = "IBM Plex Mono";
const FONT_SIZE: f64 = 16.0;
const BACKGROUND_COLOR: Color = Color::WHITE;
const DEFAULT_NODE_COLOR: Color = Color::BLACK;
const DRAFT_NODE_COLOR: Color = Color::rgb8(0xff, 0x00, 0x00);
const GRID_UNIT: Size = Size::new(FONT_SIZE, FONT_SIZE);

#[derive(Default)]
pub struct Widget {
    cursor: Cursor,
    mode: Mode,
}

#[derive(Clone, druid::Data, Default)]
pub struct Data {
    pub nodes: Arc<Vec<Node>>,
}

#[derive(Clone, druid::Data, Default)]
pub struct Node {
    pub id: Id,
    pub draft: bool,
    /// In grid units, not pixels.
    pub position: Point,
    pub text: String,
}

impl druid::Widget<Data> for Widget {
    fn event(&mut self, ctx: &mut EventCtx, event: &Event, _data: &mut Data, _env: &Env) {
        match event {
            Event::WindowConnected => {
                ctx.request_focus();
            }
            Event::KeyDown(event) => {
                match self.mode {
                    Mode::Normal => match event.text() {
                        Some("h") => {
                            self.cursor.position.x -= 1.0;
                        }
                        Some("j") => {
                            self.cursor.position.y += 1.0;
                        }
                        Some("k") => {
                            self.cursor.position.y -= 1.0;
                        }
                        Some("l") => {
                            self.cursor.position.x += 1.0;
                        }
                        Some("i") => {
                            self.mode = Mode::Insert;
                        }
                        _ => {}
                    },
                    Mode::Insert => match event {
                        _ if event.key_code == KeyCode::Escape
                            || event.key_code == KeyCode::Return =>
                        {
                            self.mode = Mode::Normal;
                        }
                        _ => {}
                    },
                }
                ctx.request_paint();
            }
            Event::MouseDown(event) => {
                self.cursor.position.x = (event.pos.x / GRID_UNIT.width).round();
                self.cursor.position.y = (event.pos.y / GRID_UNIT.height).round();
                ctx.request_paint();
            }
            _ => {}
        }
    }

    fn lifecycle(&mut self, _ctx: &mut LifeCycleCtx, _event: &LifeCycle, _data: &Data, _env: &Env) {
        //
    }

    fn update(&mut self, ctx: &mut UpdateCtx, _old_data: &Data, _data: &Data, _env: &Env) {
        ctx.request_paint();
    }

    fn layout(
        &mut self,
        _ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        _data: &Data,
        _env: &Env,
    ) -> Size {
        bc.max()
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &Data, _env: &Env) {
        let size = ctx.size();

        // Clean.
        let frame = Rect::from_origin_size(Point::ORIGIN, size);
        ctx.fill(frame, &BACKGROUND_COLOR);

        // Draw nodes.
        // REVIEW Is it cached?
        let font = ctx
            .text()
            .new_font_by_name(FONT_NAME, FONT_SIZE)
            .build()
            .unwrap();
        for node in data.nodes.iter() {
            let layout = ctx
                .text()
                .new_text_layout(&font, &node.text, f64::INFINITY)
                .build()
                .unwrap();
            let color = if node.draft {
                DRAFT_NODE_COLOR
            } else {
                DEFAULT_NODE_COLOR
            };
            ctx.draw_text(
                &layout,
                Point::new(
                    node.position.x * GRID_UNIT.width,
                    node.position.y * GRID_UNIT.height,
                ),
                &color,
            );
        }

        // Draw a cursor.
        match self.mode {
            Mode::Normal => {
                ctx.stroke(
                    Rect::from((
                        Point::new(
                            self.cursor.position.x * GRID_UNIT.width,
                            self.cursor.position.y * GRID_UNIT.height,
                        ),
                        GRID_UNIT,
                    )),
                    &Color::rgba(0x00, 0x00, 0x00, 0xa0),
                    1.0,
                );
            }
            Mode::Insert => {
                ctx.stroke(
                    Rect::from((
                        Point::new(
                            self.cursor.position.x * GRID_UNIT.width,
                            (self.cursor.position.y + 1.0) * GRID_UNIT.height,
                        ),
                        Size::new(GRID_UNIT.width, 2.0),
                    )),
                    &Color::rgba(0x00, 0x00, 0x00, 0xa0),
                    1.0,
                );
            }
        }
    }
}

#[derive(Default)]
struct Cursor {
    position: Point,
}

#[derive(Clone, Copy)]
enum Mode {
    Normal,
    Insert,
}

impl Default for Mode {
    fn default() -> Self {
        Mode::Normal
    }
}
