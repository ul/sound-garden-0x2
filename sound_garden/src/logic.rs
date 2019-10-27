use crate::world::World;
use anyhow::Result;
use crossbeam_channel::{Receiver, Sender};

pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

pub enum Command {
    Move(Direction),
    Quit,
}

pub fn main(rx: Receiver<Command>, tx: Sender<World>) -> Result<()> {
    let mut world = World::new();
    tx.send(world.clone())?;
    use Command::*;
    for cmd in rx {
        match cmd {
            Move(Direction::Left) => world.anima.position.x -= 1,
            Move(Direction::Right) => world.anima.position.x += 1,
            Move(Direction::Up) => world.anima.position.y -= 1,
            Move(Direction::Down) => world.anima.position.y += 1,
            Quit => return Ok(()),
        }
        tx.send(world.clone())?;
    }
    Ok(())
}
