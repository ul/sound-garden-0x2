use crate::{commands::*, types::*};
use druid::{
    piet::{FontBuilder, PietFont, PietText, Text, TextLayout, TextLayoutBuilder},
    BoxConstraints, Color, Command, Env, Event, EventCtx, HotKey, KeyCode, LayoutCtx, LifeCycle,
    LifeCycleCtx, PaintCtx, Point, RawMods, Rect, RenderContext, Size, SysMods, UpdateCtx, Vec2,
};
use std::sync::Arc;

// TODO Move these constants to Data or Env.
const FONT_NAME: &str = "IBM Plex Mono";
const FONT_SIZE: f64 = 20.0;
const BACKGROUND_COLOR: Color = Color::WHITE;
const CURSOR_ALPHA: f64 = 0.33;
const DEFAULT_NODE_COLOR: Color = Color::rgb8(0x20, 0x20, 0x20);
const DRAFT_NODE_COLOR: Color = Color::rgb8(0xff, 0x00, 0x00);

pub struct Widget {
    mode: Mode,
    grid_unit: Option<Size>,
    font: Option<PietFont>,
}

#[derive(Clone, druid::Data, Default)]
pub struct Data {
    pub cursor: Cursor,
    pub nodes: Arc<Vec<Node>>,
    pub draft_nodes: Arc<Vec<Id>>,
    /// Workspace is draft besides of edited nodes (usually deleted nodes).
    pub draft: bool,
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
| /      | List ops.                   |
| ?      | Help (this screen).         |
\--------------------------------------/

*/

impl druid::Widget<Data> for Widget {
    fn event(&mut self, ctx: &mut EventCtx, event: &Event, _data: &mut Data, _env: &Env) {
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
                            ctx.submit_command(
                                Command::new(MOVE_CURSOR, Vec2::new(-1.0, 0.0)),
                                None,
                            );
                        }
                        _ if HotKey::new(None, KeyCode::KeyJ).matches(event)
                            || HotKey::new(None, KeyCode::ArrowDown).matches(event) =>
                        {
                            ctx.submit_command(
                                Command::new(MOVE_CURSOR, Vec2::new(0.0, 1.0)),
                                None,
                            );
                        }
                        _ if HotKey::new(None, KeyCode::KeyK).matches(event)
                            || HotKey::new(None, KeyCode::ArrowUp).matches(event) =>
                        {
                            ctx.submit_command(
                                Command::new(MOVE_CURSOR, Vec2::new(0.0, -1.0)),
                                None,
                            );
                        }
                        _ if HotKey::new(None, KeyCode::KeyL).matches(event)
                            || HotKey::new(None, KeyCode::ArrowRight).matches(event)
                            || HotKey::new(None, KeyCode::Space).matches(event) =>
                        {
                            ctx.submit_command(
                                Command::new(MOVE_CURSOR, Vec2::new(1.0, 0.0)),
                                None,
                            );
                        }
                        _ if HotKey::new(RawMods::Alt, KeyCode::KeyH).matches(event)
                            || HotKey::new(RawMods::Alt, KeyCode::ArrowLeft).matches(event) =>
                        {
                            ctx.submit_command(Command::from(MOVE_NODE_LEFT), None);
                        }
                        _ if HotKey::new(RawMods::Alt, KeyCode::KeyJ).matches(event)
                            || HotKey::new(RawMods::Alt, KeyCode::ArrowDown).matches(event) =>
                        {
                            ctx.submit_command(Command::from(MOVE_NODE_DOWN), None);
                        }
                        _ if HotKey::new(RawMods::Alt, KeyCode::KeyK).matches(event)
                            || HotKey::new(RawMods::Alt, KeyCode::ArrowUp).matches(event) =>
                        {
                            ctx.submit_command(Command::from(MOVE_NODE_UP), None);
                        }
                        _ if HotKey::new(RawMods::Alt, KeyCode::KeyL).matches(event)
                            || HotKey::new(RawMods::Alt, KeyCode::ArrowRight).matches(event) =>
                        {
                            ctx.submit_command(Command::from(MOVE_NODE_RIGHT), None);
                        }
                        _ if HotKey::new(SysMods::Shift, KeyCode::KeyH).matches(event)
                            || HotKey::new(SysMods::Shift, KeyCode::ArrowLeft).matches(event) =>
                        {
                            ctx.submit_command(Command::from(MOVE_LINE_UP), None);
                        }
                        _ if HotKey::new(SysMods::Shift, KeyCode::KeyJ).matches(event)
                            || HotKey::new(SysMods::Shift, KeyCode::ArrowDown).matches(event) =>
                        {
                            ctx.submit_command(Command::from(MOVE_RIGHT_DOWN), None);
                        }
                        _ if HotKey::new(SysMods::Shift, KeyCode::KeyK).matches(event)
                            || HotKey::new(SysMods::Shift, KeyCode::ArrowUp).matches(event) =>
                        {
                            ctx.submit_command(Command::from(MOVE_LEFT_UP), None);
                        }
                        _ if HotKey::new(SysMods::Shift, KeyCode::KeyL).matches(event)
                            || HotKey::new(SysMods::Shift, KeyCode::ArrowRight).matches(event) =>
                        {
                            ctx.submit_command(Command::from(MOVE_LINE_DOWN), None);
                        }
                        _ if HotKey::new(None, KeyCode::KeyI).matches(event) => {
                            self.insert_mode(ctx);
                        }
                        _ if HotKey::new(SysMods::Shift, KeyCode::KeyI).matches(event) => {
                            self.insert_mode(ctx);
                            ctx.submit_command(Command::from(SPLASH), None);
                        }
                        _ if HotKey::new(None, KeyCode::KeyA).matches(event) => {
                            ctx.submit_command(
                                Command::new(MOVE_CURSOR, Vec2::new(1.0, 0.0)),
                                None,
                            );
                            self.insert_mode(ctx);
                        }
                        _ if HotKey::new(None, KeyCode::KeyC).matches(event) => {
                            self.insert_mode(ctx);
                            ctx.submit_command(Command::from(CUT_NODE), None);
                        }
                        _ if HotKey::new(None, KeyCode::KeyD).matches(event) => {
                            ctx.submit_command(Command::from(DELETE_NODE), None);
                        }
                        _ if HotKey::new(SysMods::Shift, KeyCode::KeyD).matches(event) => {
                            ctx.submit_command(Command::from(DELETE_LINE), None);
                        }
                        _ if HotKey::new(None, KeyCode::Return).matches(event) => {
                            ctx.submit_command(Command::from(COMMIT_PROGRAM), None);
                        }
                        _ if HotKey::new(None, KeyCode::Backslash).matches(event) => {
                            ctx.submit_command(Command::from(PLAY_PAUSE), None);
                        }
                        _ if HotKey::new(None, KeyCode::KeyR).matches(event) => {
                            ctx.submit_command(Command::from(TOGGLE_RECORD), None);
                        }
                        _ if HotKey::new(None, KeyCode::KeyU).matches(event) => {
                            ctx.submit_command(Command::from(UNDO), None);
                        }
                        _ if HotKey::new(SysMods::Shift, KeyCode::KeyU).matches(event) => {
                            ctx.submit_command(Command::from(REDO), None);
                        }
                        _ if HotKey::new(None, KeyCode::Equals).matches(event) => {
                            ctx.submit_command(Command::from(CYCLE_UP), None);
                        }
                        _ if HotKey::new(None, KeyCode::Minus).matches(event) => {
                            ctx.submit_command(Command::from(CYCLE_DOWN), None);
                        }
                        _ if HotKey::new(None, KeyCode::Comma).matches(event) => {
                            ctx.submit_command(Command::from(MOVE_RIGHT_TO_LEFT), None);
                        }
                        _ if HotKey::new(None, KeyCode::Period).matches(event) => {
                            ctx.submit_command(Command::from(MOVE_RIGHT_TO_RIGHT), None);
                        }
                        _ if HotKey::new(SysMods::Shift, KeyCode::Comma).matches(event) => {
                            ctx.submit_command(Command::from(MOVE_LEFT_TO_LEFT), None);
                        }
                        _ if HotKey::new(SysMods::Shift, KeyCode::Period).matches(event) => {
                            ctx.submit_command(Command::from(MOVE_LEFT_TO_RIGHT), None);
                        }
                        _ if HotKey::new(None, KeyCode::Backtick).matches(event) => {
                            ctx.submit_command(Command::from(DEBUG), None);
                        }
                        _ if HotKey::new(None, KeyCode::KeyO).matches(event) => {
                            ctx.submit_command(Command::from(INSERT_NEW_LINE_BELOW), None);
                            self.insert_mode(ctx);
                        }
                        _ if HotKey::new(SysMods::Shift, KeyCode::KeyO).matches(event) => {
                            ctx.submit_command(Command::from(INSERT_NEW_LINE_ABOVE), None);
                            self.insert_mode(ctx);
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
                            ctx.submit_command(
                                Command::new(MOVE_CURSOR, Vec2::new(-1.0, 0.0)),
                                None,
                            );
                        }
                        _ if HotKey::new(None, KeyCode::ArrowDown).matches(event) => {
                            ctx.submit_command(
                                Command::new(MOVE_CURSOR, Vec2::new(0.0, 1.0)),
                                None,
                            );
                        }
                        _ if HotKey::new(None, KeyCode::ArrowUp).matches(event) => {
                            ctx.submit_command(
                                Command::new(MOVE_CURSOR, Vec2::new(0.0, -1.0)),
                                None,
                            );
                        }
                        _ if HotKey::new(None, KeyCode::ArrowRight).matches(event) => {
                            ctx.submit_command(
                                Command::new(MOVE_CURSOR, Vec2::new(1.0, 0.0)),
                                None,
                            );
                        }
                        _ if HotKey::new(None, KeyCode::Space).matches(event) => {
                            ctx.submit_command(Command::from(MOVE_RIGHT_TO_RIGHT), None);
                        }
                        _ if HotKey::new(None, KeyCode::Backspace).matches(event) => {
                            ctx.submit_command(
                                Command::new(MOVE_CURSOR, Vec2::new(-1.0, 0.0)),
                                None,
                            );
                            ctx.submit_command(Command::from(NODE_DELETE_CHAR), None);
                        }
                        _ if event.key_code.is_printable() => {
                            if let Some(text) = event.text() {
                                ctx.submit_command(
                                    Command::new(
                                        NODE_INSERT_TEXT,
                                        NodeInsertText {
                                            text: text.to_string(),
                                        },
                                    ),
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
                    ctx.submit_command(
                        Command::new(
                            SET_CURSOR,
                            Point::new(
                                (event.pos.x / grid_unit.width).round(),
                                (event.pos.y / grid_unit.height).round(),
                            ),
                        ),
                        None,
                    );
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
        let grid_unit = self.get_grid_unit(&mut ctx.text());

        let size = ctx.size();

        // Clean.
        let frame = Rect::from_origin_size(Point::ORIGIN, size);
        ctx.fill(frame, &BACKGROUND_COLOR);

        if data.draft || !data.draft_nodes.is_empty() {
            ctx.stroke(
                Rect::from((Point::ZERO, size)),
                &Color::rgb8(0xff, 0x00, 0x00),
                1.0,
            );
        }

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
            let font = self.get_font(&mut ctx.text());
            let layout = ctx
                .text()
                .new_text_layout(font, &node.text, f64::INFINITY)
                .build()
                .unwrap();
            let color = if data.draft_nodes.contains(&node.id) {
                DRAFT_NODE_COLOR
            } else {
                DEFAULT_NODE_COLOR
            };
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

    fn get_font(&mut self, text: &mut PietText) -> &PietFont {
        if self.font.is_none() {
            self.font = Some(text.new_font_by_name(FONT_NAME, FONT_SIZE).build().unwrap());
        }
        self.font.as_ref().unwrap()
    }

    fn insert_mode(&mut self, ctx: &mut EventCtx) {
        self.mode = Mode::Insert;
        ctx.submit_command(Command::from(NEW_UNDO_GROUP), None);
    }

    fn normal_mode(&mut self, ctx: &mut EventCtx) {
        self.mode = Mode::Normal;
        ctx.submit_command(Command::from(NEW_UNDO_GROUP), None);
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
