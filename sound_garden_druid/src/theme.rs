use druid::Color;

// TODO Move these pub constants to Env.
pub const FONT_NAME: &str = "IBM Plex Mono";
pub const FONT_SIZE: f64 = 20.0;
pub const MODELINE_FONT_SIZE: f64 = 16.0;
pub const OSCILLOSCOPE_FONT_SIZE: f64 = 14.0;
pub const FOREGROUND_COLOR: Color = Color::rgb8(0x20, 0x20, 0x20);
pub const BACKGROUND_COLOR: Color = Color::WHITE;
pub const CURSOR_NORMAL_ALPHA: f64 = 0.33;
pub const CURSOR_INSERT_ALPHA: f64 = 1.0;
pub const NODE_DEFAULT_COLOR: Color = FOREGROUND_COLOR;
pub const NODE_DRAFT_COLOR: Color = Color::rgb8(0xff, 0x00, 0x00);
pub const MODELINE_DRAFT_COLOR: Color = NODE_DRAFT_COLOR;
pub const MODELINE_HEIGHT: f64 = 36.0;
pub const MODELINE_NORMAL_COLOR: Color = Color::rgb8(0xcc, 0xcc, 0xcc);
pub const MODELINE_INSERT_COLOR: Color = Color::rgb8(0x11, 0xcc, 0x11);
pub const MODELINE_RECORD_COLOR: Color = Color::rgb8(0xff, 0x00, 0x00);
pub const OSCILLOSCOPE_FOREGROUND_COLOR: Color = Color::rgb8(0x00, 0x88, 0x00);
