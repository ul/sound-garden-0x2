mod node;

use crate::state;
use crate::ui::{constants::*, eventer, util::find_edges};
use druid::{
    kurbo::{BezPath, Point, Rect, Size},
    piet::{Color, RenderContext},
    BaseState, BoxConstraints, Data, Env, Event, EventCtx, LayoutCtx, Lens, LensWrap, MouseEvent,
    PaintCtx, UpdateCtx, WidgetPod,
};

pub struct Widget(eventer::Widget<State, InnerWidget>);

struct InnerWidget {
    nodes: Vec<WidgetPod<State, LensWrap<node::State, NodeOpLens, node::Widget>>>,
    edges: Vec<(state::NodeIx, state::NodeIx)>,
    drag_nodes: Vec<state::NodeIx>,
    drag_start: (Point, Vec<state::Position>),
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
                self.regenerate_nodes(data);
                self.nodes.last_mut().unwrap().event(ctx, event, data, env);
                return;
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
            Event::Command(c) if c.selector == cmd::DRAG_NODE => {
                let ix = c.get_object::<state::NodeIx>().unwrap();
                log::debug!("Dragging node {:?}", ix);
                self.drag_nodes = vec![*ix];
                self.drag_start.1 = self
                    .drag_nodes
                    .iter()
                    .map(|ix| data.plant.nodes[*ix].position)
                    .collect();
            }
            Event::Command(c) if c.selector == cmd::DRAG_SUB_TREE => {
                let i = *c.get_object::<state::NodeIx>().unwrap();
                log::debug!("Dragging sub-tree of {:?}", i);
                let edges = find_edges(&data.plant);
                let mut nodes_to_move = Vec::new();
                let mut nodes_to_scan = vec![i];
                while let Some(ix) = nodes_to_scan.pop() {
                    nodes_to_move.push(ix);
                    for ix in edges.iter().filter(|(_, j)| *j == ix).map(|(i, _)| *i) {
                        nodes_to_scan.push(ix)
                    }
                }
                self.drag_nodes = nodes_to_move;
                self.drag_start.1 = self
                    .drag_nodes
                    .iter()
                    .map(|ix| data.plant.nodes[*ix].position)
                    .collect();
            }
            Event::MouseDown(e) => {
                self.drag_start.0 = e.pos;
                ctx.set_active(true);
            }
            Event::MouseMoved(e) => {
                if ctx.is_active() {
                    let dx = (e.pos.x - self.drag_start.0.x) as i32;
                    let dy = (e.pos.y - self.drag_start.0.y) as i32;
                    for (i, &ix) in self.drag_nodes.iter().enumerate() {
                        data.plant.nodes[ix].position.x = self.drag_start.1[i].x + dx;
                        data.plant.nodes[ix].position.y = self.drag_start.1[i].y + dy;
                    }
                }
            }
            Event::MouseUp(_) => {
                ctx.set_active(false);
                self.drag_nodes.clear();
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

    fn paint(&mut self, ctx: &mut PaintCtx, _base_state: &BaseState, data: &State, env: &Env) {
        let mut cx: f64 = 0.0;
        let mut cy: f64 = 0.0;
        for node in &data.plant.nodes {
            cx += node.position.x as f64;
            cy += node.position.y as f64;
        }
        cx /= data.plant.nodes.len() as f64;
        cy /= data.plant.nodes.len() as f64;
        for (i, j) in &self.edges {
            let p1: Point = self.nodes[*i].get_layout_rect().center();
            let p2: Point = self.nodes[*j].get_layout_rect().center();
            // let p1: Point = data.plant.nodes[*i].position.into();
            // let p2: Point = data.plant.nodes[*j].position.into();
            let mut curve = BezPath::new();
            curve.move_to(p1);
            let mx = 0.5 * (p1.x + p2.x);
            let my = 0.5 * (p1.y + p2.y);
            curve.quad_to((mx + 0.1 * (cx - mx), my + 0.1 * (cy - my)).into(), p2);
            ctx.stroke(curve, &Color::grey(0.85), 1.0);
        }
        for w in &mut self.nodes {
            w.paint_with_offset(ctx, data, env);
        }
    }
}

impl InnerWidget {
    fn regenerate_nodes(&mut self, data: &State) {
        self.edges = find_edges(&data.plant);
        let mut ix = self.nodes.len();
        self.nodes.resize_with(data.plant.nodes.len(), || {
            let w = WidgetPod::new(LensWrap::new(node::Widget::new(), NodeOpLens { ix }));
            ix += 1;
            w
        })
    }
}

impl Widget {
    pub fn new() -> Self {
        Widget(eventer::Widget::new(InnerWidget {
            nodes: Vec::new(),
            edges: Vec::new(),
            drag_nodes: Vec::new(),
            drag_start: (Point::ORIGIN, Vec::new()),
        }))
    }
}

struct NodeOpLens {
    ix: state::NodeIx,
}

impl Lens<State, node::State> for NodeOpLens {
    fn with<V, F: FnOnce(&node::State) -> V>(&self, data: &State, f: F) -> V {
        let node = &data.plant.nodes[self.ix];
        f(&node::State::new(self.ix, node.op.clone()))
    }

    fn with_mut<V, F: FnOnce(&mut node::State) -> V>(&self, data: &mut State, f: F) -> V {
        let node = &mut data.plant.nodes[self.ix];
        let mut lens = node::State::new(self.ix, node.op.clone());
        let result = f(&mut lens);
        node.op = lens.op;
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
