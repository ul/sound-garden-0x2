//! A CRDT-based engine for collaborative text editing with undo support.
//
// Undo/redo edits are shared but undo stack is local.

use crdts::{orswot::Op, CmRDT, CvRDT, Orswot};
use rand::random;
pub use ropey::Rope;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};

/// Beginning of Text
const BOT: CharId = CharId(0);
/// End of Text
const EOT: CharId = CharId(1);

// TODO Ensure skipped fields are in-sync after deserialization.
#[derive(Clone, Serialize, Deserialize)]
pub struct Engine {
    id: SessionId,
    chars: Orswot<(CharId, char), SessionId>,
    edges: Orswot<(CharId, CharId), SessionId>,
    edits: Orswot<Edit, SessionId>,
    undone: Orswot<UndoGroupId, SessionId>,
    undo_groups: Vec<UndoGroupId>,
    redo_groups: Vec<UndoGroupId>,
    #[serde(skip)]
    last_color: u64,
    #[serde(skip)]
    text: Rope,
    // REVIEW Consider BTreeMap as we edit it frequently.
    #[serde(skip)]
    text_ids: Vec<CharId>,
}

pub struct Delta {
    pub range: (usize, usize),
    pub new_text: String,
    // When color changes we start a new undo group.
    pub color: u64,
}

#[derive(Clone, Serialize, Deserialize)]
pub enum Patch {
    Edit {
        chars: Vec<Op<(CharId, char), SessionId>>,
        edges: Vec<Op<(CharId, CharId), SessionId>>,
        edits: Vec<Op<Edit, SessionId>>,
    },
    Undo {
        undone: Op<UndoGroupId, SessionId>,
    },
}

#[derive(Clone, Copy, Debug, Hash, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct CharId(u64);

#[derive(Clone, Copy, Debug, Hash, Serialize, Deserialize, PartialEq, Eq, PartialOrd)]
pub struct UndoGroupId(u64);

#[derive(Clone, Debug, Hash, Serialize, Deserialize, PartialEq, Eq)]
pub struct Edit {
    undo_group: UndoGroupId,
    inserts: Vec<CharId>,
    deletes: Vec<CharId>,
}

#[derive(Clone, Copy, Debug, Hash, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct SessionId(u64);

impl Engine {
    pub fn new() -> Self {
        Engine {
            id: SessionId::generate(),
            chars: Default::default(),
            edges: Default::default(),
            edits: Default::default(),
            undone: Default::default(),
            text: Default::default(),
            text_ids: Default::default(),
            undo_groups: vec![UndoGroupId::generate()],
            redo_groups: Default::default(),
            last_color: 0,
        }
    }

    pub fn text(&self) -> &Rope {
        &self.text
    }

    // Deltas must be sorted by increasing range start
    // and not have overlapping ranges.
    pub fn edit(&mut self, deltas: &[Delta]) -> Patch {
        self.redo_groups.clear();
        let mut chars = Vec::new();
        let mut edges = Vec::new();
        let mut edits = Vec::new();
        let mut edit = Edit {
            undo_group: self
                .undo_groups
                .last()
                .copied()
                .unwrap_or_else(|| UndoGroupId::generate()),
            inserts: Default::default(),
            deletes: Default::default(),
        };
        for delta in deltas {
            // Start a new undo group if color changed.
            if delta.color != self.last_color {
                if !edit.inserts.is_empty() || !edit.deletes.is_empty() {
                    let op = self
                        .edits
                        .add(edit, self.edits.read().derive_add_ctx(self.id));
                    self.edits.apply(op.clone());
                    edits.push(op);
                }
                self.last_color = delta.color;
                let new_group = UndoGroupId::generate();
                self.undo_groups.push(new_group);
                edit = Edit {
                    undo_group: new_group,
                    inserts: Default::default(),
                    deletes: Default::default(),
                };
            }
            let range = delta.range.0..delta.range.1;

            self.text.remove(range.clone());
            self.text.insert(range.start, &delta.new_text);

            let ids = std::iter::repeat_with(CharId::generate)
                .take(delta.new_text.chars().count())
                .collect::<Vec<_>>();

            let char_before = if range.start > 0 {
                self.text_ids[range.start - 1]
            } else {
                BOT
            };
            let char_after = if range.end < self.text_ids.len() {
                self.text_ids[range.end]
            } else {
                EOT
            };

            edit.deletes
                .extend(self.text_ids.splice(range.clone(), ids.iter().copied()));

            let previous_ids = std::iter::once(char_before).chain(ids.iter().copied());
            let next_ids = ids
                .iter()
                .copied()
                .skip(1)
                .chain(std::iter::once(char_after));

            for (((id, char), previous), next) in ids
                .iter()
                .copied()
                .zip(delta.new_text.chars())
                .zip(previous_ids)
                .zip(next_ids)
            {
                // Register insertion.
                edit.inserts.push(id);
                // Register character.
                let op = self
                    .chars
                    .add((id, char), self.chars.read().derive_add_ctx(self.id));
                self.chars.apply(op.clone());
                chars.push(op);
                // Register edge from the previous character.
                if previous != BOT {
                    let op = self
                        .edges
                        .add((previous, id), self.edges.read().derive_add_ctx(self.id));
                    self.edges.apply(op.clone());
                    edges.push(op);
                }
                // Register edge to the next character.
                if next != EOT {
                    let op = self
                        .edges
                        .add((id, next), self.edges.read().derive_add_ctx(self.id));
                    self.edges.apply(op.clone());
                    edges.push(op);
                }
            }
        }
        if !edit.inserts.is_empty() || !edit.deletes.is_empty() {
            let op = self
                .edits
                .add(edit, self.edits.read().derive_add_ctx(self.id));
            self.edits.apply(op.clone());
            edits.push(op);
        }
        Patch::Edit {
            chars,
            edits,
            edges,
        }
    }

    pub fn undo(&mut self) -> Option<Patch> {
        if let Some(id) = self.undo_groups.pop() {
            self.last_color = 0;
            self.redo_groups.push(id);
            let add_ctx = self.undone.read().derive_add_ctx(self.id);
            let undone = self.undone.add(id, add_ctx);
            self.undone.apply(undone.clone());
            self.rebuild();
            Some(Patch::Undo { undone })
        } else {
            None
        }
    }

    pub fn redo(&mut self) -> Option<Patch> {
        if let Some(id) = self.redo_groups.pop() {
            self.last_color = 0;
            self.undo_groups.push(id);
            let rm_ctx = self.undone.read().derive_rm_ctx();
            let undone = self.undone.rm(id, rm_ctx);
            self.undone.apply(undone.clone());
            self.rebuild();
            Some(Patch::Undo { undone })
        } else {
            None
        }
    }

    pub fn apply(&mut self, patch: Patch) {
        match patch {
            Patch::Edit {
                chars,
                edits,
                edges,
            } => {
                for op in chars {
                    self.chars.apply(op);
                }
                for op in edits {
                    self.edits.apply(op);
                }
                for op in edges {
                    self.edges.apply(op);
                }
            }
            Patch::Undo { undone } => {
                self.undone.apply(undone);
            }
        }
        self.rebuild();
    }

    pub fn merge(&mut self, other: Self) {
        self.chars.merge(other.chars);
        self.edits.merge(other.edits);
        self.edges.merge(other.edges);
        self.undone.merge(other.undone);
        self.rebuild();
    }

    pub fn rebuild(&mut self) {
        // Not tracking inserts as their union contains all chars.
        let mut deletes = HashSet::new();
        let undone = self.undone.read().val;
        for edit in self.edits.read().val {
            if undone.contains(&edit.undo_group) {
                deletes.extend(edit.inserts);
            } else {
                deletes.extend(edit.deletes);
            }
        }
        let mut text = String::new();
        let mut text_ids = Vec::new();
        let chars = self.chars.read().val;
        let mut id_to_char = chars.iter().copied().collect::<HashMap<_, _>>();
        let edges = self.edges.read().val;
        let mut incoming_edges = HashMap::new();
        let mut outgoing_edges = HashMap::new();
        for (from, to) in edges {
            incoming_edges.entry(to).or_insert_with(Vec::new).push(from);
            outgoing_edges.entry(from).or_insert_with(Vec::new).push(to);
        }
        let mut queue = chars
            .iter()
            .filter_map(|(id, _)| {
                if !incoming_edges.contains_key(id) {
                    Some(id)
                } else {
                    None
                }
            })
            .copied()
            .collect::<VecDeque<_>>();
        // TODO Stable tie-breaking.
        while let Some(id) = queue.pop_front() {
            if !deletes.contains(&id) {
                text_ids.push(id);
                text.push(id_to_char.remove(&id).unwrap());
            }
            for next in outgoing_edges.remove(&id).unwrap_or_default() {
                let incoming = incoming_edges.get_mut(&next).unwrap();
                incoming.retain(|&prev| prev != id);
                if incoming.is_empty() {
                    queue.push_back(next);
                }
            }
        }
        self.text = Rope::from(text);
        self.text_ids = text_ids;
    }
}

impl SessionId {
    fn generate() -> Self {
        Self(random())
    }
}

impl CharId {
    fn generate() -> Self {
        let mut id = random();
        while id == BOT.0 || id == EOT.0 {
            id = random();
        }
        Self(id)
    }
}

impl UndoGroupId {
    fn generate() -> Self {
        Self(random())
    }
}
