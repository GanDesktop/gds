use gpui_kit::{
    AnyView, App, AppContext, Context, Entity, IntoElement, Render, Window, component::Root, div,
    prelude::*,
};

pub struct Surface {
    view: AnyView,
    notices: Entity<super::Overlay>,
}

pub fn wrap(view: impl Into<AnyView>, window: &mut Window, cx: &mut App) -> Entity<Surface> {
    let view = view.into();
    let notices = super::Overlay::for_window(window, cx);
    cx.new(|_| Surface { view, notices })
}

/// Preserve typed content access through the application-wide notification layer.
pub fn content(root: &Root, cx: &App) -> AnyView {
    match root.view().clone().downcast::<Surface>() {
        Ok(surface) => surface.read(cx).view.clone(),
        Err(view) => view,
    }
}

pub fn notices(window: &Window, cx: &App) -> Option<Entity<super::Overlay>> {
    super::overlay_for(window, cx)
}

impl Surface {
    fn on_focus(
        &mut self,
        _: &crate::ui::actions::FocusNotifications,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.notices
            .update(cx, |notices, cx| notices.focus_current(window, cx));
    }
    fn on_close(
        &mut self,
        _: &crate::ui::actions::CloseNotification,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.notices
            .update(cx, |notices, cx| notices.close_current(window, cx));
    }
    fn on_close_all(
        &mut self,
        _: &crate::ui::actions::CloseAllNotifications,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.notices
            .update(cx, |notices, cx| notices.close_all(window, cx));
    }
}

impl Render for Surface {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .relative()
            .size_full()
            .on_action(cx.listener(Self::on_focus))
            .on_action(cx.listener(Self::on_close))
            .on_action(cx.listener(Self::on_close_all))
            .child(self.view.clone())
            .child(self.notices.clone())
    }
}
