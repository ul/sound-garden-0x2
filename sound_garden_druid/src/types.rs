use crdt_engine::SessionId;
use druid::{Data, Point};
use serde::{Deserialize, Serialize};
use std::{convert::TryFrom, num::ParseIntError};

#[derive(Clone, Copy, Data, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Id(u64);

impl TryFrom<&str> for Id {
    type Error = ParseIntError;
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        let id = u64::from_str_radix(value, 16)?;
        Ok(Id(id))
    }
}

impl From<Id> for String {
    fn from(id: Id) -> Self {
        format!("{:016x}", id.0)
    }
}

impl From<&Id> for String {
    fn from(id: &Id) -> Self {
        format!("{:016x}", id.0)
    }
}

impl From<Id> for u64 {
    fn from(id: Id) -> Self {
        id.0
    }
}

impl From<u64> for Id {
    fn from(id: u64) -> Self {
        Id(id)
    }
}

impl Id {
    pub fn random() -> Self {
        Id(rand::random())
    }
}

impl std::fmt::Debug for Id {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s: String = self.into();
        f.write_str(&s)
    }
}

#[derive(Clone, Data, Default)]
pub struct Node {
    pub id: Id,
    /// In grid units, not pixels.
    pub position: Point,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MetaKey {
    Position(Id),
    Cursor(SessionId),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MetaValue {
    Position(i64, i64),
}

// REVIEW Does it make any sense? Why exactly do we need default impl?
impl Default for MetaKey {
    fn default() -> Self {
        MetaKey::Position(Default::default())
    }
}

// REVIEW Does it make any sense? Why exactly do we need default impl?
impl Default for MetaValue {
    fn default() -> Self {
        MetaValue::Position(0, 0)
    }
}
