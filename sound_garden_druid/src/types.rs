use druid::{Data, Point};
use std::{convert::TryFrom, num::ParseIntError};

#[derive(Clone, Copy, Data, Debug, Default, PartialEq, Eq, Hash)]
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

#[derive(Clone, Data, Default)]
pub struct Node {
    pub id: Id,
    /// In grid units, not pixels.
    pub position: Point,
    pub text: String,
}
