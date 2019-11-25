use crate::state;
use crate::ui::constants::*;
use druid::{AppDelegate, DelegateCtx, Env, Event, KeyCode};

#[derive(Debug, Default)]
pub struct Delegate;

impl AppDelegate<state::State> for Delegate {
    fn event(
        &mut self,
        event: Event,
        data: &mut state::State,
        _env: &Env,
        ctx: &mut DelegateCtx,
    ) -> Option<Event> {
        match event {
            Event::KeyDown(e) => match e.key_code {
                KeyCode::Escape => {
                    if let state::Scene::Plant(scene) = &data.scene {
                        if let state::PlantSceneMode::Normal = scene.mode {
                            let (x, y) = data.plants[scene.ix].position.into();
                            ctx.submit_command(cmd::back_to_garden((-x, -y).into()), None);
                        }
                    }
                }
                _ => {}
            },
            _ => {}
        }
        Some(event)
    }
}
