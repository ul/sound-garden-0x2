use anyhow::Result;
use audio_program::{get_help, get_op_groups, TextOp};
use crossbeam_channel::Sender;
use rand::prelude::*;
use redo::{Command, Record};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::HashMap;

pub const MIN_X: i16 = 2;
pub const MIN_Y: i16 = 2;

// TODO make less pub
pub struct App {
    pub cycles: Vec<Vec<String>>,
    pub help_scroll: u16,
    pub input_mode: InputMode,
    pub op_groups: Vec<(String, Vec<String>)>,
    pub op_help: HashMap<String, String>,
    pub recording: bool,
    pub screen: Screen,
    pub status: String,
    filename: String,
    play: bool,
    saved_state: Record<SavedStateCommand>,
    tx_ops: Sender<Vec<TextOp>>,
}

#[derive(Serialize, Deserialize, Default)]
pub struct SavedState {
    cursor: Position,
    nodes: Vec<Node>,
    #[serde(default)]
    program: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Node {
    #[serde(default = "random")]
    pub id: u64,
    #[serde(skip, default)]
    pub draft: bool,
    pub op: String,
    pub position: Position,
}

#[derive(Serialize, Deserialize)]
pub enum InputMode {
    Normal,
    Insert,
}

pub enum Screen {
    Editor,
    Help,
    Ops,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Ord, Deserialize, Serialize)]
pub struct Position {
    pub x: i16,
    pub y: i16,
}

impl App {
    pub fn new(filename: String, tx_ops: Sender<Vec<TextOp>>) -> Self {
        let mut app = App {
            cycles: default_cycles(),
            filename,
            help_scroll: 0,
            input_mode: Default::default(),
            op_groups: get_op_groups(),
            op_help: get_help(),
            play: Default::default(),
            recording: Default::default(),
            saved_state: redo::record::Builder::new().limit(10000).default(),
            screen: Default::default(),
            status: Default::default(),
            tx_ops,
        };
        let target = app.saved_state.target_mut();
        target.cursor = Position { x: MIN_X, y: MIN_Y };
        app
    }

    // TODO Atomic write.
    pub fn save(&mut self) -> Result<()> {
        let f = std::fs::File::create(&self.filename)?;
        serde_json::to_writer_pretty(f, &self.saved_state)?;
        self.saved_state.set_saved(true);
        Ok(())
    }

    pub fn load(path: &str, tx_ops: Sender<Vec<TextOp>>) -> Result<Self> {
        let mut app = Self::new(path.to_owned(), tx_ops);
        // If path doesn't exist then we just provide default value...
        if let Ok(f) = std::fs::File::open(path) {
            // ...but if data is malformed we fail. Consumer is then responsible
            // to decide if to proceed with default state and overwrite file or
            // stop as "malformed" data may contain something important.
            app.saved_state = serde_json::from_reader(f)?;
        }
        app.load_program();
        Ok(app)
    }

    pub fn commit(&mut self) {
        self.save().ok();
        self.load_program();
    }

    pub fn undo(&mut self) {
        self.saved_state.undo().ok();
    }

    pub fn redo(&mut self) {
        self.saved_state.redo().ok();
    }

    pub fn toggle_play(&mut self) {
        self.play = !self.play;
    }

    pub fn play(&self) -> bool {
        self.play
    }

    pub fn draft(&mut self) -> bool {
        self.draft_program() != self.program()
    }

    pub fn input_mode(&self) -> &InputMode {
        &self.input_mode
    }

    pub fn normal_mode(&mut self) {
        self.saved_state
            .target_mut()
            .nodes
            .retain(|node| !node.op.is_empty());
        self.input_mode = InputMode::Normal;
    }

    pub fn insert_mode(&mut self) {
        self.input_mode = InputMode::Insert;
    }

    pub fn randomize_node_ids(&mut self) {
        self.saved_state
            .apply(SavedStateCommand::RandomizeNodeIds {
                previous_ids: Default::default(),
            })
            .ok();
    }

    pub fn move_cursor(&mut self, offset: Position) {
        self.saved_state
            .apply(SavedStateCommand::MoveCursor { offset })
            .ok();
    }

    pub fn move_nodes_and_cursor(
        &mut self,
        ids: Vec<u64>,
        nodes_offset: Position,
        cursor_offset: Position,
    ) {
        self.saved_state
            .apply(SavedStateCommand::MoveNodesAndCursor {
                cursor_offset,
                ids,
                nodes_offset,
            })
            .ok();
    }

    pub fn delete_nodes(&mut self, ids: Vec<u64>) {
        self.saved_state
            .apply(SavedStateCommand::DeleteNodes {
                ids,
                nodes: Vec::new(),
            })
            .ok();
    }

    pub fn insert_char(&mut self, char: char) {
        self.saved_state
            .apply(SavedStateCommand::InsertChar {
                char,
                id: Default::default(),
                ids: Default::default(),
                ix: Default::default(),
            })
            .ok();
    }

    pub fn delete_char(&mut self) {
        self.saved_state
            .apply(SavedStateCommand::DeleteChar {
                char: Default::default(),
                id: Default::default(),
                ids: Default::default(),
                ix: Default::default(),
            })
            .ok();
    }

    pub fn splash(&mut self) {
        self.saved_state
            .apply(SavedStateCommand::Splash {
                cursor_offset: Default::default(),
                ids: Default::default(),
            })
            .ok();
    }

    pub fn insert_line(&mut self) {
        self.saved_state
            .apply(SavedStateCommand::InsertLine {
                cursor_offset: Default::default(),
                ids: Default::default(),
            })
            .ok();
    }

    pub fn cut_op(&mut self) {
        self.saved_state
            .apply(SavedStateCommand::CutOp {
                id: Default::default(),
                ids: Default::default(),
                node: Default::default(),
                nodes_offset: Default::default(),
                tail: Default::default(),
            })
            .ok();
    }

    pub fn replace_op(&mut self, id: u64, op: String) {
        self.saved_state
            .apply(SavedStateCommand::ReplaceOp { id, op })
            .ok();
    }

    pub fn insert_space(&mut self) {
        self.saved_state
            .apply(SavedStateCommand::InsertSpace {
                ids: Default::default(),
            })
            .ok();
    }

    pub fn nodes(&self) -> &[Node] {
        &self.saved_state.target().nodes
    }

    pub fn cursor(&self) -> Position {
        self.saved_state.target().cursor
    }

    pub fn program(&self) -> &str {
        &self.saved_state.target().program
    }

    pub fn node_at_cursor(&self) -> Option<&Node> {
        self.saved_state.target().node_at_cursor()
    }

    pub fn update(&mut self) {
        self.update_status();
    }

    fn update_status(&mut self) {
        self.status = String::new();
        if let Some(Node { op, .. }) = self.node_at_cursor() {
            if let Some(help) = self.op_help.get(op.split(':').next().unwrap()) {
                self.status = help.to_owned();
            }
        }
    }

    fn load_program(&mut self) {
        let draft_program = self.draft_program();
        let target = self.saved_state.target_mut();
        target.nodes.iter_mut().for_each(|node| node.draft = false);
        target.program = draft_program;
        self.tx_ops.send(self.ops()).ok();
    }

    fn draft_program(&mut self) -> String {
        let target = &mut self.saved_state.target_mut();
        target.nodes.sort_by_key(|node| node.position);
        target
            .nodes
            .iter()
            .map(|node| node.op.to_owned())
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn ops(&self) -> Vec<TextOp> {
        self.nodes()
            .iter()
            .map(|Node { id, op, .. }| TextOp {
                id: *id,
                op: op.to_owned(),
            })
            .collect()
    }
}

impl SavedState {
    fn node_at_cursor(&self) -> Option<&Node> {
        let cursor = self.cursor;
        self.nodes.iter().find(
            |Node {
                 position: Position { y, x },
                 op,
                 ..
             }| {
                *y == cursor.y
                    && *x <= cursor.x
                    // space after node is counted as a part of the node
                    && cursor.x <= *x + op.chars().count() as i16
            },
        )
    }

    fn node_at_cursor_mut(&mut self) -> Option<&mut Node> {
        let cursor = self.cursor;
        self.nodes.iter_mut().find(
            |Node {
                 position: Position { y, x },
                 op,
                 ..
             }| {
                *y == cursor.y
                    && *x <= cursor.x
                    // space after node is counted as a part of the node
                    && cursor.x <= *x + op.chars().count() as i16
            },
        )
    }

    fn move_cursor(&mut self, offset: Position) {
        self.cursor.x += offset.x;
        self.cursor.y += offset.y;
    }

    fn move_nodes(&mut self, ids: &[u64], offset: Position) {
        for node in self.nodes.iter_mut().filter(|node| ids.contains(&node.id)) {
            node.position = node.position + offset;
        }
    }
}

impl Position {
    pub fn x(x: i16) -> Self {
        Self { x, y: 0 }
    }

    pub fn y(y: i16) -> Self {
        Self { x: 0, y }
    }
}

impl std::ops::Neg for Position {
    type Output = Self;

    fn neg(self) -> Self::Output {
        Self {
            x: -self.x,
            y: -self.y,
        }
    }
}

impl std::ops::Add for Position {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self {
            x: self.x + rhs.x,
            y: self.y + rhs.y,
        }
    }
}

impl std::ops::Sub for Position {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self {
            x: self.x - rhs.x,
            y: self.y - rhs.y,
        }
    }
}

fn default_cycles() -> Vec<Vec<String>> {
    // NOTE Always repeat the first element at the end.
    vec![vec!["s", "t", "w", "c", "s"]]
        .iter()
        .map(|cycle| cycle.iter().map(|s| s.to_string()).collect())
        .collect()
}

impl PartialOrd for Position {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        let y = self.y.cmp(&other.y);
        Some(if let Ordering::Equal = y {
            self.x.cmp(&other.x)
        } else {
            y
        })
    }
}

impl Default for InputMode {
    fn default() -> Self {
        InputMode::Normal
    }
}

impl Default for Screen {
    fn default() -> Self {
        Screen::Editor
    }
}

#[derive(Serialize, Deserialize)]
enum SavedStateCommand {
    DeleteNodes {
        ids: Vec<u64>,
        nodes: Vec<Node>,
    },
    InsertNode {
        id: u64,
        node: Option<Node>,
    },
    MoveCursor {
        offset: Position,
    },
    MoveNodesAndCursor {
        cursor_offset: Position,
        ids: Vec<u64>,
        nodes_offset: Position,
    },
    RandomizeNodeIds {
        previous_ids: HashMap<u64, u64>,
    },
    Splash {
        cursor_offset: Position,
        ids: Vec<u64>,
    },
    InsertLine {
        cursor_offset: Position,
        ids: Vec<u64>,
    },
    CutOp {
        id: Option<u64>,
        ids: Vec<u64>,
        node: Option<Node>,
        nodes_offset: Position,
        tail: String,
    },
    ReplaceOp {
        id: u64,
        op: String,
    },
    InsertSpace {
        ids: Vec<u64>,
    },
    InsertChar {
        char: char,
        id: Option<u64>,
        ids: Vec<u64>,
        ix: usize,
    },
    DeleteChar {
        char: Option<char>,
        id: Option<u64>,
        ids: Vec<u64>,
        ix: usize,
    },
    Batch {
        commands: Vec<SavedStateCommand>,
    },
}

impl SavedStateCommand {
    fn apply_impl(&mut self, state: &mut SavedState) -> redo::Result<Self> {
        use SavedStateCommand::*;
        match self {
            RandomizeNodeIds { previous_ids } => {
                state.nodes.iter_mut().for_each(|node| {
                    let next_id = random();
                    previous_ids.insert(next_id, node.id);
                    node.id = next_id;
                });
            }
            MoveCursor { offset } => {
                state.move_cursor(*offset);
            }
            MoveNodesAndCursor {
                ids,
                nodes_offset,
                cursor_offset,
            } => {
                state.move_nodes(ids, *nodes_offset);
                state.move_cursor(*cursor_offset);
            }
            DeleteNodes { ids, nodes } => {
                nodes.clear();
                let mut i = 0;
                while i < state.nodes.len() {
                    if ids.contains(&state.nodes[i].id) {
                        nodes.push(state.nodes.remove(i));
                    } else {
                        i += 1;
                    }
                }
            }
            InsertNode { node, .. } => {
                if let Some(node) = node.take() {
                    state.nodes.push(node);
                }
            }
            Splash { cursor_offset, ids } => {
                if let Some(Node {
                    op, position: p, ..
                }) = state.node_at_cursor()
                {
                    let len = op.chars().count();
                    let new_cursor_x = if p.x == state.cursor.x && len > 1 {
                        p.x
                    } else {
                        p.x + len as i16 + 1
                    };
                    *cursor_offset = Position::x(new_cursor_x - state.cursor.x);
                    state.move_cursor(*cursor_offset);
                };
                if state.node_at_cursor().is_some() {
                    let p = state.cursor;
                    *ids = state
                        .nodes
                        .iter()
                        .filter(|node| node.position.y == p.y && node.position.x >= p.x)
                        .map(|node| node.id)
                        .collect();
                    state.move_nodes(ids, Position::x(1));
                }
            }
            InsertLine { cursor_offset, ids } => {
                let p = state.cursor;
                *ids = state
                    .nodes
                    .iter()
                    .filter(|node| node.position.y > p.y)
                    .map(|node| node.id)
                    .collect();
                state.move_nodes(ids, Position::y(1));
                *cursor_offset = Position {
                    x: state
                        .nodes
                        .iter()
                        .filter(|node| node.position.y == p.y)
                        .min_by_key(|node| node.position.x)
                        .map(|node| node.position.x)
                        .unwrap_or(MIN_X)
                        - state.cursor.x,
                    y: 1,
                };
                state.move_cursor(*cursor_offset);
            }
            CutOp {
                id,
                ids,
                node,
                nodes_offset,
                tail,
            } => {
                let cursor = state.cursor;
                let mut remove_node = false;
                if let Some(node) = state.node_at_cursor_mut() {
                    *id = Some(node.id);
                    node.draft = true;
                    *nodes_offset =
                        Position::x(cursor.x - node.position.x - node.op.chars().count() as i16);
                    let ix = (cursor.x - node.position.x) as usize;
                    let chars = node.op.chars().collect::<Vec<_>>();
                    let chars = &mut chars.iter();
                    if ix > 0 {
                        node.op = chars.take(ix).collect();
                    } else {
                        remove_node = true;
                    }
                    *tail = chars.collect();
                    *ids = state
                        .nodes
                        .iter()
                        .filter(|node| node.position.y == cursor.y && node.position.x > cursor.x)
                        .map(|node| node.id)
                        .collect();
                    state.move_nodes(ids, *nodes_offset);
                }
                if remove_node {
                    *node = id
                        .and_then(|id| state.nodes.iter().position(|node| node.id == id))
                        .map(|ix| state.nodes.remove(ix));
                }
            }
            ReplaceOp { id, op } => {
                if let Some(node) = state.nodes.iter_mut().find(|node| node.id == *id) {
                    node.draft = true;
                    *op = std::mem::replace(&mut node.op, op.to_owned());
                }
            }
            InsertSpace { ids } => {
                let offset = Position::x(1);
                let p = state.cursor;
                *ids = state
                    .nodes
                    .iter()
                    .filter(|node| {
                        node.position.y == p.y
                            && p.x < node.position.x + node.op.chars().count() as i16
                    })
                    .map(|node| node.id)
                    .collect();
                state.move_nodes(ids, offset);
                state.move_cursor(offset);
            }
            InsertChar { char, id, ids, ix } => {
                let p = state.cursor;
                *ids = state
                    .nodes
                    .iter()
                    .filter(|node| node.position.y == p.y && p.x < node.position.x)
                    .map(|node| node.id)
                    .collect();
                state.move_nodes(ids, Position::x(1));
                if let Some(node) = state.node_at_cursor_mut() {
                    *id = Some(node.id);
                    *ix = (p.x - node.position.x) as usize;
                    let mut chars = node.op.chars().collect::<Vec<_>>();
                    chars.insert(*ix, *char);
                    node.op = chars.iter().collect();
                    node.draft = true;
                } else {
                    let node = Node {
                        id: random(),
                        draft: true,
                        op: char.to_string(),
                        position: state.cursor,
                    };
                    *id = Some(node.id);
                    state.nodes.push(node);
                };
                state.move_cursor(Position::x(1));
            }
            DeleteChar { char, id, ids, ix } => {
                let node_prev_x = state
                    .node_at_cursor()
                    .map(|node| node.position.x)
                    .unwrap_or_default();
                let p = state.cursor;
                let offset = Position::x(-1);
                *ids = state
                    .nodes
                    .iter()
                    .filter(|node| node.position.y == p.y && p.x <= node.position.x)
                    .map(|node| node.id)
                    .collect();
                state.move_nodes(ids, offset);
                state.move_cursor(offset);
                let cursor = state.cursor;
                if let Some(node) = state.node_at_cursor_mut() {
                    let len = node.op.chars().count();
                    if node_prev_x == node.position.x && cursor.x < node.position.x + len as i16 {
                        node.draft = true;
                        *id = Some(node.id);
                        *ix = (cursor.x - node.position.x) as usize;
                        let mut chars = node.op.chars().collect::<Vec<_>>();
                        *char = Some(chars.remove(*ix));
                        node.op = chars.iter().collect();
                    }
                }
            }
            Batch { .. } => {}
        }
        Ok(())
    }

    fn undo_impl(&mut self, state: &mut SavedState) -> redo::Result<Self> {
        use SavedStateCommand::*;
        match self {
            RandomizeNodeIds { previous_ids } => {
                state.nodes.iter_mut().for_each(|node| {
                    if let Some(&id) = previous_ids.get(&node.id) {
                        node.id = id;
                    } else {
                        node.id = random();
                    }
                });
            }
            MoveCursor { offset } => {
                state.move_cursor(-*offset);
            }
            MoveNodesAndCursor {
                ids,
                nodes_offset,
                cursor_offset,
            } => {
                state.move_nodes(ids, -*nodes_offset);
                state.move_cursor(-*cursor_offset);
            }
            DeleteNodes { nodes, .. } => {
                state.nodes.extend(nodes.drain(..));
            }
            InsertNode { id, node } => {
                if let Some(ix) = state.nodes.iter().position(|node| node.id == *id) {
                    *node = Some(state.nodes.swap_remove(ix));
                }
            }
            Splash { cursor_offset, ids } => {
                state.move_cursor(-*cursor_offset);
                state.move_nodes(ids, Position::x(-1));
            }
            InsertLine { cursor_offset, ids } => {
                state.move_cursor(-*cursor_offset);
                state.move_nodes(ids, Position::y(-1));
            }
            CutOp {
                id,
                ids,
                node,
                nodes_offset,
                tail,
            } => {
                if let Some(id) = id {
                    if let Some(node) = state.nodes.iter_mut().find(|node| node.id == *id) {
                        node.draft = true;
                        node.op.push_str(tail);
                    } else if let Some(node) = node.take() {
                        state.nodes.push(node);
                    }
                    state.move_nodes(ids, -*nodes_offset);
                }
            }
            ReplaceOp { id, op } => {
                if let Some(node) = state.nodes.iter_mut().find(|node| node.id == *id) {
                    node.draft = true;
                    *op = std::mem::replace(&mut node.op, op.to_owned());
                }
            }
            InsertSpace { ids } => {
                let offset = Position::x(-1);
                state.move_nodes(ids, offset);
                state.move_cursor(offset);
            }
            InsertChar { id, ids, ix, .. } => {
                if let Some(id) = id {
                    let mut remove_node = false;
                    if let Some(node) = state.nodes.iter_mut().find(|node| node.id == *id) {
                        if node.op.chars().count() > 1 {
                            let mut chars: Vec<_> = node.op.chars().collect();
                            chars.remove(*ix);
                            node.op = chars.iter().collect();
                            node.draft = true;
                        } else {
                            remove_node = true;
                        }
                        state.move_nodes(ids, Position::x(-1));
                        state.move_cursor(Position::x(-1));
                    }
                    if remove_node {
                        state.nodes.retain(|node| node.id != *id)
                    }
                }
            }
            DeleteChar { char, id, ids, ix } => {
                let offset = Position::x(1);
                state.move_nodes(ids, offset);
                state.move_cursor(offset);
                if let Some(id) = id {
                    if let Some(node) = state.nodes.iter_mut().find(|node| node.id == *id) {
                        let mut chars: Vec<_> = node.op.chars().collect();
                        chars.insert(*ix, char.unwrap());
                        node.op = chars.iter().collect();
                        node.draft = true;
                    }
                }
            }
            Batch { .. } => {}
        }
        Ok(())
    }
}

impl Command for SavedStateCommand {
    type Target = SavedState;
    type Error = &'static str;

    fn apply(&mut self, state: &mut SavedState) -> redo::Result<Self> {
        match self {
            SavedStateCommand::Batch { commands } => {
                for command in commands {
                    command.apply_impl(state)?;
                }
                Ok(())
            }
            command => command.apply_impl(state),
        }
    }

    fn undo(&mut self, state: &mut SavedState) -> redo::Result<Self> {
        match self {
            SavedStateCommand::Batch { commands } => {
                for command in commands.iter_mut().rev() {
                    command.undo_impl(state)?;
                }
                Ok(())
            }
            command => command.undo_impl(state),
        }
    }

    fn merge(&mut self, other: Self) -> redo::Merge<Self> {
        use redo::Merge::*;
        use SavedStateCommand::*;
        match self {
            RandomizeNodeIds { previous_ids: _ } => match other {
                // TODO make transitive
                // RandomizeNodeIds {
                //     previous_ids: other,
                // } => {
                //     *previous_ids = other;
                //     Yes
                // }
                _ => No(other),
            },
            MoveCursor { offset } => match other {
                MoveCursor { offset: other } => {
                    *offset = *offset + other;
                    Yes
                }
                _ => No(other),
            },
            MoveNodesAndCursor {
                ids,
                nodes_offset,
                cursor_offset,
            } => match other {
                MoveNodesAndCursor {
                    ids: other_ids,
                    nodes_offset: other_nodes_offset,
                    cursor_offset: other_cursor_offset,
                } if other_ids == *ids => {
                    *nodes_offset = *nodes_offset + other_nodes_offset;
                    *cursor_offset = *cursor_offset + other_cursor_offset;
                    Yes
                }
                _ => No(other),
            },
            InsertChar { .. } | DeleteChar { .. } => match other {
                InsertChar { .. } | DeleteChar { .. } => {
                    let batch = Batch {
                        commands: Vec::new(),
                    };
                    let prev_self = std::mem::replace(self, batch);
                    if let Batch { commands } = self {
                        commands.push(prev_self);
                        commands.push(other);
                    }
                    Yes
                }
                _ => No(other),
            },
            Batch { commands } => match other {
                InsertChar { .. } | DeleteChar { .. } => {
                    commands.push(other);
                    Yes
                }
                _ => No(other),
            },
            _ => No(other),
        }
    }
}
