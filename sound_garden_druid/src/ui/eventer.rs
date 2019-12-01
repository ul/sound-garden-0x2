use crate::ui::{constants::*, util::EventExt};
use druid::{
    kurbo::Size, BaseState, BoxConstraints, Data, Env, Event, EventCtx, LayoutCtx, MouseEvent,
    PaintCtx, TimerToken, UpdateCtx,
};
use std::marker::PhantomData;

pub struct Widget<T: Data, W: druid::Widget<T>> {
    inner: W,
    click_cnt: u8,
    click_event: Option<MouseEvent>,
    dbl_click_timer: TimerToken,
    phantom: PhantomData<T>,
}

impl<State: Data, W: druid::Widget<State>> druid::Widget<State> for Widget<State, W> {
    fn event(&mut self, ctx: &mut EventCtx, event: &Event, data: &mut State, env: &Env) {
        match event {
            Event::MouseDown(e) => {
                ctx.set_active(e.inside_widget(&ctx));
                if ctx.is_active() {
                    ctx.set_handled();
                }
            }
            Event::MouseUp(e) => {
                if ctx.is_active() && e.inside_widget(&ctx) {
                    self.click_cnt += 1;
                    if self.click_cnt == 1 {
                        self.click_event = Some(e.clone());
                        self.dbl_click_timer =
                            ctx.request_timer(std::time::Instant::now() + DOUBLE_CLICK_TIMEOUT);
                    } else {
                        log::debug!("Double click!");
                        self.inner.event(
                            ctx,
                            &Event::Command(cmd::double_click(e.clone())),
                            data,
                            env,
                        );
                        self.click_cnt = 0;
                        self.click_event = None;
                    }
                    ctx.set_handled();
                } else {
                    self.click_cnt = 0;
                    self.click_event = None;
                }
                ctx.set_active(false);
            }
            Event::Timer(t) if *t == self.dbl_click_timer => {
                if self.click_cnt == 1 && !ctx.is_active() {
                    if let Some(e) = self.click_event.take() {
                        log::debug!("Click!");
                        self.inner
                            .event(ctx, &Event::Command(cmd::click(e)), data, env);
                    }
                }
                self.click_cnt = 0;
                self.click_event = None;
            }
            _ => {}
        }
        self.inner.event(ctx, event, data, env);
    }

    fn update(&mut self, ctx: &mut UpdateCtx, old_data: Option<&State>, data: &State, env: &Env) {
        self.inner.update(ctx, old_data, data, env);
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &State,
        env: &Env,
    ) -> Size {
        self.inner.layout(ctx, bc, data, env)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, base_state: &BaseState, data: &State, env: &Env) {
        self.inner.paint(ctx, base_state, data, env);
    }
}

impl<T: Data, W: druid::Widget<T>> Widget<T, W> {
    pub fn new(inner: W) -> Self {
        Widget {
            inner,
            click_cnt: 0,
            click_event: None,
            dbl_click_timer: TimerToken::INVALID,
            phantom: Default::default(),
        }
    }
}
