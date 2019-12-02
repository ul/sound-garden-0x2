mod node;

use crate::lens2::{Lens2, Lens2Wrap};
use crate::state;
use crate::ui::{constants::*, eventer, util::find_edges};
use druid::{
    kurbo::{BezPath, Point, Rect, Size},
    piet::{Color, RenderContext},
    BaseState, BoxConstraints, Data, Env, Event, EventCtx, LayoutCtx, MouseEvent, PaintCtx,
    UpdateCtx, WidgetPod,
};

pub struct Widget(eventer::Widget<State, InnerWidget>);

struct InnerWidget {
    nodes: Vec<WidgetPod<State, Lens2Wrap<node::State, NodeOpLens, node::Widget>>>,
    edges: Vec<(state::NodeIx, state::NodeIx)>,
}

#[derive(Clone, Data, Debug, Eq, PartialEq)]
pub struct State {
    pub scene: state::PlantScene,
    pub plant: state::Plant,
}

impl druid::Widget<State> for InnerWidget {
    fn event(&mut self, ctx: &mut EventCtx, event: &Event, data: &mut State, env: &Env) {
        match event {
            Event::Command(c) if c.selector == cmd::DOUBLE_CLICK => {
                let pos = c.get_object::<MouseEvent>().unwrap().pos;
                if let Some(node) = self
                    .nodes
                    .iter_mut()
                    .find(|node| node.get_layout_rect().contains(pos))
                {
                    node.event(ctx, event, data, env);
                    return;
                }
                log::debug!("Adding a new node.");
                let (x, y) = pos.into();
                let node = state::Node {
                    op: String::from("0"),
                    position: (x as _, y as _).into(),
                };
                data.plant.nodes.push(node);
                return;
                // TODO Send EDIT command to the newly created node.
                // We can't do just regenerate_nodes here and then talk to self.nodes.last()
                // as nodes will be re-created in update. Need smarter node generation.
            }
            Event::Command(c) if c.selector == cmd::CLICK => {
                let pos = c.get_object::<MouseEvent>().unwrap().pos;
                if let Some(node) = self
                    .nodes
                    .iter_mut()
                    .find(|node| node.get_layout_rect().contains(pos))
                {
                    node.event(ctx, event, data, env);
                    return;
                }
                return;
            }
            Event::Command(c) if c.selector == cmd::REMOVE_NODE => {
                let ix = c.get_object::<state::NodeIx>().unwrap();
                log::debug!("Removing node {}.", ix);
                data.plant.nodes.swap_remove(*ix);
                return;
            }
            _ => {}
        }
        for w in &mut self.nodes {
            w.event(ctx, event, data, env);
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

    fn paint(&mut self, ctx: &mut PaintCtx, base_state: &BaseState, data: &State, env: &Env) {
        for w in &mut self.nodes {
            w.paint_with_offset(ctx, data, env);
        }
        let _size = base_state.size();
        let mut cx: f64 = 0.0;
        let mut cy: f64 = 0.0;
        for node in &data.plant.nodes {
            cx += node.position.x as f64;
            cy += node.position.y as f64;
        }
        cx /= data.plant.nodes.len() as f64;
        cy /= data.plant.nodes.len() as f64;
        for (i, j) in &self.edges {
            let p1: Point = data.plant.nodes[*i].position.into();
            let p2: Point = data.plant.nodes[*j].position.into();
            let mut curve = BezPath::new();
            curve.move_to(p1);
            // curve.quad_to((0.5 * (p1.x + p2.x), 0.5 * (p1.y + p2.y)).into(), p2);
            // curve.quad_to((0.5 * size.width, 0.5 * size.height).into(), p2);
            let mx = 0.5 * (p1.x + p2.x);
            let my = 0.5 * (p1.y + p2.y);
            curve.quad_to((mx + 0.1 * (cx - mx), my + 0.1 * (cy - my)).into(), p2);
            ctx.stroke(curve, &Color::BLACK, 1.0);
        }
    }
}

impl InnerWidget {
    fn regenerate_nodes(&mut self, data: &State) {
        self.edges = find_edges(&data.plant);
        self.nodes = data
            .plant
            .nodes
            .iter()
            .enumerate()
            .map(|(ix, _)| WidgetPod::new(Lens2Wrap::new(node::Widget::new(), NodeOpLens { ix })))
            .collect();
    }
}

impl Widget {
    pub fn new() -> Self {
        Widget(eventer::Widget::new(InnerWidget {
            nodes: Vec::new(),
            edges: Vec::new(),
        }))
    }
}

struct NodeOpLens {
    ix: state::NodeIx,
}

impl Lens2<State, node::State> for NodeOpLens {
    fn get<V, F: FnOnce(&node::State) -> V>(&self, data: &State, f: F) -> V {
        let op = data.plant.nodes[self.ix].op.clone();
        f(&node::State::new(self.ix, op))
    }

    fn with_mut<V, F: FnOnce(&mut node::State) -> V>(&self, data: &mut State, f: F) -> V {
        let op = data.plant.nodes[self.ix].op.clone();
        let mut lens = node::State::new(self.ix, op);
        let result = f(&mut lens);
        data.plant.nodes[self.ix].op = lens.op;
        result
    }
}

impl State {
    pub fn new(scene: state::PlantScene, plant: state::Plant) -> Self {
        State { scene, plant }
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
