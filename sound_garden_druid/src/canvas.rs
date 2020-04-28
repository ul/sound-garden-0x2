use crate::{commands::*, types::*};
use druid::{
    piet::{FontBuilder, Text, TextLayoutBuilder},
    BoxConstraints, Color, Env, Event, EventCtx, KeyCode, LayoutCtx, LifeCycle, LifeCycleCtx,
    PaintCtx, Point, Rect, RenderContext, Size, UpdateCtx,
};
use std::sync::Arc;

// TODO Move those constants to Data or Env.
const FONT_NAME: &str = "IBM Plex Mono";
const FONT_SIZE: f64 = 20.0;
const BACKGROUND_COLOR: Color = Color::WHITE;
const DEFAULT_NODE_COLOR: Color = Color::rgb8(0x33, 0x33, 0x33);
const DRAFT_NODE_COLOR: Color = Color::rgb8(0xff, 0x00, 0x00);
const GRID_UNIT: Size = Size::new(12.0, 26.0);

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
    fn event(&mut self, ctx: &mut EventCtx, event: &Event, data: &mut Data, _env: &Env) {
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
                            ctx.submit_command(new_undo_group(), None)
                        }
                        _ => {}
                    },
                    Mode::Insert => match event {
                        _ if event.key_code == KeyCode::Escape
                            || event.key_code == KeyCode::Return =>
                        {
                            self.mode = Mode::Normal;
                        }
                        _ => {
                            if event.key_code == KeyCode::Backspace {
                                self.cursor.position.x -= 1.0;
                            }

                            if event.key_code == KeyCode::ArrowLeft {
                                self.cursor.position.x -= 1.0;
                            } else if event.key_code == KeyCode::ArrowDown {
                                self.cursor.position.y += 1.0;
                            } else if event.key_code == KeyCode::ArrowUp {
                                self.cursor.position.y -= 1.0;
                            } else if event.key_code == KeyCode::ArrowRight
                                || event.key_code == KeyCode::Space
                            {
                                self.cursor.position.x += 1.0;
                            } else if let Some((node, index)) = self.node_under_cursor(data) {
                                if event.key_code == KeyCode::Backspace {
                                    ctx.submit_command(
                                        node_delete_char(NodeDeleteChar { id: node.id, index }),
                                        None,
                                    );
                                } else if let Some(text) = event.text() {
                                    ctx.submit_command(
                                        node_insert_text(NodeInsertText {
                                            id: node.id,
                                            text: text.to_string(),
                                            index,
                                        }),
                                        None,
                                    );
                                    self.cursor.position.x += text.chars().count() as f64;
                                }
                            } else if event.key_code == KeyCode::Backspace {
                            } else if let Some(text) = event.text() {
                                ctx.submit_command(
                                    create_node(CreateNode {
                                        position: self.cursor.position,
                                        text: text.to_string(),
                                    }),
                                    None,
                                );
                                self.cursor.position.x += text.chars().count() as f64;
                            }
                        }
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

        // Draw a cursor.
        match self.mode {
            Mode::Normal => {
                ctx.blurred_rect(
                    Rect::from((
                        Point::new(
                            self.cursor.position.x * GRID_UNIT.width,
                            (self.cursor.position.y + 0.25) * GRID_UNIT.height,
                        ),
                        GRID_UNIT,
                    )),
                    2.0,
                    &DEFAULT_NODE_COLOR,
                );
            }
            Mode::Insert => {
                ctx.blurred_rect(
                    Rect::from((
                        Point::new(
                            self.cursor.position.x * GRID_UNIT.width,
                            (self.cursor.position.y + 1.1) * GRID_UNIT.height,
                        ),
                        Size::new(GRID_UNIT.width, 2.0),
                    )),
                    1.0,
                    &DEFAULT_NODE_COLOR,
                );
            }
        }

        // Draw nodes.
        // REVIEW Is it cached?
        let font = ctx
            .text()
            .new_font_by_name(FONT_NAME, FONT_SIZE)
            .build()
            .unwrap();
        // let layout = ctx
        //     .text()
        //     .new_text_layout(&font, "Q", f64::INFINITY)
        //     .build()
        //     .unwrap();
        // println!(
        //     "{}x{}",
        //     layout.width(),
        //     layout.line_metric(0).unwrap().height
        // );
        let node_under_cursor = self.node_under_cursor(data);
        for node in data.nodes.iter() {
            let layout = ctx
                .text()
                .new_text_layout(&font, &node.text, f64::INFINITY)
                .build()
                .unwrap();
            let color = if node.draft {
                DRAFT_NODE_COLOR
            } else {
                if node_under_cursor.is_some()
                    && node.id == node_under_cursor.as_ref().unwrap().0.id
                {
                    BACKGROUND_COLOR
                } else {
                    DEFAULT_NODE_COLOR
                }
            };
            ctx.draw_text(
                &layout,
                Point::new(
                    node.position.x * GRID_UNIT.width,
                    (node.position.y + 1.0) * GRID_UNIT.height,
                ),
                &color,
            );
        }
    }
}

impl Widget {
    fn node_under_cursor(&self, data: &Data) -> Option<(Node, usize)> {
        let cursor = &self.cursor.position;
        data.nodes.iter().find_map(|node| {
            let len = node.text.chars().count() as isize;
            let index = (cursor.x - node.position.x) as isize;
            // index <= len instead of strict inequality as we treat trailing space as a part of node.
            if node.position.y == cursor.y && 0 <= index && index <= len {
                Some((node.clone(), index as _))
            } else {
                None
            }
        })
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
