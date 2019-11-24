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
