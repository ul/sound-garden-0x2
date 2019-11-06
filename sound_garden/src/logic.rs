use crate::world::*;
use anyhow::Result;
use audio_program::parse_tokens;
use audio_vm::VM;
use brotli::{CompressorWriter, Decompressor};
use clipboard::{ClipboardContext, ClipboardProvider};
use crossbeam_channel::Receiver;
use rand::prelude::*;
use sdl2::{event::Event, keyboard::Keycode};
use std::sync::{Arc, Mutex};

pub enum Command {
    SDLEvent(Event),
}

pub fn main(vm: Arc<Mutex<VM>>, world: Arc<Mutex<World>>, rx: Receiver<Command>) -> Result<()> {
    if let Ok(f) = std::fs::File::open("./world.json") {
        let mut w = world.lock().unwrap();
        *w = serde_json::from_reader(f)?;
    }
    let mut ops = Vec::new();
    for cmd in rx {
        let mut w = world.lock().unwrap();
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
        match cmd {
            _ => match w.screen {
                Screen::Garden => handle_garden(cmd, &mut w),
                Screen::Plant(_) => handle_plant(cmd, &mut w)?,
            },
        }
        let new_ops = match w.screen {
            Screen::Garden => Vec::new(),
            Screen::Plant(PlantEditor { ix, .. }) => {
                let Plant { nodes, edges, .. } = &w.plants[ix];
                let mut order = Vec::new();
                // All the code below relies on edges and leaves being sorted by x.
                // Start with the leftmost leaf.
                let mut cursor = edges.iter().find_map(|(i, _)| {
                    if edges.iter().any(|(_, j)| i == j) {
                        None
                    } else {
                        Some(*i)
                    }
                });
                // TODO Fix algorithmic complexity.
                // It's not critical right now because `edges` is expected to be small
                // and low constant factor linear scan should be good enough even for a loop
                // but we can do better.
                while let Some(node) = cursor {
                    if let Some((unordered_child, _)) =
                        edges.iter().find(|(i, j)| node == *j && !order.contains(i))
                    {
                        cursor = Some(*unordered_child);
                    } else {
                        order.push(node);
                        cursor = edges
                            .iter()
                            .find_map(|(i, j)| if node == *i { Some(*j) } else { None });
                    }
                }
                order
                    .iter()
                    .map(|i| nodes[*i].op.clone())
                    .collect::<Vec<_>>()
            }
        };
        if ops != new_ops {
            ops = new_ops;
            let program = parse_tokens(&ops, w.sample_rate);
            vm.lock().unwrap().load_program(program);
        }
        if let Ok(f) = std::fs::File::create("./world.json") {
            serde_json::to_writer(f, &*w)?;
        }
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
            }
            | Event::KeyDown {
                keycode: Some(Keycode::I),
                ..
            } => {
                if let Some((ix, _)) = w.plants.iter().enumerate().find(|(_, p)| {
                    p.position.x == w.garden.anima_position.x
                        && p.position.y == w.garden.anima_position.y
                }) {
                    w.screen = Screen::Plant(PlantEditor {
                        ix,
                        cursor_position: Point { x: 0, y: 0 },
                        mode: PlantEditorMode::Normal,
                    });
                } else {
                    w.plants.push(Plant {
                        position: w.garden.anima_position.clone(),
                        nodes: Vec::new(),
                        edges: Vec::new(),
                        symbol: char::from(thread_rng().gen_range(0x41, 0x5A)),
                    });
                    let ix = w.plants.len() - 1;
                    w.screen = Screen::Plant(PlantEditor {
                        ix,
                        cursor_position: Point { x: 0, y: 0 },
                        mode: PlantEditorMode::Normal,
                    });
                }
            }
            _ => {}
        },
    }
}

pub fn handle_plant(cmd: Command, w: &mut World) -> Result<()> {
    match w.screen {
        Screen::Plant(ref editor) => match editor.mode {
            PlantEditorMode::Normal => handle_plant_normal(cmd, w)?,
            PlantEditorMode::Insert => handle_plant_insert(cmd, w)?,
        },
        _ => unreachable!(),
    }
    Ok(())
}

pub fn handle_plant_normal(cmd: Command, w: &mut World) -> Result<()> {
    use Command::*;
    if let Screen::Plant(editor) = &mut w.screen {
        match cmd {
            SDLEvent(event) => match event {
                Event::KeyDown {
                    keycode: Some(Keycode::Escape),
                    ..
                } => {
                    if w.plants[editor.ix].nodes.is_empty() {
                        w.plants.swap_remove(editor.ix);
                    }
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
                }
                | Event::KeyDown {
                    keycode: Some(Keycode::Backspace),
                    ..
                } => editor.cursor_position.x -= 1,
                Event::KeyDown {
                    keycode: Some(Keycode::Right),
                    ..
                }
                | Event::KeyDown {
                    keycode: Some(Keycode::L),
                    ..
                }
                | Event::KeyDown {
                    keycode: Some(Keycode::Space),
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
                Event::KeyDown {
                    keycode: Some(Keycode::D),
                    ..
                } => {
                    let plant = &mut w.plants[editor.ix];
                    let node = node_at_cursor(plant, &editor.cursor_position);
                    if let Some((i, _)) = node {
                        plant.nodes.swap_remove(i);
                        find_edges(plant, w.cell_size);
                    }
                }
                Event::KeyDown {
                    keycode: Some(Keycode::Y),
                    ..
                } => {
                    let plant = &w.plants[editor.ix];
                    let mut compressed = Vec::new();
                    {
                        let mut compressor = CompressorWriter::new(&mut compressed, 4096, 11, 22);
                        serde_cbor::to_writer(&mut compressor, plant)?;
                    }
                    let text = base64::encode(&compressed);
                    let mut ctx: ClipboardContext = ClipboardProvider::new().unwrap();
                    ctx.set_contents(text).unwrap();
                }
                Event::KeyDown {
                    keycode: Some(Keycode::P),
                    ..
                } => {
                    let mut ctx: ClipboardContext = ClipboardProvider::new().unwrap();
                    let text = ctx.get_contents().unwrap();
                    let compressed = base64::decode(&text)?;
                    let decompressor: Decompressor<&[u8]> =
                        Decompressor::new(&compressed[..], 4096);
                    let new_plant = serde_cbor::from_reader(decompressor)?;
                    let plant = &mut w.plants[editor.ix];
                    let position = plant.position.clone();
                    *plant = new_plant;
                    plant.position = position;
                }
                _ => {}
            },
        }
    } else {
        unreachable!();
    }
    Ok(())
}

pub fn handle_plant_insert(cmd: Command, w: &mut World) -> Result<()> {
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
                }
                | Event::KeyDown {
                    keycode: Some(Keycode::Space),
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
                            let ixs = node
                                .op
                                .char_indices()
                                .skip(x)
                                .take(2)
                                .map(|x| x.0)
                                .collect::<Vec<_>>();
                            node.op.replace_range(
                                ixs[0]..*(ixs.get(1).unwrap_or(&node.op.len())),
                                &"",
                            );
                        } else {
                            plant.nodes.swap_remove(i);
                            find_edges(plant, w.cell_size);
                            info!("Removed a node.");
                        };
                    }
                }
                Event::TextInput { text, .. } => {
                    if text == " " {
                        return Ok(());
                    }
                    let plant = &mut w.plants[editor.ix];
                    let node = node_at_cursor(plant, &editor.cursor_position);
                    let position = editor.cursor_position.clone();
                    if let Some((_, node)) = node {
                        if position.x >= node.position.x + node.op.chars().count() as i32 {
                            node.op.push_str(&text);
                        } else {
                            let ix = node
                                .op
                                .char_indices()
                                .nth((position.x - node.position.x) as usize)
                                .map(|x| x.0)
                                .unwrap();
                            node.op.insert_str(ix, &text);
                        }
                    } else {
                        let node = Node {
                            op: String::from(&text),
                            position,
                        };
                        w.plants[editor.ix].nodes.push(node);
                        find_edges(&mut w.plants[editor.ix], w.cell_size);
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
    Ok(())
}

/// Slot right after node's text end also belong to the node.
fn node_at_cursor<'a>(plant: &'a mut Plant, cursor: &Point) -> Option<(usize, &'a mut Node)> {
    plant.nodes.iter_mut().enumerate().find(|(_, n)| {
        n.position.y == cursor.y
            && n.position.x <= cursor.x
            && cursor.x <= n.position.x + n.op.chars().count() as i32
    })
}

// Inefficient as hell, but good enough for the start.
fn find_edges(plant: &mut Plant, cell_size: (u32, u32)) {
    let Plant { edges, nodes, .. } = plant;
    edges.clear();
    for (i, node) in nodes.iter().enumerate() {
        let mut parent = None;
        for (j, n) in nodes
            .iter()
            .enumerate()
            .filter(|(j, n)| i != *j && n.position.y > node.position.y)
        {
            let dist = (cell_size.0 as i32 * (n.position.x - node.position.x)).pow(2)
                + (cell_size.1 as i32 * (n.position.y - node.position.y)).pow(2);
            match parent {
                Some((_, d)) => {
                    if dist < d {
                        parent = Some((j, dist));
                    }
                }

                None => parent = Some((j, dist)),
            }
        }
        if let Some((j, _)) = parent {
            edges.push((i, j));
        }
    }
    edges.sort_by(|(i1, _), (i2, _)| nodes[*i1].position.x.cmp(&nodes[*i2].position.x));
}
