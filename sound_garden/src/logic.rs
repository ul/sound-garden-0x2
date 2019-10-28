use crate::world::*;
use anyhow::Result;
use crossbeam_channel::{Receiver, Sender};
use sdl2::{event::Event, keyboard::Keycode, rect::Point};

pub enum Command {
    SDLEvent(Event),
}

pub fn main(rx: Receiver<Command>, tx: Sender<World>) -> Result<()> {
    let mut w = World::new();
    tx.send(w.clone())?;
    for cmd in rx {
        if let Command::SDLEvent(Event::Quit { .. }) = cmd {
            return Ok(());
        }
        if let Screen::Garden = w.screen {
            if let Command::SDLEvent(Event::KeyDown {
                keycode: Some(Keycode::Escape),
                ..
            }) = cmd
            {
                return Ok(());
            }
        }
        match w.screen {
            Screen::Garden => handle_garden(cmd, &mut w),
            Screen::Plant(_) => handle_plant(cmd, &mut w),
        }
        tx.send(w.clone())?;
    }
    Ok(())
}

pub fn handle_garden(cmd: Command, w: &mut World) {
    use Command::*;
    match cmd {
        SDLEvent(event) => match event {
            Event::KeyDown {
                keycode: Some(Keycode::Left),
                ..
            }
            | Event::KeyDown {
                keycode: Some(Keycode::H),
                ..
            } => w.garden.anima_position.x -= 1,
            Event::KeyDown {
                keycode: Some(Keycode::Right),
                ..
            }
            | Event::KeyDown {
                keycode: Some(Keycode::L),
                ..
            } => w.garden.anima_position.x += 1,
            Event::KeyDown {
                keycode: Some(Keycode::Up),
                ..
            }
            | Event::KeyDown {
                keycode: Some(Keycode::K),
                ..
            } => w.garden.anima_position.y -= 1,
            Event::KeyDown {
                keycode: Some(Keycode::Down),
                ..
            }
            | Event::KeyDown {
                keycode: Some(Keycode::J),
                ..
            } => w.garden.anima_position.y += 1,
            Event::KeyDown {
                keycode: Some(Keycode::Space),
                ..
            } => {
                if let Some((ix, _)) = w
                    .plants
                    .iter()
                    .enumerate()
                    .find(|(_, p)| p.position == w.garden.anima_position)
                {
                    w.screen = Screen::Plant(PlantEditor {
                        ix,
                        cursor_position: Point::new(0, 0),
                    });
                }
            }
            _ => {}
        },
    }
}

pub fn handle_plant(cmd: Command, w: &mut World) {
    use Command::*;
    match cmd {
        SDLEvent(event) => match event {
            Event::KeyDown {
                keycode: Some(Keycode::Escape),
                ..
            } => {
                // TODO Save tree.
                w.screen = Screen::Garden;
            }
            _ => {}
        },
    }
}
