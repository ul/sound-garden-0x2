mod plant;

use crate::lens2::{Lens2, Lens2Wrap};
use crate::state;
use crate::ui::{constants::*, eventer};
use druid::{
    kurbo::{Affine, Line, Point, Rect, Size, Vec2},
    piet::{Color, RenderContext},
    BaseState, BoxConstraints, Command, Cursor, Data, Env, Event, EventCtx, KeyCode, KeyEvent,
    LayoutCtx, PaintCtx, UpdateCtx, WidgetPod,
};
use fake::Fake;

pub struct Widget {
    drag_start: (Point, state::Position),
    plants: Vec<
        WidgetPod<
            State,
            Lens2Wrap<plant::State, PlantNameLens, eventer::Widget<plant::State, plant::Widget>>,
        >,
    >,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct State {
    pub scene: state::GardenScene,
    pub plants: Vec<state::Plant>,
}

impl druid::Widget<State> for Widget {
    fn event(&mut self, ctx: &mut EventCtx, event: &Event, data: &mut State, env: &Env) {
        let size = ctx.size();
        let viewport = Rect::from_origin_size(Point::ORIGIN, size);
        let offset = Vec2::new(
            -size.width / 2. - data.scene.offset.x as f64,
            -size.height / 2. - data.scene.offset.y as f64,
        );
        if let Some(event) = event.transform_scroll(offset, viewport) {
            for w in &mut self.plants {
                w.event(ctx, &event, data, env);
            }
        }
        if ctx.is_handled() {
            return;
        }
        match event {
            Event::Command(Command {
                selector: cmd::REQUEST_FOCUS,
                ..
            }) => {
                ctx.request_focus();
            }
            Event::MouseDown(e) => {
                self.drag_start = (e.pos, data.scene.offset);
                ctx.set_active(true);
                ctx.set_cursor(&Cursor::OpenHand);
                ctx.request_focus();
                ctx.invalidate();
            }
            Event::MouseMoved(e) => {
                if ctx.is_active() {
                    ctx.set_cursor(&Cursor::OpenHand);
                    data.scene.offset = (
                        self.drag_start.1.x + ((e.pos.x - self.drag_start.0.x) as i32),
                        self.drag_start.1.y + ((e.pos.y - self.drag_start.0.y) as i32),
                    )
                        .into();
                    ctx.invalidate();
                }
            }
            Event::MouseUp(_) => {
                ctx.set_active(false);
                ctx.set_cursor(&Cursor::Arrow);
                ctx.invalidate();
            }
            Event::KeyDown(KeyEvent { key_code, .. }) => match key_code {
                KeyCode::Return => {
                    let (x, y) = data.scene.offset.into();
                    let ix = self.plants.iter().enumerate().find_map(|(i, p)| {
                        let layout = p.get_layout_rect();
                        if layout.contains(Point::new(-x as _, -y as _)) {
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
                                position: (-x, -y).into(),
                                name: fake::faker::name::en::Name().fake(),
                                nodes: Vec::new(),
                            };
                            log::debug!("Creating a new plant named {}", plant.name);
                            data.plants.push(plant);
                            data.plants.len() - 1
                        }
                    };
                    ctx.submit_command(cmd::zoom_to_plant(ix), None);
                    ctx.invalidate();
                }
                _ => {}
            },
            _ => {}
        }
    }

    fn update(&mut self, ctx: &mut UpdateCtx, old_data: Option<&State>, data: &State, env: &Env) {
        match old_data {
            Some(old_data) => {
                if old_data.scene.offset != data.scene.offset {
                    ctx.invalidate();
                }
                if old_data.plants != data.plants {
                    self.regenerate_plants(data);
                    ctx.invalidate();
                }
            }
            None => {
                self.regenerate_plants(data);
                ctx.invalidate();
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
        for (w, p) in self.plants.iter_mut().zip(data.plants.iter()) {
            let size = w.layout(ctx, bc, data, env);
            let (x, y) = p.position.into();
            w.set_layout_rect(Rect::from_origin_size((x as _, y as _), size));
        }
        bc.max()
    }

    fn paint(&mut self, ctx: &mut PaintCtx, base_state: &BaseState, data: &State, env: &Env) {
        ctx.save().unwrap();
        let size = base_state.size();
        let viewport = Rect::from_origin_size(Point::ORIGIN, size);
        ctx.clip(viewport);
        ctx.transform(Affine::translate((size.width / 2., size.height / 2.)));
        ctx.stroke(
            Line::new(Point::new(0., -10.), Point::new(0., 10.)),
            &Color::rgb(0.5, 0., 0.),
            1.,
        );
        ctx.stroke(
            Line::new(Point::new(-10., 0.), Point::new(10., 0.)),
            &Color::rgb(0.5, 0., 0.),
            1.,
        );
        ctx.transform(Affine::translate((
            data.scene.offset.x as _,
            data.scene.offset.y as _,
        )));
        let visible = viewport.with_origin(Point::new(
            -size.width / 2. - data.scene.offset.x as f64,
            -size.height / 2. - data.scene.offset.y as f64,
        ));
        ctx.with_child_ctx(visible, |ctx| {
            for w in &mut self.plants {
                w.paint_with_offset(ctx, data, env);
            }
        });
        ctx.restore().unwrap();
    }
}

impl Widget {
    pub fn new() -> Self {
        Widget {
            drag_start: (Point::ORIGIN, (0, 0).into()),
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
                    eventer::Widget::new(plant::Widget::new()),
                    PlantNameLens { ix },
                ))
            })
            .collect();
    }
}

struct PlantNameLens {
    ix: state::PlantIx,
}

impl Lens2<State, plant::State> for PlantNameLens {
    fn get<V, F: FnOnce(&plant::State) -> V>(&self, data: &State, f: F) -> V {
        let name = data.plants[self.ix].name.clone();
        f(&plant::State::new(self.ix, name))
    }

    fn with_mut<V, F: FnOnce(&mut plant::State) -> V>(&self, data: &mut State, f: F) -> V {
        let name = data.plants[self.ix].name.clone();
        let mut lens = plant::State::new(self.ix, name);
        let result = f(&mut lens);
        data.plants[self.ix].name = lens.name;
        result
    }
}

impl State {
    pub fn new(scene: state::GardenScene, plants: Vec<state::Plant>) -> Self {
        State { scene, plants }
    }
}

impl Data for State {
    fn same(&self, other: &Self) -> bool {
        self == other
    }
}
