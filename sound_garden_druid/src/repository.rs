use crate::{canvas::Cursor, types::*};
use anyhow::Result;
use crdt_engine::{Delta, Engine, Patch, Rope, ValueOp};
use druid::Point;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    convert::TryFrom,
};

/// Repository is a source of truth for a distributed state in SGD.
/// We encode text part of that state in a tagged text form and then treat changes to it as text edits.
/// Extra care should be taken to preserve structure during concurrent edits.
///
/// The rest of the state is represented as key-value storage as representing its change as text edits
/// doesn't quite work for atomic values.
/// Consider the following scenario:
/// * User A moved node into position x=10.
/// * User A moved node into position x=20.
/// * User B moved node into position x=30.
/// * User A called undo for his move.
/// This would set x=1030 which is definitely not what we want.
///
/// Nodes text format is
/// ```
/// fs    <- field_separator  <- '\t'
/// rs    <- record_separator <- '\n'
/// id    <- [0-9a-f]{16}
/// node  <- id fs text
/// nodes <- (node rs)*
/// ```
#[derive(Serialize, Deserialize)]
pub struct NodeRepository {
    engine: Engine<MetaKey, MetaValue>,
}

pub enum NodeEdit {
    Move(Point),
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

    pub fn apply(&mut self, patch: Patch<MetaKey, MetaValue>) {
        self.engine.apply(patch);
    }

    pub fn add_node(&mut self, node: Node, color: u64) -> Patch<MetaKey, MetaValue> {
        let offset = self.engine.text().len_chars();
        self.engine.edit(&[
            Delta::Text {
                range: (offset, offset),
                new_text: format!("{}\t{}\n", String::from(node.id), node.text),
                color,
            },
            Delta::Value {
                op: ValueOp::Set(
                    MetaKey::Position(node.id),
                    MetaValue::Position(node.position.x as _, node.position.y as _),
                ),
                color,
            },
        ])
    }

    pub fn delete_nodes(&mut self, ids: &[Id], color: u64) -> Patch<MetaKey, MetaValue> {
        let mut deltas = ids
            .iter()
            .map(|id| Delta::Value {
                op: ValueOp::Remove(MetaKey::Position(*id)),
                color,
            })
            .collect::<Vec<_>>();
        let ids = ids.into_iter().map(String::from).collect::<HashSet<_>>();
        let code = self.engine.text();
        deltas.extend(
            code.lines()
                .map(String::from)
                .enumerate()
                .filter_map(|(line, record)| {
                    record.split('\t').next().and_then(|id| {
                        if ids.contains(id) {
                            let start = code.line_to_char(line);
                            let end = code.line_to_char(line + 1);
                            Some(Delta::Text {
                                range: (start, end),
                                new_text: String::new(),
                                color,
                            })
                        } else {
                            None
                        }
                    })
                }),
        );
        self.engine.edit(&deltas)
    }

    pub fn edit_nodes(
        &mut self,
        edits: HashMap<Id, Vec<NodeEdit>>,
        color: u64,
    ) -> Patch<MetaKey, MetaValue> {
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
                            edits.into_iter().map(move |edit| match edit {
                                NodeEdit::Move(p) => Delta::Value {
                                    op: ValueOp::Set(
                                        MetaKey::Position(id),
                                        MetaValue::Position(p.x as _, p.y as _),
                                    ),
                                    color,
                                },
                                NodeEdit::Edit { start, end, text } => {
                                    // REVIEW Optimization opportunity: id field has fixed width.
                                    let text_offset = record
                                        .chars()
                                        .enumerate()
                                        .find_map(|(i, c)| if c == '\t' { Some(i) } else { None })
                                        .unwrap();
                                    let offset = line_offset + text_offset + 1;
                                    Delta::Text {
                                        range: (offset + start, offset + end),
                                        new_text: text.to_owned(),
                                        color,
                                    }
                                }
                            })
                        })
                    })
            })
            .flatten()
            .collect::<Vec<_>>();
        self.engine.edit(&deltas)
    }

    pub fn undo(&mut self) -> Option<Patch<MetaKey, MetaValue>> {
        self.engine.undo()
    }

    pub fn redo(&mut self) -> Option<Patch<MetaKey, MetaValue>> {
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
    pub fn save(&self, filename: &str) -> Result<()> {
        std::fs::File::create(&filename)
            .or_else(|e| anyhow::bail!(e))
            .and_then(|f| {
                serde_cbor::to_writer(snap::write::FrameEncoder::new(f), self)
                    .or_else(|e| anyhow::bail!(e))
            })
            .map(|_| ())
    }

    pub fn nodes(&self) -> Vec<Node> {
        let meta = self.engine.meta();
        let mut nodes = self
            .engine
            .text()
            .lines()
            .filter_map(|line| {
                if let [id, text] = String::from(line).trim().split('\t').collect::<Vec<_>>()[..] {
                    let id = Id::try_from(id).unwrap();
                    if let Some(&MetaValue::Position(x, y)) = meta.get(&MetaKey::Position(id)) {
                        Some(Node {
                            id,
                            position: Point::new(x as _, y as _),
                            text: text.to_string(),
                        })
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        nodes.sort_unstable_by_key(|node| (node.position.y as i64, node.position.x as i64));
        nodes
    }

    pub fn text(&self) -> &Rope {
        self.engine.text()
    }

    pub fn meta(&self) -> &HashMap<MetaKey, MetaValue> {
        self.engine.meta()
    }

    pub fn get_cursor(&self) -> Cursor {
        let k = MetaKey::Cursor(self.engine.session_id());
        let position = self
            .engine
            .meta()
            .get(&k)
            .and_then(|v| match v {
                &MetaValue::Position(x, y) => Some(Point::new(x as _, y as _)),
                // _ => None,
            })
            .unwrap_or_default();
        Cursor { position }
    }

    pub fn set_cursor(&mut self, cursor: &Cursor, color: u64) -> Patch<MetaKey, MetaValue> {
        let k = MetaKey::Cursor(self.engine.session_id());
        let p = cursor.position;
        self.engine.edit(&[Delta::Value {
            op: ValueOp::Set(k, MetaValue::Position(p.x as _, p.y as _)),
            color,
        }])
    }
}
