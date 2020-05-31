//! A CRDT-based engine for collaborative text editing with meta and undo support.
//!
//! Undo/redo edits are shared but undo stack is local.

use crdts::{
    orswot::{Member, Op},
    CmRDT, CvRDT, Orswot,
};
use rand::random;
pub use ropey::Rope;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};

const START: CharId = CharId(0);
const END: CharId = CharId(1);
const FIRST: ValueId = ValueId(0);

// TODO Ensure skipped fields are in-sync after deserialization.
#[derive(Clone, Serialize, Deserialize)]
pub struct Engine<K: Member, V: Member> {
    id: SessionId,
    chars: Orswot<(CharId, char), SessionId>,
    char_edges: Orswot<(CharId, CharId), SessionId>,
    values: Orswot<(ValueId, (K, V)), SessionId>,
    value_edges: Orswot<(ValueId, ValueId), SessionId>,
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
    #[serde(skip)]
    meta: HashMap<K, V>,
    #[serde(skip)]
    meta_ids: HashMap<K, ValueId>,
}

pub enum Delta<K, V> {
    Text {
        range: (usize, usize),
        new_text: String,
        // When color changes we start a new undo group.
        color: u64,
    },
    Value {
        op: ValueOp<K, V>,
        // When color changes we start a new undo group.
        color: u64,
    },
}

pub enum ValueOp<K, V> {
    Set(K, V),
    Remove(K),
}

#[derive(Clone, Serialize, Deserialize)]
pub enum Patch<K: Member, V: Member> {
    Edit {
        chars: Vec<Op<(CharId, char), SessionId>>,
        char_edges: Vec<Op<(CharId, CharId), SessionId>>,
        values: Vec<Op<(ValueId, (K, V)), SessionId>>,
        value_edges: Vec<Op<(ValueId, ValueId), SessionId>>,
        edits: Vec<Op<Edit, SessionId>>,
    },
    Undo {
        undone: Op<UndoGroupId, SessionId>,
    },
}

#[derive(Clone, Copy, Debug, Hash, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct CharId(u64);

#[derive(Clone, Copy, Debug, Hash, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct ValueId(u64);

#[derive(Clone, Copy, Debug, Hash, Serialize, Deserialize, PartialEq, Eq, PartialOrd)]
pub struct UndoGroupId(u64);

#[derive(Clone, Debug, Hash, Serialize, Deserialize, PartialEq, Eq)]
pub struct Edit {
    undo_group: UndoGroupId,
    char_inserts: Vec<CharId>,
    char_deletes: Vec<CharId>,
    value_inserts: Vec<ValueId>,
    value_deletes: Vec<ValueId>,
}

#[derive(Clone, Copy, Debug, Hash, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct SessionId(u64);

impl<K: Member, V: Member> Engine<K, V> {
    pub fn new() -> Self {
        Engine {
            id: SessionId::generate(),
            chars: Default::default(),
            char_edges: Default::default(),
            values: Default::default(),
            value_edges: Default::default(),
            edits: Default::default(),
            undone: Default::default(),
            text: Default::default(),
            text_ids: Default::default(),
            meta: Default::default(),
            meta_ids: Default::default(),
            undo_groups: vec![UndoGroupId::generate()],
            redo_groups: Default::default(),
            last_color: 0,
        }
    }

    pub fn meta(&self) -> &HashMap<K, V> {
        &self.meta
    }

    pub fn text(&self) -> &Rope {
        &self.text
    }

    pub fn session_id(&self) -> SessionId {
        self.id
    }

    // Text deltas must not have overlapping ranges.
    pub fn edit(&mut self, deltas: &[Delta<K, V>]) -> Patch<K, V> {
        let mut deltas = deltas.iter().collect::<Vec<_>>();
        deltas.sort_by_key(|delta| match delta {
            Delta::Text { range, .. } => range.0,
            Delta::Value { .. } => 0,
        });
        self.redo_groups.clear();
        let mut chars = Vec::new();
        let mut char_edges = Vec::new();
        let mut values = Vec::new();
        let mut value_edges = Vec::new();
        let mut edits = Vec::new();
        let mut edit = Edit {
            undo_group: self
                .undo_groups
                .last()
                .copied()
                .unwrap_or_else(|| UndoGroupId::generate()),
            char_inserts: Default::default(),
            char_deletes: Default::default(),
            value_inserts: Default::default(),
            value_deletes: Default::default(),
        };
        let mut shift: isize = 0;
        for delta in deltas {
            match delta {
                Delta::Text {
                    range,
                    new_text,
                    color,
                } => {
                    // Start a new undo group if color changed.
                    if *color != self.last_color {
                        if !edit.char_inserts.is_empty()
                            || !edit.char_deletes.is_empty()
                            || !edit.value_inserts.is_empty()
                            || !edit.value_deletes.is_empty()
                        {
                            let op = self
                                .edits
                                .add(edit, self.edits.read().derive_add_ctx(self.id));
                            self.edits.apply(op.clone());
                            edits.push(op);
                        }
                        self.last_color = *color;
                        let new_group = UndoGroupId::generate();
                        self.undo_groups.push(new_group);
                        edit = Edit {
                            undo_group: new_group,
                            char_inserts: Default::default(),
                            char_deletes: Default::default(),
                            value_inserts: Default::default(),
                            value_deletes: Default::default(),
                        };
                    }
                    let range = ((range.0 as isize + shift) as usize)
                        ..((range.1 as isize + shift) as usize);

                    shift += new_text.chars().count() as isize - range.len() as isize;

                    self.text.remove(range.clone());
                    self.text.insert(range.start, &new_text);

                    let ids = std::iter::repeat_with(CharId::generate)
                        .take(new_text.chars().count())
                        .collect::<Vec<_>>();

                    let char_before = if range.start > 0 {
                        self.text_ids[range.start - 1]
                    } else {
                        START
                    };
                    let char_after = if range.end < self.text_ids.len() {
                        self.text_ids[range.end]
                    } else {
                        END
                    };

                    edit.char_deletes
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
                        .zip(new_text.chars())
                        .zip(previous_ids)
                        .zip(next_ids)
                    {
                        // Register insertion.
                        edit.char_inserts.push(id);
                        // Register character.
                        let op = self
                            .chars
                            .add((id, char), self.chars.read().derive_add_ctx(self.id));
                        self.chars.apply(op.clone());
                        chars.push(op);
                        // Register edge from the previous character.
                        if previous != START {
                            let op = self.char_edges.add(
                                (previous, id),
                                self.char_edges.read().derive_add_ctx(self.id),
                            );
                            self.char_edges.apply(op.clone());
                            char_edges.push(op);
                        }
                        // Register edge to the next character.
                        if next != END {
                            let op = self
                                .char_edges
                                .add((id, next), self.char_edges.read().derive_add_ctx(self.id));
                            self.char_edges.apply(op.clone());
                            char_edges.push(op);
                        }
                    }
                }
                Delta::Value { op, color } => {
                    // Start a new undo group if color changed.
                    if *color != self.last_color {
                        if !edit.char_inserts.is_empty()
                            || !edit.char_deletes.is_empty()
                            || !edit.value_inserts.is_empty()
                            || !edit.value_deletes.is_empty()
                        {
                            let op = self
                                .edits
                                .add(edit, self.edits.read().derive_add_ctx(self.id));
                            self.edits.apply(op.clone());
                            edits.push(op);
                        }
                        self.last_color = *color;
                        let new_group = UndoGroupId::generate();
                        self.undo_groups.push(new_group);
                        edit = Edit {
                            undo_group: new_group,
                            char_inserts: Default::default(),
                            char_deletes: Default::default(),
                            value_inserts: Default::default(),
                            value_deletes: Default::default(),
                        };
                    }
                    match op {
                        ValueOp::Set(key, value) => {
                            let id = ValueId::generate();
                            // Register insertion.
                            edit.value_inserts.push(id);
                            // Register key-value pair.
                            let op = self.values.add(
                                (id, (key.to_owned(), value.to_owned())),
                                self.values.read().derive_add_ctx(self.id),
                            );
                            self.values.apply(op.clone());
                            values.push(op);
                            self.meta.insert(key.to_owned(), value.to_owned());
                            // Register edge from the previous value.
                            if let Some(previous) = self.meta_ids.insert(key.to_owned(), id) {
                                let op = self.value_edges.add(
                                    (previous, id),
                                    self.value_edges.read().derive_add_ctx(self.id),
                                );
                                self.value_edges.apply(op.clone());
                                value_edges.push(op);
                            }
                        }
                        ValueOp::Remove(key) => {
                            self.meta.remove(key);
                            // NOTE We are not removing key from meta_ids to have better edges on subsequent inserts.
                            if let Some(id) = self.meta_ids.get(key) {
                                edit.value_deletes.push(*id);
                            }
                        }
                    }
                }
            }
        }
        if !edit.char_inserts.is_empty()
            || !edit.char_deletes.is_empty()
            || !edit.value_inserts.is_empty()
            || !edit.value_deletes.is_empty()
        {
            let op = self
                .edits
                .add(edit, self.edits.read().derive_add_ctx(self.id));
            self.edits.apply(op.clone());
            edits.push(op);
        }
        Patch::Edit {
            chars,
            char_edges,
            edits,
            values,
            value_edges,
        }
    }

    pub fn undo(&mut self) -> Option<Patch<K, V>> {
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

    pub fn redo(&mut self) -> Option<Patch<K, V>> {
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

    pub fn apply(&mut self, patch: Patch<K, V>) {
        match patch {
            Patch::Edit {
                chars,
                char_edges,
                edits,
                values,
                value_edges,
            } => {
                for op in chars {
                    self.chars.apply(op);
                }
                for op in char_edges {
                    self.char_edges.apply(op);
                }
                for op in edits {
                    self.edits.apply(op);
                }
                for op in values {
                    self.values.apply(op);
                }
                for op in value_edges {
                    self.value_edges.apply(op);
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
        self.char_edges.merge(other.char_edges);
        self.undone.merge(other.undone);
        self.rebuild();
    }

    pub fn rebuild(&mut self) {
        // Characters.
        // Not tracking inserts as their union contains all chars.
        let mut deletes = HashSet::new();
        let undone = self.undone.read().val;
        for edit in self.edits.read().val {
            if undone.contains(&edit.undo_group) {
                deletes.extend(edit.char_inserts);
            } else {
                deletes.extend(edit.char_deletes);
            }
        }
        let mut text = String::new();
        let mut text_ids = Vec::new();
        let chars = self.chars.read().val;
        let mut id_to_char = chars.iter().copied().collect::<HashMap<_, _>>();
        let edges = self.char_edges.read().val;
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
        // Values.
        let mut deletes = HashSet::new();
        for edit in self.edits.read().val {
            if undone.contains(&edit.undo_group) {
                deletes.extend(edit.value_inserts);
            } else {
                deletes.extend(edit.value_deletes);
            }
        }
        let pairs = self.values.read().val;
        let mut id_to_pair = pairs.iter().cloned().collect::<HashMap<_, _>>();
        let edges = self.value_edges.read().val;
        let mut incoming_edges = HashMap::new();
        let mut outgoing_edges = HashMap::new();
        for (from, to) in edges {
            incoming_edges.entry(to).or_insert_with(Vec::new).push(from);
            outgoing_edges.entry(from).or_insert_with(Vec::new).push(to);
        }
        let mut queue = pairs
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
        let mut order = Vec::new();
        // TODO Stable tie-breaking, with insert dominating removal.
        while let Some(id) = queue.pop_front() {
            if !deletes.contains(&id) {
                order.push((id, id_to_pair.remove(&id).unwrap()));
            }
            for next in outgoing_edges.remove(&id).unwrap_or_default() {
                let incoming = incoming_edges.get_mut(&next).unwrap();
                incoming.retain(|&prev| prev != id);
                if incoming.is_empty() {
                    queue.push_back(next);
                }
            }
        }
        self.meta = order
            .iter()
            .cloned()
            .map(|(_, pair)| pair)
            .collect::<HashMap<_, _>>();
        self.meta_ids = order
            .into_iter()
            .map(|(id, (k, _))| (k, id))
            .collect::<HashMap<_, _>>();
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
        while id == START.0 || id == END.0 {
            id = random();
        }
        Self(id)
    }
}

impl ValueId {
    fn generate() -> Self {
        let mut id = random();
        while id == FIRST.0 {
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
