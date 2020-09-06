use crate::{commands::*, theme::*};
use druid::{
    piet::{FontFamily, PietText, Text, TextLayout, TextLayoutBuilder},
    BoxConstraints, Command, Env, Event, EventCtx, HotKey, KbKey, LayoutCtx, LifeCycle,
    LifeCycleCtx, PaintCtx, Point, RawMods, Rect, RenderContext, Size, SysMods, UpdateCtx, Vec2,
};
use sound_garden_types::*;
use std::sync::Arc;

#[derive(Default)]
pub struct Widget {
    grid_unit: Option<Size>,
    font: Option<FontFamily>,
}

#[derive(Clone, druid::Data, Default)]
pub struct Data {
    pub cursor: Cursor,
    pub draft_nodes: Arc<Vec<Id>>,
    pub mode: Mode,
    pub nodes: Arc<Vec<Node>>,
    pub window_size: Size,
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
    fn event(&mut self, ctx: &mut EventCtx, event: &Event, data: &mut Data, _env: &Env) {
        match event {
            Event::WindowConnected => {
                ctx.request_focus();
            }
            Event::KeyDown(event) => {
                match data.mode {
                    Mode::Normal => match event {
                        _ if HotKey::new(None, "h").matches(event)
                            || HotKey::new(None, KbKey::ArrowLeft).matches(event)
                            || HotKey::new(None, KbKey::Backspace).matches(event) =>
                        {
                            ctx.submit_command(
                                Command::new(MOVE_CURSOR, Vec2::new(-1.0, 0.0)),
                                None,
                            );
                        }
                        _ if HotKey::new(None, "j").matches(event)
                            || HotKey::new(None, KbKey::ArrowDown).matches(event) =>
                        {
                            ctx.submit_command(
                                Command::new(MOVE_CURSOR, Vec2::new(0.0, 1.0)),
                                None,
                            );
                        }
                        _ if HotKey::new(None, "k").matches(event)
                            || HotKey::new(None, KbKey::ArrowUp).matches(event) =>
                        {
                            ctx.submit_command(
                                Command::new(MOVE_CURSOR, Vec2::new(0.0, -1.0)),
                                None,
                            );
                        }
                        _ if HotKey::new(None, "l").matches(event)
                            || HotKey::new(None, KbKey::ArrowRight).matches(event)
                            || HotKey::new(None, " ").matches(event) =>
                        {
                            ctx.submit_command(
                                Command::new(MOVE_CURSOR, Vec2::new(1.0, 0.0)),
                                None,
                            );
                        }
                        _ if HotKey::new(RawMods::Alt, "h").matches(event)
                            || HotKey::new(RawMods::Alt, KbKey::ArrowLeft).matches(event) =>
                        {
                            ctx.submit_command(Command::from(MOVE_NODE_LEFT), None);
                        }
                        _ if HotKey::new(RawMods::Alt, "j").matches(event)
                            || HotKey::new(RawMods::Alt, KbKey::ArrowDown).matches(event) =>
                        {
                            ctx.submit_command(Command::from(MOVE_NODE_DOWN), None);
                        }
                        _ if HotKey::new(RawMods::Alt, "k").matches(event)
                            || HotKey::new(RawMods::Alt, KbKey::ArrowUp).matches(event) =>
                        {
                            ctx.submit_command(Command::from(MOVE_NODE_UP), None);
                        }
                        _ if HotKey::new(RawMods::Alt, "l").matches(event)
                            || HotKey::new(RawMods::Alt, KbKey::ArrowRight).matches(event) =>
                        {
                            ctx.submit_command(Command::from(MOVE_NODE_RIGHT), None);
                        }
                        _ if HotKey::new(SysMods::Shift, "J").matches(event)
                            || HotKey::new(SysMods::Shift, KbKey::ArrowDown).matches(event) =>
                        {
                            ctx.submit_command(Command::from(MOVE_BELOW_DOWN), None);
                        }
                        _ if HotKey::new(SysMods::Shift, "K").matches(event)
                            || HotKey::new(SysMods::Shift, KbKey::ArrowUp).matches(event) =>
                        {
                            ctx.submit_command(Command::from(MOVE_BELOW_UP), None);
                        }
                        _ if HotKey::new(RawMods::AltShift, "J").matches(event)
                            || HotKey::new(RawMods::AltShift, KbKey::ArrowDown).matches(event) =>
                        {
                            ctx.submit_command(Command::from(MOVE_ABOVE_DOWN), None);
                        }
                        _ if HotKey::new(RawMods::AltShift, "K").matches(event)
                            || HotKey::new(RawMods::AltShift, KbKey::ArrowUp).matches(event) =>
                        {
                            ctx.submit_command(Command::from(MOVE_ABOVE_UP), None);
                        }
                        _ if HotKey::new(SysMods::Shift, "H").matches(event)
                            || HotKey::new(SysMods::Shift, KbKey::ArrowLeft).matches(event) =>
                        {
                            ctx.submit_command(Command::from(MOVE_LINE_UP), None);
                        }
                        _ if HotKey::new(SysMods::Shift, "L").matches(event)
                            || HotKey::new(SysMods::Shift, KbKey::ArrowRight).matches(event) =>
                        {
                            ctx.submit_command(Command::from(MOVE_LINE_DOWN), None);
                        }
                        _ if HotKey::new(None, "i").matches(event) => {
                            ctx.submit_command(Command::from(INSERT_MODE), None);
                        }
                        _ if HotKey::new(SysMods::Shift, "I").matches(event) => {
                            ctx.submit_command(Command::from(SPLASH), None);
                        }
                        _ if HotKey::new(None, "a").matches(event) => {
                            ctx.submit_command(Command::from(INSERT_MODE), None);
                            ctx.submit_command(
                                Command::new(MOVE_CURSOR, Vec2::new(1.0, 0.0)),
                                None,
                            );
                        }
                        _ if HotKey::new(None, "c").matches(event) => {
                            ctx.submit_command(Command::from(CUT_NODE), None);
                        }
                        _ if HotKey::new(None, "d").matches(event) => {
                            ctx.submit_command(Command::from(DELETE_NODE), None);
                        }
                        _ if HotKey::new(SysMods::Shift, "D").matches(event) => {
                            ctx.submit_command(Command::from(DELETE_LINE), None);
                        }
                        _ if HotKey::new(None, KbKey::Enter).matches(event) => {
                            ctx.submit_command(Command::from(COMMIT_PROGRAM), None);
                        }
                        _ if HotKey::new(None, "\\").matches(event) => {
                            ctx.submit_command(Command::from(PLAY_PAUSE), None);
                        }
                        _ if HotKey::new(None, "r").matches(event) => {
                            ctx.submit_command(Command::from(TOGGLE_RECORD), None);
                        }
                        _ if HotKey::new(None, "u").matches(event) => {
                            ctx.submit_command(Command::from(UNDO), None);
                        }
                        _ if HotKey::new(SysMods::Shift, "U").matches(event) => {
                            ctx.submit_command(Command::from(REDO), None);
                        }
                        _ if HotKey::new(None, "=").matches(event) => {
                            ctx.submit_command(Command::from(CYCLE_UP), None);
                        }
                        _ if HotKey::new(None, "-").matches(event) => {
                            ctx.submit_command(Command::from(CYCLE_DOWN), None);
                        }
                        _ if HotKey::new(None, ",").matches(event) => {
                            ctx.submit_command(Command::from(MOVE_RIGHT_TO_LEFT), None);
                        }
                        _ if HotKey::new(None, ".").matches(event) => {
                            ctx.submit_command(Command::from(MOVE_RIGHT_TO_RIGHT), None);
                        }
                        _ if HotKey::new(SysMods::Shift, ">").matches(event) => {
                            ctx.submit_command(Command::from(MOVE_LEFT_TO_LEFT), None);
                        }
                        _ if HotKey::new(SysMods::Shift, "<").matches(event) => {
                            ctx.submit_command(Command::from(MOVE_LEFT_TO_RIGHT), None);
                        }
                        _ if HotKey::new(None, "`").matches(event) => {
                            ctx.submit_command(Command::from(DEBUG), None);
                        }
                        _ if HotKey::new(None, "o").matches(event) => {
                            ctx.submit_command(Command::from(INSERT_NEW_LINE_BELOW), None);
                        }
                        _ if HotKey::new(SysMods::Shift, "O").matches(event) => {
                            ctx.submit_command(Command::from(INSERT_NEW_LINE_ABOVE), None);
                        }
                        _ if HotKey::new(None, "v").matches(event) => {
                            ctx.submit_command(Command::from(TOGGLE_OSCILLOSCOPE), None);
                        }
                        _ if HotKey::new(RawMods::Alt, "=").matches(event) => {
                            ctx.submit_command(Command::from(OSCILLOSCOPE_ZOOM_IN), None);
                        }
                        _ if HotKey::new(RawMods::Alt, "-").matches(event) => {
                            ctx.submit_command(Command::from(OSCILLOSCOPE_ZOOM_OUT), None);
                        }
                        _ => {}
                    },
                    Mode::Insert => match event {
                        _ if HotKey::new(None, KbKey::Escape).matches(event)
                            || HotKey::new(None, KbKey::Enter).matches(event) =>
                        {
                            ctx.submit_command(Command::from(NORMAL_MODE), None);
                        }
                        _ if HotKey::new(None, KbKey::ArrowLeft).matches(event) => {
                            ctx.submit_command(
                                Command::new(MOVE_CURSOR, Vec2::new(-1.0, 0.0)),
                                None,
                            );
                        }
                        _ if HotKey::new(None, KbKey::ArrowDown).matches(event) => {
                            ctx.submit_command(
                                Command::new(MOVE_CURSOR, Vec2::new(0.0, 1.0)),
                                None,
                            );
                        }
                        _ if HotKey::new(None, KbKey::ArrowUp).matches(event) => {
                            ctx.submit_command(
                                Command::new(MOVE_CURSOR, Vec2::new(0.0, -1.0)),
                                None,
                            );
                        }
                        _ if HotKey::new(None, KbKey::ArrowRight).matches(event) => {
                            ctx.submit_command(
                                Command::new(MOVE_CURSOR, Vec2::new(1.0, 0.0)),
                                None,
                            );
                        }
                        _ if HotKey::new(None, " ").matches(event) => {
                            ctx.submit_command(Command::from(MOVE_RIGHT_TO_RIGHT), None);
                        }
                        _ if HotKey::new(None, KbKey::Backspace).matches(event) => {
                            ctx.submit_command(
                                Command::new(MOVE_CURSOR, Vec2::new(-1.0, 0.0)),
                                None,
                            );
                            ctx.submit_command(Command::from(NODE_DELETE_CHAR), None);
                        }
                        _ => {
                            if let KbKey::Character(text) = &event.key {
                                ctx.submit_command(
                                    Command::new(NODE_INSERT_TEXT, text.to_string()),
                                    None,
                                );
                            }
                        }
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
                                (event.pos.x / grid_unit.width - 0.5).round(),
                                (event.pos.y / grid_unit.height - 0.5).round(),
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

    fn update(&mut self, ctx: &mut UpdateCtx, old_data: &Data, data: &Data, _env: &Env) {
        use druid::Data;
        if !data.same(old_data) {
            ctx.request_paint();
        }
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        _bc: &BoxConstraints,
        data: &Data,
        _env: &Env,
    ) -> Size {
        let grid_unit = self.get_grid_unit(&mut ctx.text());
        Size::from(data.nodes.iter().fold(
            (
                data.window_size.width,
                data.window_size.height - MODELINE_HEIGHT,
            ),
            |(width, height), node| {
                let x =
                    (node.position.x + node.text.chars().count() as f64 + 1.0) * grid_unit.width;
                let y = (node.position.y + 2.0) * grid_unit.height;
                (width.max(x), height.max(y))
            },
        ))
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &Data, _env: &Env) {
        let grid_unit = self.get_grid_unit(&mut ctx.text());

        // Clean.
        let size = ctx.size();
        ctx.fill(size.to_rect(), &BACKGROUND_COLOR);

        // Draw a cursor.
        match data.mode {
            Mode::Normal => {
                ctx.fill(
                    Rect::from((
                        Point::new(
                            data.cursor.position.x * grid_unit.width,
                            data.cursor.position.y * grid_unit.height,
                        ),
                        grid_unit,
                    )),
                    &CURSOR_COLOR.with_alpha(CURSOR_NORMAL_ALPHA),
                );
            }
            Mode::Insert => {
                ctx.fill(
                    Rect::from((
                        Point::new(
                            data.cursor.position.x * grid_unit.width - 1.0,
                            data.cursor.position.y * grid_unit.height,
                        ),
                        Size::new(2.0, grid_unit.height),
                    )),
                    &CURSOR_COLOR.with_alpha(CURSOR_INSERT_ALPHA),
                );
            }
        }

        // Draw nodes.
        for node in data.nodes.iter() {
            let font = self.get_font(&mut ctx.text());
            let color = if data.draft_nodes.contains(&node.id) {
                NODE_DRAFT_COLOR
            } else {
                NODE_DEFAULT_COLOR
            };
            let layout = ctx
                .text()
                .new_text_layout(&node.text)
                .font(font.clone(), FONT_SIZE)
                .text_color(color)
                .build()
                .unwrap();
            ctx.draw_text(
                &layout,
                Point::new(
                    node.position.x * grid_unit.width,
                    node.position.y * grid_unit.height,
                ),
            );
        }
    }
}

impl Widget {
    fn get_grid_unit(&mut self, text: &mut PietText) -> Size {
        if self.grid_unit.is_none() {
            let font = self.get_font(text);
            let layout = text
                .new_text_layout("Q")
                .font(font.clone(), FONT_SIZE)
                .text_color(FOREGROUND_COLOR)
                .build()
                .unwrap();
            self.grid_unit = Some(layout.size());
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
