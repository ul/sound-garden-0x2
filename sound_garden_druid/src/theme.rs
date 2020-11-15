use druid::Color;

// TODO Move these pub constants to Env.
pub const FONT_NAME: &str = "IBM Plex Mono";
pub const FONT_SIZE: f64 = 14.0;
pub const MODELINE_FONT_SIZE: f64 = 12.0;
pub const OSCILLOSCOPE_FONT_SIZE: f64 = 12.0;
pub const FOREGROUND_COLOR: Color = Color::rgb8(0x22, 0x22, 0x20);
pub const BACKGROUND_COLOR: Color = Color::rgb8(0xf3, 0xf0, 0xe8);
pub const CURSOR_COLOR: Color = Color::rgb8(0x22, 0x22, 0x20);
pub const CURSOR_NORMAL_ALPHA: f64 = 0.33;
pub const CURSOR_INSERT_ALPHA: f64 = 0.66;
pub const NODE_DEFAULT_COLOR: Color = FOREGROUND_COLOR;
pub const NODE_DRAFT_COLOR: Color = Color::rgb8(0xff, 0x81, 0x2b);
pub const MODELINE_DRAFT_COLOR: Color = NODE_DRAFT_COLOR;
pub const MODELINE_HEIGHT: f64 = 26.0;
pub const MODELINE_NORMAL_COLOR: Color = Color::rgb8(0xcc, 0xcc, 0xcc);
pub const MODELINE_INSERT_COLOR: Color = Color::rgb8(0x55, 0xae, 0x39);
pub const MODELINE_RECORD_COLOR: Color = Color::rgb8(0xdf, 0x00, 0x00);
pub const OSCILLOSCOPE_FOREGROUND_COLOR: Color = BACKGROUND_COLOR;
pub const OSCILLOSCOPE_BACKGROUND_COLOR: Color = Color::rgb8(0x4c, 0x4c, 0x49);
