use anyhow::Result;
use audio_program::{compile_program, get_help, get_op_groups, Context, TextOp};
use audio_vm::VM;
use itertools::Itertools;
use rand::prelude::*;
use redo::{Command, Record};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub const MIN_X: i16 = 2;
pub const MIN_Y: i16 = 2;

// TODO make less pub
pub struct App {
    pub ctx: Context,
    pub cycles: Vec<Vec<String>>,
    pub help_scroll: u16,
    pub input_mode: InputMode,
    pub op_groups: Vec<(String, Vec<String>)>,
    pub op_help: HashMap<String, String>,
    pub play: bool,
    pub recording: bool,
    pub screen: Screen,
    pub status: String,
    filename: String,
    sample_rate: u32,
    saved_state: Record<SavedStateCommand>,
    vm: Arc<Mutex<VM>>,
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
    pub fn new(filename: String, vm: Arc<Mutex<VM>>, sample_rate: u32) -> Self {
        App {
            ctx: Default::default(),
            cycles: default_cycles(),
            filename,
            help_scroll: 0,
            input_mode: Default::default(),
            op_groups: get_op_groups(),
            op_help: get_help(),
            play: Default::default(),
            recording: Default::default(),
            sample_rate,
            saved_state: Default::default(),
            screen: Default::default(),
            status: Default::default(),
            vm,
        }
    }

    // TODO Atomic write.
    pub fn save(&mut self) -> Result<()> {
        let target = &mut self.saved_state.target_mut();
        target.nodes.iter_mut().for_each(|node| node.draft = false);
        // TODO mv to load_program
        target.nodes.sort_by_key(|node| node.position);
        target.program = target.nodes.iter().map(|node| node.op.to_owned()).join(" ");

        let f = std::fs::File::create(&self.filename)?;
        serde_json::to_writer_pretty(f, &self.saved_state)?;
        self.saved_state.set_saved(true);
        Ok(())
    }

    pub fn load(path: &str, vm: Arc<Mutex<VM>>, sample_rate: u32) -> Self {
        let mut app = Self::new(path.to_owned(), vm, sample_rate);
        if let Ok(f) = std::fs::File::open(path) {
            if let Ok(saved_state) = serde_json::from_reader(f) {
                app.saved_state = saved_state;
            }
        }
        app.load_program();
        app
    }

    pub fn commit(&mut self) {
        self.save().ok();
        self.load_program();
    }

    pub fn undo(&mut self) {
        self.saved_state.undo().ok();
    }

    pub fn redo(&mut self) {
        self.saved_state.undo().ok();
    }

    pub fn play(&mut self) {
        self.vm.lock().unwrap().play();
    }

    pub fn pause(&mut self) {
        self.vm.lock().unwrap().pause();
    }

    pub fn draft(&self) -> bool {
        !self.saved_state.is_saved()
            || self
                .saved_state
                .target()
                .nodes
                .iter()
                .any(|node| node.draft)
    }

    pub fn input_mode(&self) -> &InputMode {
        &self.input_mode
    }

    pub fn normal_mode(&mut self) {
        // TODO cleanup empty nodes
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
        self.update_status();
    }

    pub fn move_nodes(&mut self, ids: Vec<u64>, offset: Position) {
        self.saved_state
            .apply(SavedStateCommand::MoveNodes { ids, offset })
            .ok();
        self.update_status();
    }

    pub fn delete_nodes(&mut self, ids: Vec<u64>) {
        self.saved_state
            .apply(SavedStateCommand::DeleteNodes {
                ids,
                nodes: Vec::new(),
            })
            .ok();
        self.update_status();
    }

    pub fn insert_char(&mut self, id: u64, ix: usize, char: char) {
        self.saved_state
            .apply(SavedStateCommand::InsertChar { id, ix, char })
            .ok();
        self.update_status();
    }

    pub fn delete_char(&mut self, id: u64, ix: usize) {
        self.saved_state
            .apply(SavedStateCommand::DeleteChar { id, ix, char: None })
            .ok();
        self.update_status();
    }

    pub fn insert_node(&mut self, node: Node) {
        self.saved_state
            .apply(SavedStateCommand::InsertNode {
                id: node.id,
                node: Some(node),
            })
            .ok();
        self.update_status();
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

    pub fn node_at_cursor(&self) -> Option<usize> {
        self.saved_state.target().node_at_cursor()
    }

    fn update_status(&mut self) {
        self.status = String::new();
        if let Some(ix) = self.node_at_cursor() {
            if let Some(help) = self
                .op_help
                .get(self.nodes()[ix].op.split(':').next().unwrap())
            {
                self.status = help.to_owned();
            }
        }
    }

    fn load_program(&mut self) {
        let program = compile_program(&self.ops(), self.sample_rate, &mut self.ctx);
        // Ensure the smallest possible scope to limit locking time.
        let garbage = {
            self.vm.lock().unwrap().load_program(program);
        };
        drop(garbage);
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
    pub fn node_at_cursor(&self) -> Option<usize> {
        self.nodes.iter().position(
            |Node {
                 position: Position { y, x },
                 op,
                 ..
             }| {
                *y == self.cursor.y
                    && *x <= self.cursor.x
                    // space after node is counted as a part of the node
                    && self.cursor.x <= *x + op.len() as i16
            },
        )
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

//-----------------------------------------------------------------------------

#[derive(Serialize, Deserialize)]
enum SavedStateCommand {
    DeleteChar {
        id: u64,
        ix: usize,
        char: Option<char>,
    },
    DeleteNodes {
        ids: Vec<u64>,
        nodes: Vec<Node>,
    },
    InsertChar {
        id: u64,
        ix: usize,
        char: char,
    },
    InsertNode {
        id: u64,
        node: Option<Node>,
    },
    MoveCursor {
        offset: Position,
    },
    MoveNodes {
        ids: Vec<u64>,
        offset: Position,
    },
    RandomizeNodeIds {
        previous_ids: HashMap<u64, u64>,
    },
}

impl Command for SavedStateCommand {
    type Target = SavedState;
    type Error = &'static str;

    fn apply(&mut self, state: &mut SavedState) -> redo::Result<Self> {
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
                state.cursor.x += offset.x;
                state.cursor.y += offset.y;
            }
            MoveNodes { ids, offset } => {
                for node in state.nodes.iter_mut().filter(|node| ids.contains(&node.id)) {
                    node.position.x += offset.x;
                    node.position.y += offset.y;
                }
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
            InsertChar { id, ix, char } => {
                if let Some(node) = state.nodes.iter_mut().find(|node| node.id == *id) {
                    let mut chars: Vec<_> = node.op.chars().collect();
                    chars.insert(*ix, *char);
                    node.op = chars.iter().join("");
                }
            }
            InsertNode { node, .. } => {
                if let Some(node) = node.take() {
                    state.nodes.push(node);
                }
            }
            DeleteChar { id, ix, char } => {
                if let Some(node) = state.nodes.iter_mut().find(|node| node.id == *id) {
                    let mut chars: Vec<_> = node.op.chars().collect();
                    *char = Some(chars.remove(*ix));
                    node.op = chars.iter().join("");
                }
            }
        }
        Ok(())
    }

    fn undo(&mut self, state: &mut SavedState) -> redo::Result<Self> {
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
                state.cursor.x -= offset.x;
                state.cursor.y -= offset.y;
            }
            MoveNodes { ids, offset } => {
                for node in state.nodes.iter_mut().filter(|node| ids.contains(&node.id)) {
                    node.position.x -= offset.x;
                    node.position.y -= offset.y;
                }
            }
            DeleteNodes { nodes, .. } => {
                state.nodes.extend(nodes.drain(..));
            }
            InsertChar { id, ix, .. } => {
                if let Some(node) = state.nodes.iter_mut().find(|node| node.id == *id) {
                    let mut chars: Vec<_> = node.op.chars().collect();
                    chars.remove(*ix);
                    node.op = chars.iter().join("");
                }
            }
            InsertNode { id, node } => {
                if let Some(ix) = state.nodes.iter().position(|node| node.id == *id) {
                    *node = Some(state.nodes.swap_remove(ix));
                }
            }
            DeleteChar { id, ix, char } => {
                if let Some(node) = state.nodes.iter_mut().find(|node| node.id == *id) {
                    let mut chars: Vec<_> = node.op.chars().collect();
                    chars.insert(*ix, char.unwrap());
                    node.op = chars.iter().join("");
                }
            }
        }
        Ok(())
    }

    fn merge(&mut self, other: Self) -> redo::Merge<Self> {
        use redo::Merge::*;
        use SavedStateCommand::*;
        match self {
            RandomizeNodeIds { previous_ids } => match other {
                RandomizeNodeIds {
                    previous_ids: other,
                } => {
                    // TODO make transitive
                    *previous_ids = other;
                    Yes
                }
                _ => No(other),
            },
            MoveCursor { offset } => match other {
                MoveCursor { offset: other } => {
                    offset.x += other.x;
                    offset.y += other.y;
                    Yes
                }
                _ => No(other),
            },
            _ => No(other),
        }
    }
}

/*
pub struct Cut(usize, Option<String>);

impl Command for Cut {
    fn apply(&mut self, node: &mut Node) -> undo::Result {
        self.1 = Some(node.op.to_string());
        node.op = node.op.chars().take(self.0).join("");
        Ok(())
    }

    fn undo(&mut self, node: &mut Node) -> undo::Result {
        node.op = self.1.take().unwrap_or_else(|| node.op.to_string());
        Ok(())
    }
}

*/
