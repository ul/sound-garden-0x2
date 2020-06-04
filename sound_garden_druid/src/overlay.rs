use crate::theme::BACKGROUND_COLOR;
use druid::{Point, Rect, RenderContext, WidgetPod};

pub struct Widget<T> {
    bg: WidgetPod<T, Box<dyn druid::Widget<T>>>,
    fg: WidgetPod<T, Box<dyn druid::Widget<T>>>,
}

impl<T> Widget<T> {
    pub fn new(bg: impl druid::Widget<T> + 'static, fg: impl druid::Widget<T> + 'static) -> Self {
        Widget {
            bg: WidgetPod::new(bg).boxed(),
            fg: WidgetPod::new(fg).boxed(),
        }
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

    fn update(&mut self, ctx: &mut druid::UpdateCtx, _old_data: &T, data: &T, env: &druid::Env) {
        self.bg.update(ctx, data, env);
        self.fg.update(ctx, data, env);
    }

    fn layout(
        &mut self,
        ctx: &mut druid::LayoutCtx,
        bc: &druid::BoxConstraints,
        data: &T,
        env: &druid::Env,
    ) -> druid::Size {
        let size = bc.max();
        self.bg.set_layout_rect(ctx, data, env, size.to_rect());
        self.fg.set_layout_rect(ctx, data, env, size.to_rect());
        size
    }

    fn paint(&mut self, ctx: &mut druid::PaintCtx, data: &T, env: &druid::Env) {
        let size = ctx.size();
        let frame = Rect::from_origin_size(Point::ORIGIN, size);
        ctx.fill(frame, &BACKGROUND_COLOR);

        self.bg.paint(ctx, data, env);
        self.fg.paint(ctx, data, env);
    }
}
