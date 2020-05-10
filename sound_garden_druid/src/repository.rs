use crate::types::*;
use crdt_engine::{Delta, Engine, Patch};
use druid::Point;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, convert::TryFrom};

/// Repository is a source of truth for a distributed state in SGD.
/// We encode that state in a text form and then treat changes to it as text edits.
/// Extra care should be taken to preserve structure during concurrent edits.
/// Nodes format is
/// ```
/// fs    <- field_separator  <- '\t'
/// rs    <- record_separator <- '\n'
/// id    <- [0-9a-f]{16}
/// x, y  <- [0-9]+
/// node  <- id fs x fs y fs text
/// nodes <- (node rs)*
/// ```
#[derive(Serialize, Deserialize)]
pub struct NodeRepository {
    engine: Engine,
}

pub enum NodeEdit {
    MoveX(f64),
    MoveY(f64),
    Edit {
        start: usize,
        end: usize,
        text: String,
    },
}

impl NodeRepository {
    pub fn new() -> Self {
        NodeRepository {
            engine: Engine::new(),
        }
    }

    /// NOTE Always call this function after repository deserialization.
    /// TODO Enforce.
    pub fn thaw(&mut self) {
        self.engine.rebuild();
    }

    pub fn apply(&mut self, patch: Patch) {
        self.engine.apply(patch);
    }

    pub fn add_node(&mut self, node: Node, color: u64) -> Patch {
        let offset = self.engine.text().len_chars();
        self.engine.edit(vec![Delta {
            range: (offset, offset),
            new_text: format!(
                "{}\t{}\t{}\t{}\n",
                String::from(node.id),
                node.position.x,
                node.position.y,
                node.text
            ),
            color,
        }])
    }

    pub fn delete_node(&mut self, id: Id, color: u64) -> Option<Patch> {
        let id = String::from(id);
        let code = self.engine.text();
        code.lines()
            .map(String::from)
            .enumerate()
            .find_map(|(line, record)| {
                if record.starts_with(&id) {
                    let start = code.line_to_char(line);
                    let end = code.line_to_char(line + 1);
                    Some(Delta {
                        range: (start, end),
                        new_text: String::new(),
                        color,
                    })
                } else {
                    None
                }
            })
            .map(|delta| self.engine.edit(vec![delta]))
    }

    pub fn edit_nodes(&mut self, edits: HashMap<Id, Vec<NodeEdit>>, color: u64) -> Patch {
        let code = self.engine.text();
        let deltas = code
            .lines()
            .map(String::from)
            .enumerate()
            .filter_map(|(line, record)| {
                let line_offset = code.line_to_char(line);
                record
                    .split('\t')
                    .next()
                    .and_then(|id| Id::try_from(id).ok())
                    .and_then(|id| {
                        edits.get(&id).map(|edits| {
                            edits.into_iter().map(move |edit| {
                                let mut field_pos =
                                    record.chars().enumerate().filter_map(|(i, c)| {
                                        if c == '\t' {
                                            Some(i)
                                        } else {
                                            None
                                        }
                                    });
                                match edit {
                                    NodeEdit::MoveX(x) => {
                                        let start = field_pos.next().unwrap() + 1;
                                        let end = field_pos.next().unwrap();
                                        Delta {
                                            range: (line_offset + start, line_offset + end),
                                            new_text: format!("{}", x),
                                            color,
                                        }
                                    }
                                    NodeEdit::MoveY(y) => {
                                        field_pos.next();
                                        let start = field_pos.next().unwrap() + 1;
                                        let end = field_pos.next().unwrap();
                                        Delta {
                                            range: (line_offset + start, line_offset + end),
                                            new_text: format!("{}", y),
                                            color,
                                        }
                                    }
                                    NodeEdit::Edit { start, end, text } => {
                                        field_pos.next();
                                        field_pos.next();
                                        let offset = line_offset + field_pos.next().unwrap() + 1;
                                        Delta {
                                            range: (offset + start, offset + end),
                                            new_text: text.to_owned(),
                                            color,
                                        }
                                    }
                                }
                            })
                        })
                    })
            })
            .flatten()
            .collect::<Vec<_>>();
        self.engine.edit(deltas)
    }

    pub fn undo(&mut self) -> Option<Patch> {
        self.engine.undo()
    }

    pub fn redo(&mut self) -> Option<Patch> {
        self.engine.redo()
    }

    pub fn load(filename: &str) -> Self {
        std::fs::File::open(filename)
            .ok()
            .map(|f| snap::read::FrameDecoder::new(f))
            .and_then(|f| serde_cbor::from_reader::<Self, _>(f).ok())
            .map(|mut node_repo| {
                node_repo.thaw();
                node_repo
            })
            .unwrap_or_else(|| Self::new())
    }

    // TODO Atomic write.
    pub fn save(&self, filename: &str) {
        std::fs::File::create(&filename)
            .ok()
            .and_then(|f| serde_cbor::to_writer(snap::write::FrameEncoder::new(f), self).ok());
    }

    pub fn nodes(&self) -> Vec<Node> {
        let mut nodes = self
            .engine
            .text()
            .lines()
            .filter_map(|line| {
                if let [id, x, y, text] =
                    String::from(line).trim().split('\t').collect::<Vec<_>>()[..]
                {
                    Some(Node {
                        id: Id::try_from(id).unwrap(),
                        position: Point::new(x.parse().unwrap(), y.parse().unwrap()),
                        text: text.to_string(),
                    })
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        nodes.sort_unstable_by_key(|node| (node.position.y as i64, node.position.x as i64));
        nodes
    }
}
