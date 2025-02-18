use dap::client::SessionId;
use gpui::{list, AnyElement, Empty, Entity, FocusHandle, Focusable, ListState, Subscription};
use project::debugger::session::Session;
use ui::prelude::*;
use util::maybe;

pub struct LoadedSourceList {
    list: ListState,
    focus_handle: FocusHandle,
    _subscription: Subscription,
    session: Entity<Session>,
    client_id: SessionId,
}

impl LoadedSourceList {
    pub fn new(
        session: Entity<Session>,
        client_id: SessionId,
        cx: &mut Context<Self>,
    ) -> Self {
        let weak_entity = cx.weak_entity();
        let focus_handle = cx.focus_handle();

        let list = ListState::new(
            0,
            gpui::ListAlignment::Top,
            px(1000.),
            move |ix, _window, cx| {
                weak_entity
                    .upgrade()
                    .map(|loaded_sources| {
                        loaded_sources.update(cx, |this, cx| this.render_entry(ix, cx))
                    })
                    .unwrap_or(div().into_any())
            },
        );

        let _subscription = cx.observe(&session, |loaded_source_list, state, cx| {
            let len = state.update(cx, |state, cx| state.loaded_sources(cx).len());

            loaded_source_list.list.reset(len);
            cx.notify();
        });

        Self {
            list,
            session,
            focus_handle,
            _subscription,
            client_id,
        }
    }

    fn render_entry(&mut self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(source) = maybe!({
            self.session
                .update(cx, |state, cx| state.loaded_sources(cx).get(ix).cloned())
        }) else {
            return Empty.into_any();
        };

        v_flex()
            .rounded_md()
            .w_full()
            .group("")
            .p_1()
            .hover(|s| s.bg(cx.theme().colors().element_hover))
            .child(
                h_flex()
                    .gap_0p5()
                    .text_ui_sm(cx)
                    .when_some(source.name.clone(), |this, name| this.child(name)),
            )
            .child(
                h_flex()
                    .text_ui_xs(cx)
                    .text_color(cx.theme().colors().text_muted)
                    .when_some(source.path.clone(), |this, path| this.child(path)),
            )
            .into_any()
    }
}

impl Focusable for LoadedSourceList {
    fn focus_handle(&self, _: &gpui::App) -> gpui::FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for LoadedSourceList {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.session.update(cx, |state, cx| {
            state.loaded_sources(cx);
        });

        div()
            .track_focus(&self.focus_handle)
            .size_full()
            .p_1()
            .child(list(self.list.clone()).size_full())
    }
}
