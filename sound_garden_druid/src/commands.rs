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
