use crate::state::*;
use crate::ui::{constants::*, util};
use audio_ops::Noop;
use audio_program::{parse_tokens, Context};
use audio_vm::{vm::FADE_FRAMES, VM};
use brotli::{CompressorWriter, Decompressor};
use druid::{AppDelegate, Application, DelegateCtx, Env, Event, KeyCode};
use std::sync::{Arc, Mutex};

pub struct Delegate {
    ctx: Context,
    ops: Vec<CachedOp>,
    vm: Arc<Mutex<VM>>,
}

#[derive(PartialEq, Eq)]
struct CachedOp {
    id: u64,
    op: String,
}

impl AppDelegate<State> for Delegate {
    fn event(
        &mut self,
        event: Event,
        data: &mut State,
        _env: &Env,
        ctx: &mut DelegateCtx,
    ) -> Option<Event> {
        if let Scene::Plant(scene) = &mut data.scene {
            match scene.mode {
                PlantSceneMode::Normal => match event {
                    Event::KeyDown(e) => match e.key_code {
                        KeyCode::Escape => {
                            ctx.submit_command(cmd::back_to_garden(), None);
                        }
                        KeyCode::KeyY => {
                            let plant = &data.plants[scene.ix];
                            let mut compressed = Vec::new();
                            {
                                let mut compressor =
                                    CompressorWriter::new(&mut compressed, 4096, 11, 22);
                                serde_cbor::to_writer(&mut compressor, plant).unwrap();
                            }
                            let text = base64::encode(&compressed);
                            Application::clipboard().put_string(text);
                            log::debug!("Copied seedling to clipboard.");
                        }
                        KeyCode::KeyP => {
                            log::debug!("Planting seedling from clipboard.");
                            let text = Application::clipboard().get_string().unwrap_or_default();
                            let compressed = base64::decode(&text);
                            if compressed.is_err() {
                                log::error!("Failed to decode seedling.");
                                return None;
                            }
                            let compressed = compressed.unwrap();
                            let decompressor: Decompressor<&[u8]> =
                                Decompressor::new(&compressed[..], 4096);
                            let new_plant = serde_cbor::from_reader(decompressor);
                            if new_plant.is_err() {
                                log::error!("Failed to decompress seedling.");
                                return None;
                            }
                            let new_plant = new_plant.unwrap();
                            let plant = &mut data.plants[scene.ix];
                            let position = plant.position.clone();
                            *plant = new_plant;
                            plant.position = position;
                        }
                        _ => {}
                    },
                    _ => {}
                },
                PlantSceneMode::Insert => match event {
                    Event::Command(ref c) if c.selector == crate::ui::text_line::EDIT_END => {
                        scene.mode = PlantSceneMode::Normal;
                    }
                    _ => {}
                },
            }
            match event {
                Event::Command(ref c) if c.selector == cmd::PLANT_SCENE_MODE => {
                    scene.mode = c.get_object::<PlantSceneMode>().unwrap().clone();
                }
                _ => {}
            }
        }
        let new_ops = match data.scene {
            Scene::Garden(_) => Vec::new(),
            Scene::Plant(PlantScene { ix, .. }) => {
                let Plant { nodes, .. } = &data.plants[ix];
                let edges = util::find_edges(&data.plants[ix]);
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
                    .map(|i| CachedOp {
                        id: nodes[*i].id,
                        op: nodes[*i].op.clone(),
                    })
                    .collect::<Vec<_>>()
            }
        };
        if self.ops != new_ops {
            {
                self.vm.lock().unwrap().fade_out()
            }
            log::debug!(
                "Sleeping for {} µs to fade out",
                (1e6 * FADE_FRAMES / (data.sample_rate as f64)) as u64,
            );
            std::thread::sleep(std::time::Duration::from_micros(
                (1e6 * FADE_FRAMES / (data.sample_rate as f64)) as _,
            ));
            let prg = new_ops.iter().map(|x| x.op.to_owned()).collect::<Vec<_>>();
            log::info!("New program is '{}'", prg.join(" "));
            let mut new_program = parse_tokens(&prg, data.sample_rate, &mut self.ctx);
            let mut old_program = { self.vm.lock().unwrap().unload_program() };
            let program = new_ops
                .iter()
                .enumerate()
                .map(|(i, op)| {
                    let src = self
                        .ops
                        .iter()
                        .position(|x| x == op)
                        .map(|i| &mut old_program[i])
                        .unwrap_or(&mut new_program[i]);
                    std::mem::replace(src, Box::new(Noop::new()))
                })
                .collect();
            {
                let mut vm = self.vm.lock().unwrap();
                vm.load_program(program);
                vm.fade_in();
            }
            self.ops = new_ops;
        }
        Some(event)
    }
}

impl Delegate {
    pub fn new(vm: Arc<Mutex<VM>>) -> Self {
        Delegate {
            ctx: Context::new(),
            ops: Vec::new(),
            vm,
        }
    }
}
