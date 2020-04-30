use crate::types::Id;
use druid::{Command, Point, Selector};

// NOTE Do not construct commands from selectors directly in other modules.
// Provide and use command creator functions instead.
// For the price of a modest boilerplate increase it provides (just a tiny bit) better type safety.

pub const NODE_INSERT_TEXT: Selector = Selector::new("sound_garden_druid.NODE_INSERT_TEXT");

pub struct NodeInsertText {
    pub id: Id,
    pub index: usize,
    pub text: String,
}

pub fn node_insert_text(payload: NodeInsertText) -> Command {
    Command::new(NODE_INSERT_TEXT, payload)
}

pub const NODE_DELETE_CHAR: Selector = Selector::new("sound_garden_druid.NODE_DELETE_CHAR");

pub struct NodeDeleteChar {
    pub id: Id,
    pub index: usize,
}

pub fn node_delete_char(payload: NodeDeleteChar) -> Command {
    Command::new(NODE_DELETE_CHAR, payload)
}

pub const CREATE_NODE: Selector = Selector::new("sound_garden_druid.CREATE_NODE");

pub struct CreateNode {
    pub position: Point,
    pub text: String,
}

pub fn create_node(payload: CreateNode) -> Command {
    Command::new(CREATE_NODE, payload)
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
pub const POLL_NODES: Selector = Selector::new("sound_garden_druid.POLL_NODES");

pub fn poll_nodes() -> Command {
    Command::from(POLL_NODES)
}
