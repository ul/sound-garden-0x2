use crate::{commands::*, types::*};
use druid::{
    piet::{FontBuilder, Text, TextLayoutBuilder},
    BoxConstraints, Color, Env, Event, EventCtx, HotKey, KeyCode, LayoutCtx, LifeCycle,
    LifeCycleCtx, PaintCtx, Point, Rect, RenderContext, Size, SysMods, UpdateCtx,
};
use std::sync::Arc;

// TODO Move those constants to Data or Env.
const FONT_NAME: &str = "IBM Plex Mono";
const FONT_SIZE: f64 = 20.0;
const BACKGROUND_COLOR: Color = Color::WHITE;
const CURSOR_ALPHA: f64 = 0.33;
const DEFAULT_NODE_COLOR: Color = Color::rgb8(0x20, 0x20, 0x20);
// const DRAFT_NODE_COLOR: Color = Color::rgb8(0xff, 0x00, 0x00);
// TODO Calculate from the font settings automatically.
// Current values are obtained with
// ```
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
// ```
const GRID_UNIT: Size = Size::new(12.0, 26.0);

pub struct Widget {
    cursor: Cursor,
    mode: Mode,
}

#[derive(Clone, druid::Data, Default)]
pub struct Data {
    pub nodes: Arc<Vec<Node>>,
    pub draft_nodes: Arc<Vec<Id>>,
}

#[derive(Clone, druid::Data, Default)]
pub struct Node {
    pub id: Id,
    /// In grid units, not pixels.
    pub position: Point,
    pub text: String,
}

impl Default for Widget {
    fn default() -> Self {
        Widget {
            cursor: Default::default(),
            mode: Default::default(),
        }
    }
}

impl druid::Widget<Data> for Widget {
    fn event(&mut self, ctx: &mut EventCtx, event: &Event, data: &mut Data, _env: &Env) {
        match event {
            Event::WindowConnected => {
                ctx.request_focus();
            }
            Event::KeyDown(event) => {
                match self.mode {
                    Mode::Normal => match event {
                        _ if HotKey::new(None, KeyCode::KeyH).matches(event) => {
                            self.cursor.position.x -= 1.0;
                        }
                        _ if HotKey::new(None, KeyCode::KeyJ).matches(event) => {
                            self.cursor.position.y += 1.0;
                        }
                        _ if HotKey::new(None, KeyCode::KeyK).matches(event) => {
                            self.cursor.position.y -= 1.0;
                        }
                        _ if HotKey::new(None, KeyCode::KeyL).matches(event) => {
                            self.cursor.position.x += 1.0;
                        }
                        _ if HotKey::new(None, KeyCode::KeyI).matches(event) => {
                            self.mode = Mode::Insert;
                            ctx.submit_command(new_undo_group(), None)
                        }
                        _ if HotKey::new(None, KeyCode::Return).matches(event) => {
                            ctx.submit_command(commit_program(), None)
                        }
                        _ if HotKey::new(None, KeyCode::Backslash).matches(event) => {
                            ctx.submit_command(play_pause(), None)
                        }
                        _ if HotKey::new(None, KeyCode::KeyR).matches(event) => {
                            ctx.submit_command(toggle_record(), None)
                        }
                        _ if HotKey::new(None, KeyCode::KeyU).matches(event) => {
                            ctx.submit_command(undo(), None)
                        }
                        _ if HotKey::new(SysMods::Shift, KeyCode::KeyU).matches(event) => {
                            ctx.submit_command(redo(), None)
                        }
                        _ => {}
                    },
                    Mode::Insert => {
                        if HotKey::new(None, KeyCode::Backspace).matches(event) {
                            self.cursor.position.x -= 1.0;
                        }
                        match event {
                            _ if HotKey::new(None, KeyCode::Escape).matches(event)
                                || HotKey::new(None, KeyCode::Return).matches(event) =>
                            {
                                self.mode = Mode::Normal;
                            }
                            _ if HotKey::new(None, KeyCode::ArrowLeft).matches(event) => {
                                self.cursor.position.x -= 1.0;
                            }
                            _ if HotKey::new(None, KeyCode::ArrowDown).matches(event) => {
                                self.cursor.position.y += 1.0;
                            }
                            _ if HotKey::new(None, KeyCode::ArrowUp).matches(event) => {
                                self.cursor.position.y -= 1.0;
                            }
                            _ if HotKey::new(None, KeyCode::ArrowRight).matches(event)
                                || HotKey::new(None, KeyCode::Space).matches(event) =>
                            {
                                self.cursor.position.x += 1.0;
                            }
                            _ => {
                                if let Some((node, index)) = self.node_under_cursor(data) {
                                    if event.key_code == KeyCode::Backspace {
                                        ctx.submit_command(
                                            node_delete_char(NodeDeleteChar { id: node.id, index }),
                                            None,
                                        );
                                    } else if event.key_code.is_printable() {
                                        let text = event.text().unwrap();
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
                                } else if event.key_code.is_printable() {
                                    let text = event.text().unwrap();
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
                        }
                    }
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
                    1.0,
                    &DEFAULT_NODE_COLOR.with_alpha(CURSOR_ALPHA),
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
                    &DEFAULT_NODE_COLOR.with_alpha(CURSOR_ALPHA),
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
        for node in data.nodes.iter() {
            let layout = ctx
                .text()
                .new_text_layout(&font, &node.text, f64::INFINITY)
                .build()
                .unwrap();
            // Draft check requires extra work to make it reliable when jamming.
            // let color = if data.draft_nodes.contains(&node.id) {
            //     DRAFT_NODE_COLOR
            // } else {
            //     DEFAULT_NODE_COLOR
            // };
            let color = DEFAULT_NODE_COLOR;
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
