use anyhow::Result;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sound_garden_types::*;
use std::{collections::HashMap, convert::TryFrom};

/// Repository is the source of truth for the Sound Garden document.
#[derive(Default)]
pub struct NodeRepository {
    nodes: Vec<Node>,
    cursor: Cursor,
    undo_stack: Vec<Snapshot>,
    redo_stack: Vec<Snapshot>,
    last_undo_group: Option<u64>,
}

#[derive(Clone)]
struct Snapshot {
    nodes: Vec<Node>,
    cursor: Cursor,
}

#[derive(Serialize, Deserialize)]
struct StoredNodeRepository {
    nodes: Vec<StoredNode>,
    cursor: StoredPoint,
}

#[derive(Serialize, Deserialize)]
struct StoredNode {
    id: Id,
    position: StoredPoint,
    text: String,
}

#[derive(Clone, Copy, Default, Serialize, Deserialize)]
struct StoredPoint {
    x: f64,
    y: f64,
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
        Self::default()
    }

    pub fn add_node(&mut self, node: Node, color: u64) {
        self.push_undo_snapshot(color);
        self.nodes.push(node);
        self.sort_nodes();
    }

    pub fn delete_nodes(&mut self, ids: &[Id], color: u64) {
        self.push_undo_snapshot(color);
        self.nodes.retain(|node| !ids.contains(&node.id));
    }

    pub fn edit_nodes(&mut self, edits: HashMap<Id, Vec<NodeEdit>>, color: u64) {
        self.push_undo_snapshot(color);
        for node in &mut self.nodes {
            if let Some(edits) = edits.get(&node.id) {
                for edit in edits {
                    match edit {
                        NodeEdit::Move(p) => node.position = *p,
                        NodeEdit::Edit { start, end, text } => {
                            node.text = replace_char_range(&node.text, *start, *end, text);
                        }
                    }
                }
            }
        }
        self.sort_nodes();
    }

    pub fn undo(&mut self) -> bool {
        if let Some(snapshot) = self.undo_stack.pop() {
            self.redo_stack.push(self.snapshot());
            self.restore(snapshot);
            self.last_undo_group = None;
            true
        } else {
            false
        }
    }

    pub fn redo(&mut self) -> bool {
        if let Some(snapshot) = self.redo_stack.pop() {
            self.undo_stack.push(self.snapshot());
            self.restore(snapshot);
            self.last_undo_group = None;
            true
        } else {
            false
        }
    }

    pub fn load(filename: &str) -> Self {
        std::fs::File::open(filename)
            .ok()
            .map(snap::read::FrameDecoder::new)
            .and_then(|f| serde_cbor::from_reader::<Self, _>(f).ok())
            .unwrap_or_else(Self::new)
    }

    // TODO Atomic write.
    pub fn save(&self, filename: &str) -> Result<()> {
        std::fs::File::create(filename)
            .or_else(|e| anyhow::bail!(e))
            .and_then(|f| {
                serde_cbor::to_writer(snap::write::FrameEncoder::new(f), self)
                    .or_else(|e| anyhow::bail!(e))
            })
            .map(|_| ())
    }

    pub fn nodes(&self) -> Vec<Node> {
        let mut nodes = self.nodes.clone();
        nodes.sort_unstable_by_key(|node| (node.position.y as i64, node.position.x as i64));
        nodes
    }

    pub fn text(&self) -> String {
        self.nodes()
            .into_iter()
            .map(|node| format!("{}\t{}\n", String::from(node.id), node.text))
            .collect()
    }

    pub fn meta(&self) -> HashMap<MetaKey, MetaValue> {
        let mut meta = self
            .nodes
            .iter()
            .map(|node| {
                (
                    MetaKey::Position(node.id),
                    MetaValue::Position(node.position.x as _, node.position.y as _),
                )
            })
            .collect::<HashMap<_, _>>();
        meta.insert(
            MetaKey::Cursor,
            MetaValue::Position(self.cursor.position.x as _, self.cursor.position.y as _),
        );
        meta
    }

    pub fn get_cursor(&self) -> Cursor {
        self.cursor.clone()
    }

    pub fn set_cursor(&mut self, cursor: &Cursor, _color: u64) {
        self.cursor = cursor.clone();
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            nodes: self.nodes.clone(),
            cursor: self.cursor.clone(),
        }
    }

    fn push_undo_snapshot(&mut self, group: u64) {
        if self.last_undo_group != Some(group) {
            self.undo_stack.push(self.snapshot());
            self.redo_stack.clear();
            self.last_undo_group = Some(group);
        }
    }

    fn restore(&mut self, snapshot: Snapshot) {
        self.nodes = snapshot.nodes;
        self.cursor = snapshot.cursor;
        self.sort_nodes();
    }

    fn sort_nodes(&mut self) {
        self.nodes
            .sort_unstable_by_key(|node| (node.position.y as i64, node.position.x as i64));
    }
}

impl Serialize for NodeRepository {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        StoredNodeRepository::from(self).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for NodeRepository {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(StoredNodeRepository::deserialize(deserializer)?.into())
    }
}

impl From<&NodeRepository> for StoredNodeRepository {
    fn from(repo: &NodeRepository) -> Self {
        Self {
            nodes: repo.nodes.iter().map(StoredNode::from).collect(),
            cursor: repo.cursor.position.into(),
        }
    }
}

impl From<StoredNodeRepository> for NodeRepository {
    fn from(repo: StoredNodeRepository) -> Self {
        let mut repo = Self {
            nodes: repo.nodes.into_iter().map(Node::from).collect(),
            cursor: Cursor {
                position: repo.cursor.into(),
            },
            ..Self::new()
        };
        repo.sort_nodes();
        repo
    }
}

impl From<&Node> for StoredNode {
    fn from(node: &Node) -> Self {
        Self {
            id: node.id,
            position: node.position.into(),
            text: node.text.clone(),
        }
    }
}

impl From<StoredNode> for Node {
    fn from(node: StoredNode) -> Self {
        Self {
            id: node.id,
            position: node.position.into(),
            text: node.text,
        }
    }
}

impl From<Point> for StoredPoint {
    fn from(point: Point) -> Self {
        Self {
            x: point.x,
            y: point.y,
        }
    }
}

impl From<StoredPoint> for Point {
    fn from(point: StoredPoint) -> Self {
        Point::new(point.x, point.y)
    }
}

fn replace_char_range(s: &str, start: usize, end: usize, replacement: &str) -> String {
    let prefix = s.chars().take(start);
    let suffix = s.chars().skip(end);
    prefix.chain(replacement.chars()).chain(suffix).collect()
}

impl TryFrom<&str> for NodeRepository {
    type Error = anyhow::Error;

    fn try_from(s: &str) -> Result<Self> {
        let mut nodes = Vec::new();
        for line in s.lines() {
            if let Some((id, text)) = line.split_once('\t') {
                nodes.push(Node {
                    id: Id::try_from(id)?,
                    position: Point::ZERO,
                    text: text.to_owned(),
                });
            }
        }
        Ok(Self {
            nodes,
            ..Self::new()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: u64, x: f64, y: f64, text: &str) -> Node {
        Node {
            id: Id::from(id),
            position: Point::new(x, y),
            text: text.to_owned(),
        }
    }

    #[test]
    fn nodes_and_text_are_sorted_by_grid_position() {
        let mut repo = NodeRepository::new();
        repo.add_node(node(0x2, 5.0, 1.0, "second"), 1);
        repo.add_node(node(0x1, 1.0, 1.0, "first"), 2);
        repo.add_node(node(0x3, 0.0, 2.0, "third"), 3);

        let nodes = repo.nodes();

        assert_eq!(nodes[0].id, Id::from(0x1));
        assert_eq!(nodes[1].id, Id::from(0x2));
        assert_eq!(nodes[2].id, Id::from(0x3));
        assert_eq!(
            repo.text(),
            "0000000000000001\tfirst\n0000000000000002\tsecond\n0000000000000003\tthird\n"
        );
    }

    #[test]
    fn edit_nodes_moves_and_edits_by_character_range() {
        let mut repo = NodeRepository::new();
        repo.add_node(node(0x1, 0.0, 0.0, "a🎹cd"), 1);

        let mut edits = HashMap::new();
        edits.insert(
            Id::from(0x1),
            vec![
                NodeEdit::Move(Point::new(3.0, 4.0)),
                NodeEdit::Edit {
                    start: 1,
                    end: 2,
                    text: "b".to_owned(),
                },
            ],
        );
        repo.edit_nodes(edits, 2);

        let nodes = repo.nodes();
        assert_eq!(nodes[0].position, Point::new(3.0, 4.0));
        assert_eq!(nodes[0].text, "abcd");
        assert_eq!(
            repo.meta().get(&MetaKey::Position(Id::from(0x1))),
            Some(&MetaValue::Position(3, 4))
        );
    }

    #[test]
    fn undo_groups_changes_by_color_and_redo_restores_them() {
        let mut repo = NodeRepository::new();
        repo.add_node(node(0x1, 0.0, 0.0, "one"), 7);
        repo.add_node(node(0x2, 1.0, 0.0, "two"), 7);

        assert!(repo.undo());
        assert!(repo.nodes().is_empty());

        assert!(repo.redo());
        assert_eq!(repo.nodes().len(), 2);
    }

    #[test]
    fn adding_after_undo_clears_redo_history() {
        let mut repo = NodeRepository::new();
        repo.add_node(node(0x1, 0.0, 0.0, "one"), 1);
        assert!(repo.undo());

        repo.add_node(node(0x2, 0.0, 0.0, "two"), 2);

        assert!(!repo.redo());
        let nodes = repo.nodes();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].id, Id::from(0x2));
    }

    #[test]
    fn parses_text_format_and_rejects_invalid_ids() {
        let repo =
            NodeRepository::try_from("000000000000000a\tosc\nnot a node\n000000000000000b\t+\n")
                .expect("valid text repository");

        assert_eq!(repo.nodes().len(), 2);
        assert_eq!(repo.text(), "000000000000000a\tosc\n000000000000000b\t+\n");
        assert!(NodeRepository::try_from("not-hex\tbad\n").is_err());
    }
}
