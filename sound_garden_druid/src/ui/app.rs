use crate::lens2::{Lens2, Lens2Wrap};
use crate::state::{self, Scene};
use crate::ui::constants::*;
use crate::ui::scene::*;
use druid::{
    kurbo::{Point, Rect, Size},
    piet::{Color, RenderContext},
    BaseState, BoxConstraints, BoxedWidget, Command, Env, Event, EventCtx, LayoutCtx, PaintCtx,
    UpdateCtx, WidgetPod,
};

pub struct Widget {
    scene: Option<BoxedWidget<State>>,
}

pub type State = state::State;

impl druid::Widget<State> for Widget {
    fn event(&mut self, ctx: &mut EventCtx, event: &Event, data: &mut State, env: &Env) {
        if let Some(scene) = &mut self.scene {
            scene.event(ctx, event, data, env);
        }
        match event {
            Event::Command(c) if c.selector == cmd::BACK_TO_GARDEN => {
                data.scene = Scene::Garden(state::GardenScene {
                    offset: *c.get_object().unwrap(),
                });
                ctx.submit_command(Command::from(cmd::REQUEST_FOCUS), None);
            }
            Event::Command(c) if c.selector == cmd::ZOOM_TO_PLANT => {
                data.scene = Scene::Plant(state::PlantScene {
                    ix: *c.get_object().unwrap(),
                    cursor: (0, 0).into(),
                    mode: state::PlantSceneMode::Normal,
                });
                ctx.submit_command(Command::from(cmd::REQUEST_FOCUS), None);
            }
            _ => {}
        }
    }

    fn update(&mut self, ctx: &mut UpdateCtx, old_data: Option<&State>, data: &State, env: &Env) {
        match old_data {
            Some(old_data) => {
                use Scene::*;
                match old_data.scene {
                    Garden(_) => match data.scene {
                        Garden(_) => {}
                        _ => self.change_scene(data),
                    },
                    Plant(_) => match data.scene {
                        Plant(_) => {}
                        _ => self.change_scene(data),
                    },
                }
            }
            None => self.change_scene(data),
        }
        if let Some(scene) = &mut self.scene {
            scene.update(ctx, data, env);
        }
        let _ = data.save(STATE_FILE);
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &State,
        env: &Env,
    ) -> Size {
        if let Some(scene) = &mut self.scene {
            let size = scene.layout(ctx, bc, data, env);
            scene.set_layout_rect(Rect::from_origin_size(Point::ORIGIN, size));
        }
        bc.max()
    }

    fn paint(&mut self, ctx: &mut PaintCtx, _base_state: &BaseState, data: &State, env: &Env) {
        ctx.clear(Color::WHITE);
        if let Some(scene) = &mut self.scene {
            scene.paint_with_offset(ctx, data, env);
        }
    }
}

impl Widget {
    pub fn new() -> Self {
        Widget { scene: None }
    }

    fn change_scene(&mut self, data: &State) {
        {
            use Scene::*;
            self.scene = Some(match data.scene {
                Garden(_) => {
                    log::debug!("Changing scene to Garden");
                    let lens = GardenSceneLens {};
                    WidgetPod::new(Box::new(Lens2Wrap::new(garden::Widget::new(), lens)))
                }
                Plant(_) => {
                    log::debug!("Changing scene to Plant");
                    let lens = PlantSceneLens {};
                    WidgetPod::new(Box::new(Lens2Wrap::new(plant::Widget::new(), lens)))
                }
            });
        }
    }
}

struct GardenSceneLens {}

impl Lens2<State, garden::State> for GardenSceneLens {
    fn get<V, F: FnOnce(&garden::State) -> V>(&self, data: &State, f: F) -> V {
        if let Scene::Garden(scene) = &data.scene {
            f(&garden::State::new(scene.clone(), data.plants.clone()))
        } else {
            unreachable!();
        }
    }

    fn with_mut<V, F: FnOnce(&mut garden::State) -> V>(&self, data: &mut State, f: F) -> V {
        if let Scene::Garden(scene) = &mut data.scene {
            let mut lens = garden::State::new(scene.clone(), data.plants.clone());
            let result = f(&mut lens);
            *scene = lens.scene;
            data.plants = lens.plants;
            result
        } else {
            unreachable!();
        }
    }
}

struct PlantSceneLens {}

impl Lens2<State, plant::State> for PlantSceneLens {
    fn get<V, F: FnOnce(&plant::State) -> V>(&self, data: &State, f: F) -> V {
        if let Scene::Plant(scene) = &data.scene {
            f(&plant::State::new(
                scene.clone(),
                data.plants[scene.ix].clone(),
            ))
        } else {
            unreachable!();
        }
    }

    fn with_mut<V, F: FnOnce(&mut plant::State) -> V>(&self, data: &mut State, f: F) -> V {
        if let Scene::Plant(scene) = &mut data.scene {
            let mut lens = plant::State::new(scene.clone(), data.plants[scene.ix].clone());
            let result = f(&mut lens);
            *scene = lens.scene;
            data.plants[scene.ix] = lens.plant;
            result
        } else {
            unreachable!();
        }
    }
}
