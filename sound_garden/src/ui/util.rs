use crate::{state, ui::constants::*};
use druid::{
    kurbo::{Point, Rect},
    EventCtx, MouseEvent,
};

pub trait EventExt {
    fn inside_widget(&self, ctx: &EventCtx) -> bool;
}

impl EventExt for MouseEvent {
    fn inside_widget(&self, ctx: &EventCtx) -> bool {
        Rect::from_origin_size(Point::ORIGIN, ctx.size()).contains(self.pos)
    }
}

// Inefficient as hell, but good enough for the start.
pub fn find_edges(plant: &state::Plant) -> Vec<(state::NodeIx, state::NodeIx)> {
    let mut edges = Vec::new();
    let state::Plant { nodes, .. } = plant;
    for (i, node) in nodes.iter().enumerate() {
        let mut parent = None;
        for (j, n) in nodes
            .iter()
            .enumerate()
            .filter(|(j, n)| i != *j && n.position.y >= node.position.y + (PLANT_FONT_SIZE as i32))
        {
            let dist =
                (n.position.x - node.position.x).pow(2) + (n.position.y - node.position.y).pow(2);
            match parent {
                Some((_, d)) => {
                    if dist < d {
                        parent = Some((j, dist));
                    }
                }

                None => parent = Some((j, dist)),
            }
        }
        if let Some((j, _)) = parent {
            edges.push((i, j));
        }
    }
    edges.sort_by(|(i1, _), (i2, _)| nodes[*i1].position.x.cmp(&nodes[*i2].position.x));
    edges
}
