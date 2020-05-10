use druid::{Command, Selector};

// NOTE Do not construct commands from selectors directly in other modules.
// Provide and use command creator functions instead.
// For the price of a modest boilerplate increase it provides (just a tiny bit) better type safety.

pub const NODE_INSERT_TEXT: Selector = Selector::new("sound_garden_druid.NODE_INSERT_TEXT");

pub struct NodeInsertText {
    pub text: String,
}

pub fn node_insert_text(payload: NodeInsertText) -> Command {
    Command::new(NODE_INSERT_TEXT, payload)
}

pub const NODE_DELETE_CHAR: Selector = Selector::new("sound_garden_druid.NODE_DELETE_CHAR");

pub fn node_delete_char() -> Command {
    Command::from(NODE_DELETE_CHAR)
}

pub const NEW_UNDO_GROUP: Selector = Selector::new("sound_garden_druid.NEW_UNDO_GROUP");

pub fn new_undo_group() -> Command {
    Command::from(NEW_UNDO_GROUP)
}

pub const COMMIT_PROGRAM: Selector = Selector::new("sound_garden_druid.COMMIT_PROGRAM");

pub fn commit_program() -> Command {
    Command::from(COMMIT_PROGRAM)
}

pub const PLAY_PAUSE: Selector = Selector::new("sound_garden_druid.PLAY_PAUSE");

pub fn play_pause() -> Command {
    Command::from(PLAY_PAUSE)
}

pub const TOGGLE_RECORD: Selector = Selector::new("sound_garden_druid.TOGGLE_RECORD");

pub fn toggle_record() -> Command {
    Command::from(TOGGLE_RECORD)
}

pub const UNDO: Selector = Selector::new("sound_garden_druid.UNDO");

pub fn undo() -> Command {
    Command::from(UNDO)
}

pub const REDO: Selector = Selector::new("sound_garden_druid.REDO");

pub fn redo() -> Command {
    Command::from(REDO)
}

pub const SAVE: Selector = Selector::new("sound_garden_druid.GENERATE_NODES");

pub const SPLASH: Selector = Selector::new("sound_garden_druid.SPLASH");

pub fn splash() -> Command {
    Command::from(SPLASH)
}

pub const DELETE_NODE: Selector = Selector::new("sound_garden_druid.DELETE_NODE");

pub fn delete_node() -> Command {
    Command::from(DELETE_NODE)
}

pub const DELETE_LINE: Selector = Selector::new("sound_garden_druid.DELETE_LINE");

pub fn delete_line() -> Command {
    Command::from(DELETE_LINE)
}

pub const CUT_NODE: Selector = Selector::new("sound_garden_druid.CUT_NODE");

pub fn cut_node() -> Command {
    Command::from(CUT_NODE)
}

pub const CYCLE_UP: Selector = Selector::new("sound_garden_druid.CYCLE_UP");

pub fn cycle_up() -> Command {
    Command::from(CYCLE_UP)
}

pub const CYCLE_DOWN: Selector = Selector::new("sound_garden_druid.CYCLE_DOWN");

pub fn cycle_down() -> Command {
    Command::from(CYCLE_DOWN)
}
