use serde::{Deserialize, Serialize};
use std::{
    convert::TryFrom,
    num::ParseIntError,
    ops::{Add, AddAssign, Sub, SubAssign},
};

#[derive(Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub const ZERO: Self = Self { x: 0.0, y: 0.0 };

    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

impl std::fmt::Debug for Point {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Point")
            .field("x", &self.x)
            .field("y", &self.y)
            .finish()
    }
}

#[derive(Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Vec2 {
    pub x: f64,
    pub y: f64,
}

impl Vec2 {
    pub const ZERO: Self = Self { x: 0.0, y: 0.0 };

    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

impl std::fmt::Debug for Vec2 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Vec2")
            .field("x", &self.x)
            .field("y", &self.y)
            .finish()
    }
}

impl Add<Vec2> for Point {
    type Output = Point;

    fn add(self, rhs: Vec2) -> Self::Output {
        Point::new(self.x + rhs.x, self.y + rhs.y)
    }
}

impl AddAssign<Vec2> for Point {
    fn add_assign(&mut self, rhs: Vec2) {
        self.x += rhs.x;
        self.y += rhs.y;
    }
}

impl Sub<Vec2> for Point {
    type Output = Point;

    fn sub(self, rhs: Vec2) -> Self::Output {
        Point::new(self.x - rhs.x, self.y - rhs.y)
    }
}

impl SubAssign<Vec2> for Point {
    fn sub_assign(&mut self, rhs: Vec2) {
        self.x -= rhs.x;
        self.y -= rhs.y;
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Id(u64);

impl TryFrom<&str> for Id {
    type Error = ParseIntError;

    fn try_from(val: &str) -> Result<Self, Self::Error> {
        let id = u64::from_str_radix(val, 16)?;
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

#[derive(Clone, Default, PartialEq)]
pub struct Node {
    pub id: Id,
    /// In grid units, not pixels.
    pub position: Point,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MetaKey {
    Position(Id),
    Cursor,
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

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Normal,
    Insert,
}

#[derive(Clone, Default, PartialEq)]
pub struct Cursor {
    pub position: Point,
}
