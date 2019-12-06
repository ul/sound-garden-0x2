mod plant;

use crate::state;
use crate::ui::{constants::*, eventer};
use druid::{
    kurbo::{Affine, Line, Point, Rect, Size, Vec2},
    piet::{Color, RenderContext},
    BaseState, BoxConstraints, Cursor, Data, Env, Event, EventCtx, LayoutCtx, Lens, LensWrap,
    MouseEvent, PaintCtx, UpdateCtx, WidgetPod,
};

pub struct Widget(eventer::Widget<State, InnerWidget>);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct State {
    pub garden_offset: state::Position,
    pub plants: Vec<state::Plant>,
}

struct InnerWidget {
    drag_start: Option<(Point, state::Position)>,
    drag_plant: Option<state::PlantIx>,
    plants: Vec<WidgetPod<State, LensWrap<plant::State, PlantNameLens, plant::Widget>>>,
}

impl druid::Widget<State> for InnerWidget {
    fn event(&mut self, ctx: &mut EventCtx, event: &Event, data: &mut State, env: &Env) {
        let size = ctx.size();
        let viewport = Rect::from_origin_size(Point::ORIGIN, size);
        let offset = Vec2::new(
            -size.width / 2. - data.garden_offset.x as f64,
            -size.height / 2. - data.garden_offset.y as f64,
        );
        match event {
            Event::Command(c) if c.selector == cmd::DOUBLE_CLICK => {
                let mut e = c.get_object::<MouseEvent>().unwrap().clone();
                e.pos += offset;
                for plant in &mut self.plants {
                    if plant.get_layout_rect().contains(e.pos) {
                        plant.event(ctx, &Event::Command(cmd::double_click(e)), data, env);
                        return;
                    }
                }
                let (x, y) = e.pos.into();
                let plant = state::Plant {
                    position: (x as _, y as _).into(),
                    name: crate::names::generate(),
                    nodes: Vec::new(),
                };
                log::debug!("Creating a new plant named {}", plant.name);
                data.plants.push(plant);
                let ix = data.plants.len() - 1;
                ctx.submit_command(cmd::zoom_to_plant(ix), None);
                return;
            }
            Event::Command(c) if c.selector == cmd::CLICK => {
                if self.drag_plant.is_some() {
                    self.drag_plant = None;
                    return;
                }
                let mut e = c.get_object::<MouseEvent>().unwrap().clone();
                e.pos += offset;
                for plant in &mut self.plants {
                    if plant.get_layout_rect().contains(e.pos) {
                        plant.event(ctx, &Event::Command(cmd::click(e)), data, env);
                        return;
                    }
                }
                return;
            }
            Event::MouseDown(e) => {
                let original_pos = e.pos;
                let mut e = e.clone();
                e.pos += offset;
                for (i, plant) in self.plants.iter_mut().enumerate() {
                    if plant.get_layout_rect().contains(e.pos) {
                        self.drag_start = None;
                        if e.mods.ctrl && e.count == 2 {
                            data.plants.swap_remove(i);
                        } else if e.mods.meta {
                            self.drag_start = Some((original_pos, data.plants[i].position));
                            self.drag_plant = Some(i);
                            ctx.set_active(true);
                            ctx.set_cursor(&Cursor::OpenHand);
                        }
                        return;
                    }
                }
                self.drag_start = Some((original_pos, data.garden_offset));
                self.drag_plant = None;
                ctx.set_active(true);
                ctx.set_cursor(&Cursor::OpenHand);
                ctx.invalidate();
            }
            Event::MouseMoved(e) => {
                if ctx.is_active() {
                    if let Some(drag_start) = self.drag_start {
                        ctx.set_cursor(&Cursor::OpenHand);
                        if let Some(ix) = self.drag_plant {
                            let p = &mut data.plants[ix].position;
                            p.x = drag_start.1.x + (e.pos.x - drag_start.0.x) as i32;
                            p.y = drag_start.1.y + (e.pos.y - drag_start.0.y) as i32;
                        } else {
                            data.garden_offset = (
                                drag_start.1.x + ((e.pos.x - drag_start.0.x) as i32),
                                drag_start.1.y + ((e.pos.y - drag_start.0.y) as i32),
                            )
                                .into();
                        }
                        ctx.invalidate();
                    }
                }
            }
            Event::MouseUp(_) => {
                self.drag_start = None;
                ctx.set_active(false);
                ctx.set_cursor(&Cursor::Arrow);
                ctx.invalidate();
            }
            _ => {}
        }
        if let Some(event) = event.transform_scroll(offset, viewport) {
            for w in &mut self.plants {
                w.event(ctx, &event, data, env);
            }
        }
    }

    fn update(&mut self, ctx: &mut UpdateCtx, old_data: Option<&State>, data: &State, env: &Env) {
        match old_data {
            Some(old_data) => {
                if old_data.garden_offset != data.garden_offset {
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
            data.garden_offset.x as _,
            data.garden_offset.y as _,
        )));
        let visible = viewport.with_origin(Point::new(
            -size.width / 2. - data.garden_offset.x as f64,
            -size.height / 2. - data.garden_offset.y as f64,
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
        Widget(eventer::Widget::new(InnerWidget {
            drag_start: None,
            drag_plant: None,
            plants: Vec::new(),
        }))
    }
}

impl InnerWidget {
    fn regenerate_plants(&mut self, data: &State) {
        self.plants = data
            .plants
            .iter()
            .enumerate()
            .map(|(ix, _)| {
                WidgetPod::new(LensWrap::new(plant::Widget::new(), PlantNameLens { ix }))
            })
            .collect();
    }
}

struct PlantNameLens {
    ix: state::PlantIx,
}

impl Lens<State, plant::State> for PlantNameLens {
    fn with<V, F: FnOnce(&plant::State) -> V>(&self, data: &State, f: F) -> V {
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
    pub fn new(garden_offset: state::Position, plants: Vec<state::Plant>) -> Self {
        State {
            garden_offset,
            plants,
        }
    }
}

impl Data for State {
    fn same(&self, other: &Self) -> bool {
        self == other
    }
}

impl druid::Widget<State> for Widget {
    fn event(&mut self, ctx: &mut EventCtx, event: &Event, data: &mut State, env: &Env) {
        self.0.event(ctx, event, data, env);
    }

    fn update(&mut self, ctx: &mut UpdateCtx, old_data: Option<&State>, data: &State, env: &Env) {
        self.0.update(ctx, old_data, data, env);
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &State,
        env: &Env,
    ) -> Size {
        self.0.layout(ctx, bc, data, env)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, base_state: &BaseState, data: &State, env: &Env) {
        self.0.paint(ctx, base_state, data, env)
    }
}
