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
                keycode: Some(Keycode::Return),
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
                        mode: PlantEditorMode::Normal,
                    });
                }
            }
            _ => {}
        },
    }
}

pub fn handle_plant(cmd: Command, w: &mut World) {
    match w.screen {
        Screen::Plant(ref editor) => match editor.mode {
            PlantEditorMode::Normal => handle_plant_normal(cmd, w),
            PlantEditorMode::Insert => handle_plant_insert(cmd, w),
        },
        _ => unreachable!(),
    }
}

pub fn handle_plant_normal(cmd: Command, w: &mut World) {
    use Command::*;
    if let Screen::Plant(editor) = &mut w.screen {
        match cmd {
            SDLEvent(event) => match event {
                Event::KeyDown {
                    keycode: Some(Keycode::Escape),
                    ..
                } => {
                    // TODO Save tree.
                    w.screen = Screen::Garden;
                }
                Event::KeyDown {
                    keycode: Some(Keycode::Return),
                    ..
                }
                | Event::KeyUp {
                    keycode: Some(Keycode::I),
                    ..
                } => {
                    editor.mode = PlantEditorMode::Insert;
                }
                Event::KeyDown {
                    keycode: Some(Keycode::Left),
                    ..
                }
                | Event::KeyDown {
                    keycode: Some(Keycode::H),
                    ..
                } => editor.cursor_position.x -= 1,
                Event::KeyDown {
                    keycode: Some(Keycode::Right),
                    ..
                }
                | Event::KeyDown {
                    keycode: Some(Keycode::L),
                    ..
                } => editor.cursor_position.x += 1,
                Event::KeyDown {
                    keycode: Some(Keycode::Up),
                    ..
                }
                | Event::KeyDown {
                    keycode: Some(Keycode::K),
                    ..
                } => editor.cursor_position.y -= 1,
                Event::KeyDown {
                    keycode: Some(Keycode::Down),
                    ..
                }
                | Event::KeyDown {
                    keycode: Some(Keycode::J),
                    ..
                } => editor.cursor_position.y += 1,
                _ => {}
            },
        }
    } else {
        unreachable!();
    }
}

pub fn handle_plant_insert(cmd: Command, w: &mut World) {
    use Command::*;
    if let Screen::Plant(editor) = &mut w.screen {
        match cmd {
            SDLEvent(event) => match event {
                Event::KeyDown {
                    keycode: Some(Keycode::Escape),
                    ..
                }
                | Event::KeyDown {
                    keycode: Some(Keycode::Return),
                    ..
                } => {
                    editor.mode = PlantEditorMode::Normal;
                }
                Event::KeyDown {
                    keycode: Some(Keycode::Left),
                    ..
                } => editor.cursor_position.x -= 1,
                Event::KeyDown {
                    keycode: Some(Keycode::Right),
                    ..
                } => editor.cursor_position.x += 1,
                Event::KeyDown {
                    keycode: Some(Keycode::Backspace),
                    ..
                } => {
                    let plant = &mut w.plants[editor.ix];
                    let node = node_at_cursor(plant, &editor.cursor_position);
                    editor.cursor_position.x -= 1;
                    if let Some((i, node)) = node {
                        if editor.cursor_position.x > node.position.x {
                            let x = (editor.cursor_position.x - node.position.x) as usize;
                            node.op.replace_range(x..(x + 1), &"");
                        } else {
                            plant.nodes.swap_remove(i);
                            find_edges(plant);
                            info!("Removed a node.");
                        };
                    }
                }
                Event::TextInput { text, .. } => {
                    if text == " " {
                        return;
                    }
                    let plant = &mut w.plants[editor.ix];
                    let node = node_at_cursor(plant, &editor.cursor_position);
                    let position = editor.cursor_position.clone();
                    if let Some((_, node)) = node {
                        if position.x >= node.position.x + node.op.len() as i32 {
                            node.op.push_str(&text);
                        } else {
                            node.op
                                .insert_str((position.x - node.position.x) as usize, &text);
                        }
                    } else {
                        let node = Node {
                            op: String::from(&text),
                            position,
                        };
                        w.plants[editor.ix].nodes.push(node);
                        find_edges(&mut w.plants[editor.ix]);
                        info!("Created new node.");
                    };
                    editor.cursor_position.x += 1;
                }
                _ => {}
            },
        }
    } else {
        unreachable!();
    }
}

/// Slot right after node's text end also belong to the node.
fn node_at_cursor<'a>(plant: &'a mut Plant, cursor: &Point) -> Option<(usize, &'a mut Node)> {
    plant.nodes.iter_mut().enumerate().find(|(_, n)| {
        n.position.y == cursor.y
            && n.position.x <= cursor.x
            && cursor.x <= n.position.x + n.op.len() as i32
    })
}

// Inefficient as hell, but good enough for the start.
fn find_edges(plant: &mut Plant) {
    let edges = &mut plant.edges;
    edges.clear();
    for (i, node) in plant.nodes.iter().enumerate() {
        let mut parent = None;
        for (j, n) in plant
            .nodes
            .iter()
            .enumerate()
            .filter(|(j, n)| i != *j && n.position.y > node.position.y)
        {
            let delta = (n.position.x - node.position.x).abs();
            match parent {
                Some((_, y, d)) => {
                    if n.position.y < y || (n.position.y == y && delta < d) {
                        parent = Some((j, n.position.y, delta));
                    }
                }

                None => parent = Some((j, n.position.y, delta)),
            }
        }
        if let Some((j, _, _)) = parent {
            edges.push((i, j));
        }
    }
}
