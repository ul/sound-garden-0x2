use anyhow::Result;
use audio_program::{get_help, get_op_groups, Context, TextOp};
use itertools::Itertools;
use rand::prelude::*;
use redo::{Command, Record};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::HashMap;

pub const MIN_X: i16 = 2;
pub const MIN_Y: i16 = 2;

pub struct App {
    pub ctx: Context,
    pub cycles: Vec<Vec<String>>,
    pub help_scroll: u16,
    pub input_mode: InputMode,
    pub op_groups: Vec<(String, Vec<String>)>,
    pub op_help: HashMap<String, String>,
    pub ops: Vec<TextOp>,
    pub play: bool,
    pub recording: bool,
    pub screen: Screen,
    pub status: String,
    saved_state: Record<SavedStateCommand>,
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
    Replace,
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
    // TODO Atomic write.
    pub fn save<P: AsRef<std::path::Path>>(&mut self, path: P) -> Result<()> {
        let f = std::fs::File::create(path)?;
        serde_json::to_writer_pretty(f, &self.saved_state)?;
        self.saved_state.set_saved(true);
        Ok(())
    }

    pub fn load<P: AsRef<std::path::Path>>(path: P) -> Result<Self> {
        let f = std::fs::File::open(path)?;
        Ok(Self {
            saved_state: serde_json::from_reader(f)?,
            ..Default::default()
        })
    }

    pub fn undo(&mut self) {
        self.saved_state.undo();
    }

    pub fn redo(&mut self) {
        self.saved_state.undo();
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

    pub fn replace_mode(&mut self) {
        self.input_mode = InputMode::Replace;
    }

    pub fn randomize_node_ids(&mut self) {
        self.saved_state.apply(SavedStateCommand::RandomizeNodeIds {
            previous_ids: Default::default(),
        });
    }

    pub fn move_cursor(&mut self, offset: Position) {
        self.saved_state
            .apply(SavedStateCommand::MoveCursor { offset });
        self.update_status();
    }

    pub fn move_nodes(&mut self, ids: Vec<u64>, offset: Position) {
        self.saved_state
            .apply(SavedStateCommand::MoveNodes { ids, offset });
        self.update_status();
    }

    pub fn delete_nodes(&mut self, ids: Vec<u64>) {
        self.saved_state.apply(SavedStateCommand::DeleteNodes {
            ids,
            nodes: Vec::new(),
        });
        self.update_status();
    }

    pub fn insert_char(&mut self, id: u64, ix: usize, char: char) {
        self.saved_state
            .apply(SavedStateCommand::InsertChar { id, ix, char });
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

    pub fn sort_nodes(&mut self) {
        let target = self.saved_state.target();
        target.nodes.sort_by_key(|node| node.position);
        target.program = target.nodes.iter().map(|node| node.op.to_owned()).join(" ");
    }

    pub fn undraft(&mut self) {
        self.saved_state
            .target()
            .nodes
            .iter_mut()
            .for_each(|node| node.draft = false);
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

impl Default for App {
    fn default() -> Self {
        App {
            ctx: Default::default(),
            cycles: default_cycles(),
            help_scroll: 0,
            input_mode: Default::default(),
            op_groups: get_op_groups(),
            op_help: get_help(),
            ops: Default::default(),
            play: Default::default(),
            recording: Default::default(),
            saved_state: Default::default(),
            screen: Default::default(),
            status: Default::default(),
        }
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
    RandomizeNodeIds { previous_ids: HashMap<u64, u64> },
    MoveCursor { offset: Position },
    MoveNodes { ids: Vec<u64>, offset: Position },
    DeleteNodes { ids: Vec<u64>, nodes: Vec<Node> },
    InsertChar { id: u64, ix: usize, char: char },
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
        }
        Ok(())
    }

    fn undo(&mut self, state: &mut SavedState) -> redo::Result<Self> {
        use SavedStateCommand::*;
        match self {
            RandomizeNodeIds { previous_ids } => {
                state.nodes.iter_mut().for_each(|node| {
                    node.id = *previous_ids.get(&node.id).unwrap_or_else(|| &random());
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
            DeleteNodes { ids, nodes } => {
                state.nodes.extend(nodes.drain(..));
            }
            InsertChar { id, ix, .. } => {
                if let Some(node) = state.nodes.iter_mut().find(|node| node.id == *id) {
                    let mut chars: Vec<_> = node.op.chars().collect();
                    chars.remove(*ix);
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
