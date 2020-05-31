use druid::{Point, Selector, Vec2};

pub const NODE_INSERT_TEXT: Selector<NodeInsertText> =
    Selector::new("sound_garden_druid.NODE_INSERT_TEXT");
pub const NODE_DELETE_CHAR: Selector = Selector::new("sound_garden_druid.NODE_DELETE_CHAR");
pub const NEW_UNDO_GROUP: Selector = Selector::new("sound_garden_druid.NEW_UNDO_GROUP");
pub const COMMIT_PROGRAM: Selector = Selector::new("sound_garden_druid.COMMIT_PROGRAM");
pub const PLAY_PAUSE: Selector = Selector::new("sound_garden_druid.PLAY_PAUSE");
pub const TOGGLE_RECORD: Selector = Selector::new("sound_garden_druid.TOGGLE_RECORD");
pub const UNDO: Selector = Selector::new("sound_garden_druid.UNDO");
pub const REDO: Selector = Selector::new("sound_garden_druid.REDO");
pub const SAVE: Selector = Selector::new("sound_garden_druid.GENERATE_NODES");
pub const SPLASH: Selector = Selector::new("sound_garden_druid.SPLASH");
pub const DELETE_NODE: Selector = Selector::new("sound_garden_druid.DELETE_NODE");
pub const DELETE_LINE: Selector = Selector::new("sound_garden_druid.DELETE_LINE");
pub const CUT_NODE: Selector = Selector::new("sound_garden_druid.CUT_NODE");
pub const CYCLE_UP: Selector = Selector::new("sound_garden_druid.CYCLE_UP");
pub const CYCLE_DOWN: Selector = Selector::new("sound_garden_druid.CYCLE_DOWN");
pub const MOVE_RIGHT_TO_RIGHT: Selector = Selector::new("sound_garden_druid.MOVE_RIGHT_TO_RIGHT");
pub const MOVE_RIGHT_TO_LEFT: Selector = Selector::new("sound_garden_druid.MOVE_RIGHT_TO_LEFT");
pub const MOVE_LEFT_TO_RIGHT: Selector = Selector::new("sound_garden_druid.MOVE_LEFT_TO_RIGHT");
pub const MOVE_LEFT_TO_LEFT: Selector = Selector::new("sound_garden_druid.MOVE_LEFT_TO_LEFT");
pub const MOVE_NODE_LEFT: Selector = Selector::new("sound_garden_druid.MOVE_NODE_LEFT");
pub const MOVE_NODE_RIGHT: Selector = Selector::new("sound_garden_druid.MOVE_NODE_RIGHT");
pub const MOVE_NODE_UP: Selector = Selector::new("sound_garden_druid.MOVE_NODE_UP");
pub const MOVE_NODE_DOWN: Selector = Selector::new("sound_garden_druid.MOVE_NODE_DOWN");
pub const MOVE_LINE_UP: Selector = Selector::new("sound_garden_druid.MOVE_LINE_UP");
pub const MOVE_LINE_DOWN: Selector = Selector::new("sound_garden_druid.MOVE_LINE_DOWN");
pub const MOVE_LEFT_UP: Selector = Selector::new("sound_garden_druid.MOVE_LEFT_UP");
pub const MOVE_RIGHT_DOWN: Selector = Selector::new("sound_garden_druid.MOVE_RIGHT_DOWN");
pub const DEBUG: Selector = Selector::new("sound_garden_druid.DEBUG");
pub const INSERT_NEW_LINE_BELOW: Selector =
    Selector::new("sound_garden_druid.INSERT_NEW_LINE_BELOW");
pub const INSERT_NEW_LINE_ABOVE: Selector =
    Selector::new("sound_garden_druid.INSERT_NEW_LINE_ABOVE");
pub const MOVE_CURSOR: Selector<Vec2> = Selector::new("sound_garden_druid.MOVE_CURSOR");
pub const SET_CURSOR: Selector<Point> = Selector::new("sound_garden_druid.SET_CURSOR");

pub struct NodeInsertText {
    pub text: String,
}
