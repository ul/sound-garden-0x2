use crate::lens2::{Lens2, Lens2Wrap};
use crate::state;
use crate::ui::{constants::*, text_line};
use druid::{
    kurbo::{Rect, Size},
    piet::Color,
    BaseState, BoxConstraints, Data, Env, Event, EventCtx, LayoutCtx, PaintCtx, UpdateCtx,
    WidgetPod,
};

pub struct Widget {
    nodes: Vec<WidgetPod<State, Lens2Wrap<text_line::State, NodeOpLens, text_line::Widget>>>,
}

#[derive(Clone, Data, Debug, Eq, PartialEq)]
pub struct State {
    pub scene: state::PlantScene,
    pub plant: state::Plant,
}

impl druid::Widget<State> for Widget {
    fn event(&mut self, ctx: &mut EventCtx, event: &Event, data: &mut State, env: &Env) {
        for w in &mut self.nodes {
            w.event(ctx, event, data, env);
        }
        if ctx.is_handled() {
            return;
        }
        match event {
            _ => {}
        }
    }

    fn update(&mut self, ctx: &mut UpdateCtx, old_data: Option<&State>, data: &State, env: &Env) {
        match old_data {
            Some(old_data) => {
                if old_data.scene.ix != data.scene.ix || old_data.plant.nodes != data.plant.nodes {
                    self.regenerate_nodes(data);
                    ctx.invalidate();
                }
            }
            None => {
                self.regenerate_nodes(data);
                ctx.invalidate();
            }
        }
        for w in &mut self.nodes {
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
        for (w, n) in self.nodes.iter_mut().zip(data.plant.nodes.iter()) {
            let size = w.layout(ctx, bc, data, env);
            let (x, y) = n.position.into();
            w.set_layout_rect(Rect::from_origin_size((x as _, y as _), size));
        }
        bc.max()
    }

    fn paint(&mut self, ctx: &mut PaintCtx, _base_state: &BaseState, data: &State, env: &Env) {
        for w in &mut self.nodes {
            w.paint_with_offset(ctx, data, env);
        }
    }
}

impl Widget {
    pub fn new() -> Self {
        Widget { nodes: Vec::new() }
    }

    fn regenerate_nodes(&mut self, data: &State) {
        self.nodes = data
            .plant
            .nodes
            .iter()
            .enumerate()
            .map(|(ix, _)| {
                WidgetPod::new(Lens2Wrap::new(text_line::Widget::new(), NodeOpLens { ix }))
            })
            .collect();
    }
}

struct NodeOpLens {
    ix: state::NodeIx,
}

impl Lens2<State, text_line::State> for NodeOpLens {
    fn get<V, F: FnOnce(&text_line::State) -> V>(&self, data: &State, f: F) -> V {
        let op = data.plant.nodes[self.ix].op.clone();
        f(&text_line::State::new(op, PLANT_FONT_SIZE, Color::BLACK))
    }

    fn with_mut<V, F: FnOnce(&mut text_line::State) -> V>(&self, data: &mut State, f: F) -> V {
        let op = data.plant.nodes[self.ix].op.clone();
        let mut lens = text_line::State::new(op, PLANT_FONT_SIZE, Color::BLACK);
        let result = f(&mut lens);
        data.plant.nodes[self.ix].op = lens.text;
        result
    }
}

impl State {
    pub fn new(scene: state::PlantScene, plant: state::Plant) -> Self {
        State { scene, plant }
    }
}
