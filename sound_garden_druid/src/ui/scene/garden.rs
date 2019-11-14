use super::super::{constants::FONT_SIZE, label::Label};
use crate::state::{Scene, State};
use druid::{
    kurbo::{Rect, Size},
    piet::Color,
    BaseState, BoxConstraints, Env, Event, EventCtx, LayoutCtx, PaintCtx, UpdateCtx, Widget,
    WidgetPod,
};

pub struct GardenScene {
    cursor: WidgetPod<(), Label>,
}

impl Widget<State> for GardenScene {
    fn paint(&mut self, ctx: &mut PaintCtx, _base_state: &BaseState, _data: &State, env: &Env) {
        self.cursor.paint_with_offset(ctx, &(), env);
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &State,
        env: &Env,
    ) -> Size {
        if let Scene::Garden(scene_data) = &data.scene {
            let size = self.cursor.layout(ctx, bc, &(), env);
            let (x, y) = scene_data.cursor;
            self.cursor
                .set_layout_rect(Rect::from_origin_size((x as _, y as _), size));
        }
        bc.max()
    }

    fn event(&mut self, event: &Event, ctx: &mut EventCtx, data: &mut State, _env: &Env) {
        if let Scene::Garden(scene_data) = &mut data.scene {
            match event {
                Event::MouseDown(e) => {
                    scene_data.cursor.0 = e.pos.x as _;
                    scene_data.cursor.1 = e.pos.y as _;
                    ctx.invalidate();
                }
                _ => {}
            }
        } else {
            unreachable!();
        }
    }

    fn update(&mut self, ctx: &mut UpdateCtx, old_data: Option<&State>, data: &State, _env: &Env) {
        match old_data {
            Some(old_data) => {
                if let Scene::Garden(old_scene_data) = &old_data.scene {
                    if let Scene::Garden(scene_data) = &data.scene {
                        if old_scene_data.cursor != scene_data.cursor {
                            ctx.invalidate();
                        }
                    } else {
                        unreachable!();
                    }
                } else {
                    unreachable!();
                }
            }
            None => ctx.invalidate(),
        }
    }
}
impl GardenScene {
    pub fn new() -> Self {
        GardenScene {
            cursor: WidgetPod::new(Label::new(String::from("@"), FONT_SIZE, Color::BLACK)),
        }
    }
}
