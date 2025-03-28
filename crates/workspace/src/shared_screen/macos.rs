use crate::{
    item::{Item, ItemEvent},
    ItemNavHistory, WorkspaceId,
};
use anyhow::Result;
use call::participant::{Frame, RemoteVideoTrack};
use client::{proto::PeerId, User};
use futures::StreamExt;
use gpui::{
    div, surface, App, Context, Entity, EventEmitter, FocusHandle, Focusable, InteractiveElement,
    ParentElement, Render, SharedString, Styled, Task, Window,
};
use std::sync::{Arc, Weak};
use ui::{prelude::*, Icon, IconName};

pub enum Event {
    Close,
}

pub struct SharedScreen {
    track: Weak<RemoteVideoTrack>,
    frame: Option<Frame>,
    pub peer_id: PeerId,
    user: Arc<User>,
    nav_history: Option<ItemNavHistory>,
    _maintain_frame: Task<Result<()>>,
    focus: FocusHandle,
}

impl SharedScreen {
    pub fn new(
        track: Arc<RemoteVideoTrack>,
        peer_id: PeerId,
        user: Arc<User>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.focus_handle();
        let mut frames = track.frames();
        Self {
            track: Arc::downgrade(&track),
            frame: None,
            peer_id,
            user,
            nav_history: Default::default(),
            _maintain_frame: cx.spawn_in(window, |this, mut cx| async move {
                while let Some(frame) = frames.next().await {
                    this.update(&mut cx, |this, cx| {
                        this.frame = Some(frame);
                        cx.notify();
                    })?;
                }
                this.update(&mut cx, |_, cx| cx.emit(Event::Close))?;
                Ok(())
            }),
            focus: cx.focus_handle(),
        }
    }
}

impl EventEmitter<Event> for SharedScreen {}

impl Focusable for SharedScreen {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}
impl Render for SharedScreen {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .bg(cx.theme().colors().editor_background)
            .track_focus(&self.focus)
            .key_context("SharedScreen")
            .size_full()
            .children(
                self.frame
                    .as_ref()
                    .map(|frame| surface(frame.image()).size_full()),
            )
    }
}

impl Item for SharedScreen {
    type Event = Event;

    fn tab_tooltip_text(&self, _: &App) -> Option<SharedString> {
        Some(format!("{}'s screen", self.user.github_login).into())
    }

    fn deactivated(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(nav_history) = self.nav_history.as_mut() {
            nav_history.push::<()>(None, cx);
        }
    }

    fn tab_icon(&self, _window: &Window, _cx: &App) -> Option<Icon> {
        Some(Icon::new(IconName::Screen))
    }

    fn tab_content_text(&self, _window: &Window, _cx: &App) -> Option<SharedString> {
        Some(format!("{}'s screen", self.user.github_login).into())
    }

    fn telemetry_event_text(&self) -> Option<&'static str> {
        None
    }

    fn set_nav_history(
        &mut self,
        history: ItemNavHistory,
        _window: &mut Window,
        _: &mut Context<Self>,
    ) {
        self.nav_history = Some(history);
    }

    fn clone_on_split(
        &self,
        _workspace_id: Option<WorkspaceId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<Self>> {
        let track = self.track.upgrade()?;
        Some(cx.new(|cx| Self::new(track, self.peer_id, self.user.clone(), window, cx)))
    }

    fn to_item_events(event: &Self::Event, mut f: impl FnMut(ItemEvent)) {
        match event {
            Event::Close => f(ItemEvent::CloseItem),
        }
    }
}
