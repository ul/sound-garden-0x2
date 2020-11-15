use crate::{commands::*, theme::*};
use druid::{
    piet::{FontFamily, PietText, Text, TextLayout, TextLayoutBuilder},
    BoxConstraints, Env, Event, EventCtx, HotKey, KbKey, LayoutCtx, LifeCycle, LifeCycleCtx,
    PaintCtx, Point, RawMods, Rect, RenderContext, Size, SysMods, UpdateCtx, Vec2,
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
                            ctx.submit_command(MOVE_CURSOR.with(Vec2::new(-1.0, 0.0)));
                        }
                        _ if HotKey::new(None, "j").matches(event)
                            || HotKey::new(None, KbKey::ArrowDown).matches(event) =>
                        {
                            ctx.submit_command(MOVE_CURSOR.with(Vec2::new(0.0, 1.0)));
                        }
                        _ if HotKey::new(None, "k").matches(event)
                            || HotKey::new(None, KbKey::ArrowUp).matches(event) =>
                        {
                            ctx.submit_command(MOVE_CURSOR.with(Vec2::new(0.0, -1.0)));
                        }
                        _ if HotKey::new(None, "l").matches(event)
                            || HotKey::new(None, KbKey::ArrowRight).matches(event)
                            || HotKey::new(None, " ").matches(event) =>
                        {
                            ctx.submit_command(MOVE_CURSOR.with(Vec2::new(1.0, 0.0)));
                        }
                        _ if HotKey::new(RawMods::Alt, "h").matches(event)
                            || HotKey::new(RawMods::Alt, KbKey::ArrowLeft).matches(event) =>
                        {
                            ctx.submit_command(MOVE_NODE_LEFT);
                        }
                        _ if HotKey::new(RawMods::Alt, "j").matches(event)
                            || HotKey::new(RawMods::Alt, KbKey::ArrowDown).matches(event) =>
                        {
                            ctx.submit_command(MOVE_NODE_DOWN);
                        }
                        _ if HotKey::new(RawMods::Alt, "k").matches(event)
                            || HotKey::new(RawMods::Alt, KbKey::ArrowUp).matches(event) =>
                        {
                            ctx.submit_command(MOVE_NODE_UP);
                        }
                        _ if HotKey::new(RawMods::Alt, "l").matches(event)
                            || HotKey::new(RawMods::Alt, KbKey::ArrowRight).matches(event) =>
                        {
                            ctx.submit_command(MOVE_NODE_RIGHT);
                        }
                        _ if HotKey::new(SysMods::Shift, "J").matches(event)
                            || HotKey::new(SysMods::Shift, KbKey::ArrowDown).matches(event) =>
                        {
                            ctx.submit_command(MOVE_BELOW_DOWN);
                        }
                        _ if HotKey::new(SysMods::Shift, "K").matches(event)
                            || HotKey::new(SysMods::Shift, KbKey::ArrowUp).matches(event) =>
                        {
                            ctx.submit_command(MOVE_BELOW_UP);
                        }
                        _ if HotKey::new(RawMods::AltShift, "J").matches(event)
                            || HotKey::new(RawMods::AltShift, KbKey::ArrowDown).matches(event) =>
                        {
                            ctx.submit_command(MOVE_ABOVE_DOWN);
                        }
                        _ if HotKey::new(RawMods::AltShift, "K").matches(event)
                            || HotKey::new(RawMods::AltShift, KbKey::ArrowUp).matches(event) =>
                        {
                            ctx.submit_command(MOVE_ABOVE_UP);
                        }
                        _ if HotKey::new(SysMods::Shift, "H").matches(event)
                            || HotKey::new(SysMods::Shift, KbKey::ArrowLeft).matches(event) =>
                        {
                            ctx.submit_command(MOVE_LINE_UP);
                        }
                        _ if HotKey::new(SysMods::Shift, "L").matches(event)
                            || HotKey::new(SysMods::Shift, KbKey::ArrowRight).matches(event) =>
                        {
                            ctx.submit_command(MOVE_LINE_DOWN);
                        }
                        _ if HotKey::new(None, "i").matches(event) => {
                            ctx.submit_command(INSERT_MODE);
                        }
                        _ if HotKey::new(SysMods::Shift, "I").matches(event) => {
                            ctx.submit_command(SPLASH);
                        }
                        _ if HotKey::new(None, "a").matches(event) => {
                            ctx.submit_command(INSERT_MODE);
                            ctx.submit_command(MOVE_CURSOR.with(Vec2::new(1.0, 0.0)));
                        }
                        _ if HotKey::new(None, "c").matches(event) => {
                            ctx.submit_command(CUT_NODE);
                        }
                        _ if HotKey::new(None, "d").matches(event) => {
                            ctx.submit_command(DELETE_NODE);
                        }
                        _ if HotKey::new(SysMods::Shift, "D").matches(event) => {
                            ctx.submit_command(DELETE_LINE);
                        }
                        _ if HotKey::new(None, KbKey::Enter).matches(event) => {
                            ctx.submit_command(COMMIT_PROGRAM);
                        }
                        _ if HotKey::new(None, "\\").matches(event) => {
                            ctx.submit_command(PLAY_PAUSE);
                        }
                        _ if HotKey::new(None, "r").matches(event) => {
                            ctx.submit_command(TOGGLE_RECORD);
                        }
                        _ if HotKey::new(None, "u").matches(event) => {
                            ctx.submit_command(UNDO);
                        }
                        _ if HotKey::new(SysMods::Shift, "U").matches(event) => {
                            ctx.submit_command(REDO);
                        }
                        _ if HotKey::new(None, "=").matches(event) => {
                            ctx.submit_command(CYCLE_UP);
                        }
                        _ if HotKey::new(None, "-").matches(event) => {
                            ctx.submit_command(CYCLE_DOWN);
                        }
                        _ if HotKey::new(None, ",").matches(event) => {
                            ctx.submit_command(MOVE_RIGHT_TO_LEFT);
                        }
                        _ if HotKey::new(None, ".").matches(event) => {
                            ctx.submit_command(MOVE_RIGHT_TO_RIGHT);
                        }
                        _ if HotKey::new(SysMods::Shift, ">").matches(event) => {
                            ctx.submit_command(MOVE_LEFT_TO_LEFT);
                        }
                        _ if HotKey::new(SysMods::Shift, "<").matches(event) => {
                            ctx.submit_command(MOVE_LEFT_TO_RIGHT);
                        }
                        _ if HotKey::new(None, "`").matches(event) => {
                            ctx.submit_command(DEBUG);
                        }
                        _ if HotKey::new(None, "o").matches(event) => {
                            ctx.submit_command(INSERT_NEW_LINE_BELOW);
                        }
                        _ if HotKey::new(SysMods::Shift, "O").matches(event) => {
                            ctx.submit_command(INSERT_NEW_LINE_ABOVE);
                        }
                        _ if HotKey::new(None, "v").matches(event) => {
                            ctx.submit_command(TOGGLE_OSCILLOSCOPE);
                        }
                        _ if HotKey::new(RawMods::Alt, "=").matches(event) => {
                            ctx.submit_command(OSCILLOSCOPE_ZOOM_IN);
                        }
                        _ if HotKey::new(RawMods::Alt, "-").matches(event) => {
                            ctx.submit_command(OSCILLOSCOPE_ZOOM_OUT);
                        }
                        _ => {}
                    },
                    Mode::Insert => match event {
                        _ if HotKey::new(None, KbKey::Escape).matches(event)
                            || HotKey::new(None, KbKey::Enter).matches(event) =>
                        {
                            ctx.submit_command(NORMAL_MODE);
                        }
                        _ if HotKey::new(None, KbKey::ArrowLeft).matches(event) => {
                            ctx.submit_command(MOVE_CURSOR.with(Vec2::new(-1.0, 0.0)));
                        }
                        _ if HotKey::new(None, KbKey::ArrowDown).matches(event) => {
                            ctx.submit_command(MOVE_CURSOR.with(Vec2::new(0.0, 1.0)));
                        }
                        _ if HotKey::new(None, KbKey::ArrowUp).matches(event) => {
                            ctx.submit_command(MOVE_CURSOR.with(Vec2::new(0.0, -1.0)));
                        }
                        _ if HotKey::new(None, KbKey::ArrowRight).matches(event) => {
                            ctx.submit_command(MOVE_CURSOR.with(Vec2::new(1.0, 0.0)));
                        }
                        _ if HotKey::new(None, " ").matches(event) => {
                            ctx.submit_command(MOVE_RIGHT_TO_RIGHT);
                        }
                        _ if HotKey::new(None, KbKey::Backspace).matches(event) => {
                            ctx.submit_command(MOVE_CURSOR.with(Vec2::new(-1.0, 0.0)));
                            ctx.submit_command(NODE_DELETE_CHAR);
                        }
                        _ => {
                            if let KbKey::Character(text) = &event.key {
                                ctx.submit_command(NODE_INSERT_TEXT.with(text.to_string()));
                            }
                        }
                    },
                }
                ctx.request_paint();
            }
            Event::MouseDown(event) => {
                if let Some(grid_unit) = self.grid_unit {
                    ctx.submit_command(SET_CURSOR.with(Point::new(
                        (event.pos.x / grid_unit.width - 0.5).round(),
                        (event.pos.y / grid_unit.height - 0.5).round(),
                    )));
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
                .new_text_layout(node.text.clone())
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
