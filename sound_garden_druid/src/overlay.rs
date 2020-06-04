use crate::theme::BACKGROUND_COLOR;
use druid::{Point, Rect, RenderContext};

pub struct Widget<T> {
    bg: Box<dyn druid::Widget<T>>,
    fg: Box<dyn druid::Widget<T>>,
}

impl<T> Widget<T> {
    pub fn new(bg: Box<dyn druid::Widget<T>>, fg: Box<dyn druid::Widget<T>>) -> Self {
        Widget { bg, fg }
    }
}

impl<T: druid::Data> druid::Widget<T> for Widget<T> {
    fn event(
        &mut self,
        ctx: &mut druid::EventCtx,
        event: &druid::Event,
        data: &mut T,
        env: &druid::Env,
    ) {
        self.fg.event(ctx, event, data, env);
        self.bg.event(ctx, event, data, env);
    }

    fn lifecycle(
        &mut self,
        ctx: &mut druid::LifeCycleCtx,
        event: &druid::LifeCycle,
        data: &T,
        env: &druid::Env,
    ) {
        self.fg.lifecycle(ctx, event, data, env);
        self.bg.lifecycle(ctx, event, data, env);
    }

    fn update(&mut self, ctx: &mut druid::UpdateCtx, old_data: &T, data: &T, env: &druid::Env) {
        self.bg.update(ctx, old_data, data, env);
        self.fg.update(ctx, old_data, data, env);
    }

    fn layout(
        &mut self,
        _ctx: &mut druid::LayoutCtx,
        bc: &druid::BoxConstraints,
        _data: &T,
        _env: &druid::Env,
    ) -> druid::Size {
        bc.max()
    }

    fn paint(&mut self, ctx: &mut druid::PaintCtx, data: &T, env: &druid::Env) {
        let size = ctx.size();

        // Clean.
        let frame = Rect::from_origin_size(Point::ORIGIN, size);
        ctx.fill(frame, &BACKGROUND_COLOR);

        self.bg.paint(ctx, data, env);
        self.fg.paint(ctx, data, env);
    }
}
