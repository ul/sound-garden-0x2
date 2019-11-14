use super::scene::*;
use crate::state::{Scene, State};
use druid::{
    kurbo::{Point, Rect, Size},
    piet::{Color, RenderContext},
    BaseState, BoxConstraints, BoxedWidget, Env, Event, EventCtx, LayoutCtx, PaintCtx, UpdateCtx,
    Widget, WidgetPod,
};

pub struct App {
    scene: Option<BoxedWidget<State>>,
}

impl Widget<State> for App {
    fn paint(&mut self, ctx: &mut PaintCtx, _base_state: &BaseState, data: &State, env: &Env) {
        ctx.clear(Color::WHITE);
        if let Some(scene) = &mut self.scene {
            scene.paint_with_offset(ctx, data, env);
        }
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

    fn event(&mut self, event: &Event, ctx: &mut EventCtx, data: &mut State, env: &Env) {
        if let Some(scene) = &mut self.scene {
            scene.event(event, ctx, data, env);
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
                    Plant => match data.scene {
                        Plant => {}
                        _ => self.change_scene(data),
                    },
                }
            }
            None => self.change_scene(data),
        }
        if let Some(scene) = &mut self.scene {
            scene.update(ctx, data, env);
        }
    }
}

impl App {
    pub fn new() -> Self {
        App { scene: None }
    }

    fn change_scene(&mut self, data: &State) {
        {
            use Scene::*;
            self.scene = Some(match data.scene {
                Garden(_) => {
                    debug!("Changing scene to Garden");
                    WidgetPod::new(Box::new(GardenScene::new()))
                }
                Plant => {
                    debug!("Changing scene to Plant");
                    WidgetPod::new(Box::new(PlantScene::new()))
                }
            });
        }
    }
}
