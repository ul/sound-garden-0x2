use crate::{commands::*, types::*};
use druid::{
    piet::{CairoFont, FontBuilder, PietText, Text, TextLayout, TextLayoutBuilder},
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

pub struct Widget {
    mode: Mode,
    grid_unit: Option<Size>,
    font: Option<CairoFont>,
}

#[derive(Clone, druid::Data, Default)]
pub struct Data {
    pub cursor: Cursor,
    pub nodes: Arc<Vec<Node>>,
    pub draft_nodes: Arc<Vec<Id>>,
}

#[derive(Clone, druid::Data, Default)]
pub struct Cursor {
    pub position: Point,
}

impl Default for Widget {
    fn default() -> Self {
        Widget {
            mode: Default::default(),
            grid_unit: Default::default(),
            font: Default::default(),
        }
    }
}

/*

TODO commands in normal mode:

/--------------------------------------\
| '      | Commit without migration.   |
| Alt+h  | Move node left.             |
| Alt+j  | Move node down.             |
| Alt+k  | Move node up.               |
| Alt+l  | Move node right.            |
| J      | Move node and after down.   |
| K      | Move node and before up.    |
| H      | Move line up.               |
| L      | Move line down.             |
| ,      | Move right of line left.    |
| .      | Move right of line right.   |
| <      | Move left of line left.     |
| >      | Move left of line right.    |
| =      | Cycle up / Increase by 1.   |
| -      | Cycle down / Decrease by 1. |
| /      | List ops.                   |
| ?      | Help (this screen).         |
\--------------------------------------/

*/

impl druid::Widget<Data> for Widget {
    fn event(&mut self, ctx: &mut EventCtx, event: &Event, data: &mut Data, _env: &Env) {
        match event {
            Event::WindowConnected => {
                ctx.request_focus();
            }
            Event::KeyDown(event) => {
                match self.mode {
                    Mode::Normal => match event {
                        _ if HotKey::new(None, KeyCode::KeyH).matches(event)
                            || HotKey::new(None, KeyCode::ArrowLeft).matches(event)
                            || HotKey::new(None, KeyCode::Backspace).matches(event) =>
                        {
                            data.cursor.position.x -= 1.0;
                        }
                        _ if HotKey::new(None, KeyCode::KeyJ).matches(event)
                            || HotKey::new(None, KeyCode::ArrowDown).matches(event) =>
                        {
                            data.cursor.position.y += 1.0;
                        }
                        _ if HotKey::new(None, KeyCode::KeyK).matches(event)
                            || HotKey::new(None, KeyCode::ArrowUp).matches(event) =>
                        {
                            data.cursor.position.y -= 1.0;
                        }
                        _ if HotKey::new(None, KeyCode::KeyL).matches(event)
                            || HotKey::new(None, KeyCode::ArrowRight).matches(event) =>
                        {
                            data.cursor.position.x += 1.0;
                        }
                        _ if HotKey::new(None, KeyCode::KeyI).matches(event) => {
                            self.insert_mode(ctx);
                        }
                        _ if HotKey::new(SysMods::Shift, KeyCode::KeyI).matches(event) => {
                            self.insert_mode(ctx);
                            ctx.submit_command(splash(), None);
                        }
                        _ if HotKey::new(None, KeyCode::KeyA).matches(event) => {
                            data.cursor.position.x += 1.0;
                            self.insert_mode(ctx);
                        }
                        _ if HotKey::new(None, KeyCode::KeyC).matches(event) => {
                            self.insert_mode(ctx);
                            ctx.submit_command(cut_node(), None);
                        }
                        _ if HotKey::new(None, KeyCode::KeyD).matches(event) => {
                            ctx.submit_command(delete_node(), None);
                        }
                        _ if HotKey::new(SysMods::Shift, KeyCode::KeyD).matches(event) => {
                            ctx.submit_command(delete_line(), None);
                        }
                        _ if HotKey::new(None, KeyCode::Return).matches(event) => {
                            ctx.submit_command(commit_program(), None);
                        }
                        _ if HotKey::new(None, KeyCode::Backslash).matches(event) => {
                            ctx.submit_command(play_pause(), None);
                        }
                        _ if HotKey::new(None, KeyCode::KeyR).matches(event) => {
                            ctx.submit_command(toggle_record(), None);
                        }
                        _ if HotKey::new(None, KeyCode::KeyU).matches(event) => {
                            ctx.submit_command(undo(), None);
                        }
                        _ if HotKey::new(SysMods::Shift, KeyCode::KeyU).matches(event) => {
                            ctx.submit_command(redo(), None);
                        }
                        _ => {}
                    },
                    Mode::Insert => match event {
                        _ if HotKey::new(None, KeyCode::Escape).matches(event)
                            || HotKey::new(None, KeyCode::Return).matches(event) =>
                        {
                            self.normal_mode(ctx);
                        }
                        _ if HotKey::new(None, KeyCode::ArrowLeft).matches(event) => {
                            data.cursor.position.x -= 1.0;
                        }
                        _ if HotKey::new(None, KeyCode::ArrowDown).matches(event) => {
                            data.cursor.position.y += 1.0;
                        }
                        _ if HotKey::new(None, KeyCode::ArrowUp).matches(event) => {
                            data.cursor.position.y -= 1.0;
                        }
                        _ if HotKey::new(None, KeyCode::ArrowRight).matches(event)
                            || HotKey::new(None, KeyCode::Space).matches(event) =>
                        {
                            data.cursor.position.x += 1.0;
                        }
                        _ if HotKey::new(None, KeyCode::Backspace).matches(event) => {
                            data.cursor.position.x -= 1.0;
                            ctx.submit_command(node_delete_char(), None);
                        }
                        _ if event.key_code.is_printable() => {
                            if let Some(text) = event.text() {
                                ctx.submit_command(
                                    node_insert_text(NodeInsertText {
                                        text: text.to_string(),
                                    }),
                                    None,
                                );
                            }
                        }
                        _ => {}
                    },
                }
                ctx.request_paint();
            }
            Event::MouseDown(event) => {
                if let Some(grid_unit) = self.grid_unit {
                    data.cursor.position.x = (event.pos.x / grid_unit.width).round();
                    data.cursor.position.y = (event.pos.y / grid_unit.height).round();
                }
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
        let grid_unit = self.get_grid_unit(ctx.text());

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
                            data.cursor.position.x * grid_unit.width,
                            (data.cursor.position.y + 0.25) * grid_unit.height,
                        ),
                        grid_unit,
                    )),
                    1.0,
                    &DEFAULT_NODE_COLOR.with_alpha(CURSOR_ALPHA),
                );
            }
            Mode::Insert => {
                ctx.blurred_rect(
                    Rect::from((
                        Point::new(
                            data.cursor.position.x * grid_unit.width,
                            (data.cursor.position.y + 1.1) * grid_unit.height,
                        ),
                        Size::new(grid_unit.width, 2.0),
                    )),
                    1.0,
                    &DEFAULT_NODE_COLOR.with_alpha(CURSOR_ALPHA),
                );
            }
        }

        // Draw nodes.
        for node in data.nodes.iter() {
            let font = self.get_font(ctx.text());
            let layout = ctx
                .text()
                .new_text_layout(font, &node.text, f64::INFINITY)
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
                    node.position.x * grid_unit.width,
                    (node.position.y + 1.0) * grid_unit.height,
                ),
                &color,
            );
        }
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

    fn get_font(&mut self, text: &mut PietText) -> &CairoFont {
        if self.font.is_none() {
            self.font = Some(text.new_font_by_name(FONT_NAME, FONT_SIZE).build().unwrap());
        }
        self.font.as_ref().unwrap()
    }

    fn insert_mode(&mut self, ctx: &mut EventCtx) {
        self.mode = Mode::Insert;
        ctx.submit_command(new_undo_group(), None);
    }

    fn normal_mode(&mut self, ctx: &mut EventCtx) {
        self.mode = Mode::Normal;
        ctx.submit_command(new_undo_group(), None);
    }
}

impl Data {
    pub fn node_at_cursor(&self) -> Option<(Node, usize)> {
        let cursor = self.cursor.position;
        self.nodes.iter().find_map(|node| {
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
