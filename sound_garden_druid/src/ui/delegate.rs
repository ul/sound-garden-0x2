use crate::state::*;
use crate::ui::{constants::*, util};
use audio_program::parse_tokens;
use audio_vm::VM;
use druid::{AppDelegate, DelegateCtx, Env, Event, KeyCode};
use std::sync::{Arc, Mutex};

pub struct Delegate {
    ops: Vec<String>,
    vm: Arc<Mutex<VM>>,
}

impl AppDelegate<State> for Delegate {
    fn event(
        &mut self,
        event: Event,
        data: &mut State,
        _env: &Env,
        ctx: &mut DelegateCtx,
    ) -> Option<Event> {
        match event {
            Event::KeyDown(e) => match e.key_code {
                KeyCode::Escape => {
                    if let Scene::Plant(scene) = &data.scene {
                        if let PlantSceneMode::Normal = scene.mode {
                            ctx.submit_command(cmd::back_to_garden(), None);
                        }
                    }
                }
                _ => {}
            },
            _ => {}
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
                    .map(|i| nodes[*i].op.clone())
                    .collect::<Vec<_>>()
            }
        };
        if self.ops != new_ops {
            self.ops = new_ops;
            if !self.ops.is_empty() {
                log::info!("New program is '{}'", self.ops.join(" "));
            }
            let program = parse_tokens(&self.ops, data.sample_rate);
            self.vm.lock().unwrap().load_program(program);
        }
        Some(event)
    }
}

impl Delegate {
    pub fn new(vm: Arc<Mutex<VM>>) -> Self {
        Delegate {
            ops: Vec::new(),
            vm,
        }
    }
}
