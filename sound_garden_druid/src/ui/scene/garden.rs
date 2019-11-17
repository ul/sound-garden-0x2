use super::super::{constants, text_line};
use crate::lens2::{Lens2, Lens2Wrap};
use crate::state::{self, Scene, State};
use druid::{
    kurbo::{Point, Rect, Size},
    piet::Color,
    BaseState, BoxConstraints, Env, Event, EventCtx, KeyCode, KeyEvent, LayoutCtx, PaintCtx,
    UpdateCtx, Widget, WidgetPod,
};
use fake::Fake;

pub struct GardenScene {
    cursor: WidgetPod<text_line::State, text_line::TextLine>,
    plants: Vec<WidgetPod<State, Lens2Wrap<text_line::State, PlantNameLens, text_line::TextLine>>>,
}

impl Widget<State> for GardenScene {
    fn event(&mut self, event: &Event, ctx: &mut EventCtx, data: &mut State, env: &Env) {
        // TODO Resolve double-handle of Return etc.
        for w in &mut self.plants {
            w.event(event, ctx, data, env);
        }
        if let Scene::Garden(scene_data) = &mut data.scene {
            match event {
                Event::MouseDown(e) => {
                    ctx.set_active(true);
                    ctx.request_focus();
                    scene_data.cursor.x = e.pos.x as _;
                    scene_data.cursor.y = e.pos.y as _;
                    ctx.invalidate();
                }
                Event::MouseMoved(e) => {
                    if ctx.is_active() {
                        scene_data.cursor.x = e.pos.x as _;
                        scene_data.cursor.y = e.pos.y as _;
                        ctx.invalidate();
                    }
                }
                Event::MouseUp(_) => {
                    ctx.set_active(false);
                    ctx.invalidate();
                }
                Event::KeyDown(KeyEvent { key_code, .. }) => match key_code {
                    KeyCode::Return => {
                        data.scene = {
                            let ix = self.plants.iter().enumerate().find_map(|(i, p)| {
                                let layout = p.get_layout_rect();
                                let (x, y) = scene_data.cursor.into();
                                if layout.contains(Point::new(x as _, y as _)) {
                                    Some(i)
                                } else {
                                    None
                                }
                            });
                            let ix = match ix {
                                Some(ix) => {
                                    log::debug!("Entering plant {}", ix);
                                    ix
                                }
                                None => {
                                    let plant = state::Plant {
                                        position: scene_data.cursor,
                                        name: fake::faker::name::en::Name().fake(),
                                        nodes: Vec::new(),
                                    };
                                    log::debug!("Creating a new plant named {}", plant.name);
                                    data.plants.push(plant);
                                    data.plants.len() - 1
                                }
                            };
                            Scene::Plant(state::PlantScene {
                                ix,
                                cursor: (0, 0).into(),
                                mode: state::PlantSceneMode::Normal,
                            })
                        };
                        ctx.invalidate();
                    }
                    _ => {}
                },
                _ => {}
            }
        } else {
            unreachable!();
        }
    }

    fn update(&mut self, ctx: &mut UpdateCtx, old_data: Option<&State>, data: &State, env: &Env) {
        match old_data {
            Some(old_data) => {
                if let Scene::Garden(old_scene_data) = &old_data.scene {
                    if let Scene::Garden(scene_data) = &data.scene {
                        if old_scene_data.cursor != scene_data.cursor {
                            ctx.invalidate();
                        }
                        if old_data.plants != data.plants {
                            self.regenerate_plants(data);
                            ctx.invalidate();
                        }
                    } else {
                        unreachable!();
                    }
                } else {
                    unreachable!();
                }
            }
            None => {
                self.regenerate_plants(data);
                ctx.invalidate()
            }
        }
        for w in &mut self.plants {
            w.update(ctx, data, env);
        }
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &State,
        env: &Env,
    ) -> Size {
        if let Scene::Garden(scene_data) = &data.scene {
            let size = self.cursor.layout(
                ctx,
                bc,
                &text_line::State::new(String::from("@"), constants::ANIMA_FONT_SIZE, Color::BLACK),
                env,
            );
            let (x, y) = scene_data.cursor.into();
            self.cursor
                .set_layout_rect(Rect::from_origin_size((x as _, y as _), size));
        }
        for (w, p) in self.plants.iter_mut().zip(data.plants.iter()) {
            let size = w.layout(ctx, bc, data, env);
            let (x, y) = p.position.into();
            w.set_layout_rect(Rect::from_origin_size((x as _, y as _), size));
        }
        bc.max()
    }

    fn paint(&mut self, ctx: &mut PaintCtx, _base_state: &BaseState, data: &State, env: &Env) {
        self.cursor.paint_with_offset(
            ctx,
            &text_line::State::new(String::from("@"), constants::ANIMA_FONT_SIZE, Color::BLACK),
            env,
        );
        for w in &mut self.plants {
            w.paint_with_offset(ctx, data, env);
        }
    }
}
impl GardenScene {
    pub fn new() -> Self {
        GardenScene {
            cursor: WidgetPod::new(text_line::TextLine::new()),
            plants: Vec::new(),
        }
    }

    fn regenerate_plants(&mut self, data: &State) {
        self.plants = data
            .plants
            .iter()
            .enumerate()
            .map(|(ix, _)| {
                WidgetPod::new(Lens2Wrap::new(
                    text_line::TextLine::editable(),
                    PlantNameLens { ix },
                ))
            })
            .collect();
    }
}

struct PlantNameLens {
    ix: state::PlantIx,
}

impl Lens2<State, text_line::State> for PlantNameLens {
    fn get<V, F: FnOnce(&text_line::State) -> V>(&self, data: &State, f: F) -> V {
        let name = data.plants[self.ix].name.clone();
        f(&text_line::State::new(
            name,
            constants::PLANT_FONT_SIZE,
            Color::BLACK,
        ))
    }

    fn with_mut<V, F: FnOnce(&mut text_line::State) -> V>(&self, data: &mut State, f: F) -> V {
        let name = data.plants[self.ix].name.clone();
        let mut lens = text_line::State::new(name, constants::PLANT_FONT_SIZE, Color::BLACK);
        let result = f(&mut lens);
        data.plants[self.ix].name = lens.text;
        result
    }
}
