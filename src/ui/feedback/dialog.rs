//! Anatomical dialog content, hosted by AntiWebUI's modal and focus lifecycle.
use crate::ui::{
    Icon,
    container::container_traits,
    keytips::{Anchor, DialogScope},
};
use anti_webui::prelude::*;
use gds_uim::el;
use std::rc::Rc;

const SLIDE: Transition = Transition::ms(250).cubic_bezier(1.0 / 3.0, 0.72, 2.0 / 3.0, 1.0);
const ALERT_INSET: f32 = 60.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogCommand {
    Confirm,
    Cancel,
}

impl DialogCommand {
    pub const ALL: [Self; 2] = [Self::Confirm, Self::Cancel];

    pub fn label_id(self) -> &'static str {
        match self {
            Self::Confirm => "dialog-confirm",
            Self::Cancel => "dialog-cancel",
        }
    }

    pub fn default_stroke(self) -> &'static str {
        match self {
            Self::Confirm => "alt-a",
            Self::Cancel => "alt-c",
        }
    }

    fn matches(self, event: &KeyboardEvent) -> bool {
        let modifiers = event.modifiers;
        modifiers.alt
            && !modifiers.ctrl
            && !modifiers.logo
            && !modifiers.shift
            && matches!(&event.key, Key::Character(key) if key.eq_ignore_ascii_case(&self.default_stroke()[4..]))
    }
}

pub struct Dialog {
    base: Div,
    title: Option<View>,
    body: Div,
    actions: Option<View>,
}
impl Default for Dialog {
    fn default() -> Self {
        Self {
            base: el! { .div.{
                flex; flex_col; min_h: 0.0_f32; min_w: 0.0_f32;
                w: 360.0_f32; max_w: $(pct(100.0)); max_h: $(pct(100.0));
                gap: 16.0_f32; p: 20.0_f32;
            } },
            title: None,
            body: el! { .div.{
                flex; flex_col; min_h: 0.0_f32; min_w: 0.0_f32;
                gap: 16.0_f32; overflow_y_scroll;
            } },
            actions: None,
        }
    }
}
impl Dialog {
    pub fn title(mut self, title: impl Into<String>) -> Self {
        let title = title.into();
        self.title = Some(
            el! { title.text.{
                flex_none;
                dyn_style: s => $(s.text_size(crate::app::Gds::current().appearance().typography().scaled(18.0)));
                font_weight: FontWeight::SEMIBOLD;
            } }
            .into_view(),
        );
        self
    }
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.body = self.body.child(child);
        self
    }
    pub fn children<V: IntoView>(mut self, children: impl IntoIterator<Item = V>) -> Self {
        self.body = self.body.children(children);
        self
    }
    pub fn actions(mut self, actions: impl IntoView) -> Self {
        self.actions = Some(actions.into_view());
        self
    }
    pub fn decisions(mut self, decisions: DecisionActions) -> Self {
        let node = node_ref();
        let buttons = decisions.buttons.clone();
        self.base = el! { self->base.{
            node_ref:node; focusable;
            on_key_down: move event => ${
                if let Some(button) = buttons.iter().find(|button| button.command.matches(event)) {
                    event.prevent_default();
                    event.stop_propagation();
                    button.activate();
                }
            };
        } };
        request_frame(move |_| node.focus());
        self.actions = Some(decisions.into_view());
        self
    }
    pub fn show<V: IntoView>(
        open: impl Fn() -> bool + 'static,
        dismiss: impl Fn() + 'static,
        content: impl Fn() -> V + 'static,
    ) -> Show {
        Self::present(open, dismiss, content, false)
    }
    pub fn show_alert<V: IntoView>(
        open: impl Fn() -> bool + 'static,
        dismiss: impl Fn() + 'static,
        content: impl Fn() -> V + 'static,
    ) -> Show {
        Self::present(open, dismiss, content, true)
    }
    fn present<V: IntoView>(
        open: impl Fn() -> bool + 'static,
        dismiss: impl Fn() + 'static,
        content: impl Fn() -> V + 'static,
        alert: bool,
    ) -> Show {
        let content = Rc::new(content);
        anti_webui::widgets::modal_with(open, dismiss, move |modal| {
            let hints = DialogScope::new(move || modal.open());
            let visible = follow(move || if modal.open() { 1.0_f32 } else { 0.0_f32 }, SLIDE);
            modal.hold_while(move || visible.get() > 0.0);
            move || {
                let node = node_ref();
                request_frame(move |_| node.focus());
                let content = content.clone();
                let scrim = el! { .div.{ absolute; inset: 0.0_f32;
                    dyn_style: move s => ${
                            let light = crate::app::Gds::current().appearance().theme().mode()
                                == crate::appearance::theme::Mode::Light;
                            let alpha = if light { 13.0 / 255.0 } else { 51.0 / 255.0 };
                            s.bg(rgb(0x000000).with_alpha(alpha * visible.get()))
                    };
                } };
                let surface = el! { .div.{
                    flex; flex_col; min_w: 0.0_f32; min_h: 0.0_f32;
                    max_w: 100.0_f32.pct; max_h: 100.0_f32.pct;
                    rounded: 8.0_f32; border: 1.0_f32; shadow_lg; overflow_clip;
                    focusable; node_ref: node;
                        dyn_style: move s => ${
                            let progress = visible.get();
                            let travel = if alert { ALERT_INSET } else { 16.0 };
                            s.bg(theme().overlay).border_color(theme().border)
                                .text_color(theme().text).opacity(progress)
                                .translate_y(-travel * (1.0 - progress))
                        };
                    on_key_down: move event => ${
                        hints.key(event, true);
                        if DialogCommand::Cancel.matches(event) {
                            event.prevent_default();
                            event.stop_propagation();
                            modal.dismiss();
                        }
                    };
                    on_key_up: move event => $(hints.key(event, false));
                }.[
                    (move () => $(hints.view(|| content().into_view()))).dyn_view,
                ] };
                let scrim = if alert {
                    scrim
                        .pointer_events_auto()
                        .on_click(|event| event.stop_propagation())
                } else {
                    modal.scrim(scrim)
                };
                el! { .div.{ size_full }.[
                        scrim,
                    .div.{
                        absolute; inset: 0.0_f32; flex; justify_center; p: 24.0_f32;
                            if alert { items_start; pt: ALERT_INSET } else { items_center };
                    }.[ $(modal.surface(surface)) ],
                ] }
            }
        })
    }
}
impl Styled for Dialog {
    fn style_mut(&mut self) -> &mut Style {
        self.base.style_mut()
    }
}
impl IntoView for Dialog {
    fn into_view(self) -> View {
        el! { self->base.[
            if let Some(title) = self->title [title],
            self->body,
            if let Some(actions) = self->actions [actions],
        ] }
        .into_view()
    }
}

pub struct DialogActions {
    base: Div,
}
impl Default for DialogActions {
    fn default() -> Self {
        Self {
            base: el! { .div.{ flex; flex_wrap; flex_none; justify_end; items_center; gap: 8.0_f32; mt: 8.0_f32 } },
        }
    }
}
container_traits!(DialogActions);

/// Commands for an alert with cancel and confirm controls in its footer.
#[derive(Default)]
pub struct DecisionActions {
    buttons: Vec<DialogDecision>,
}

impl DecisionActions {
    pub fn child(mut self, decision: DialogDecision) -> Self {
        self.buttons.push(decision);
        self
    }
}

impl IntoView for DecisionActions {
    fn into_view(self) -> View {
        el! { .DialogActions::default.[..self->buttons] }.into_view()
    }
}

/// A dialog action with one availability predicate and one activation path.
#[derive(Clone)]
pub struct DialogDecision {
    command: DialogCommand,
    label: Option<String>,
    icon: Icon,
    disabled: Rc<dyn Fn() -> bool>,
    on_activate: Rc<dyn Fn()>,
}

impl DialogDecision {
    pub fn confirm(on_activate: impl Fn() + 'static) -> Self {
        Self::new(DialogCommand::Confirm, Icon::Check, on_activate)
    }

    pub fn cancel(on_activate: impl Fn() + 'static) -> Self {
        Self::new(DialogCommand::Cancel, Icon::Close, on_activate)
    }

    fn new(command: DialogCommand, icon: Icon, on_activate: impl Fn() + 'static) -> Self {
        Self {
            command,
            icon,
            label: None,
            disabled: Rc::new(|| false),
            on_activate: Rc::new(on_activate),
        }
    }

    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = icon;
        self
    }

    pub fn disabled_when(mut self, disabled: impl Fn() -> bool + 'static) -> Self {
        self.disabled = Rc::new(disabled);
        self
    }

    pub fn activate(&self) {
        if !(self.disabled)() {
            (self.on_activate)();
        }
    }

    fn sheet(command: DialogCommand) -> Rc<Sheet> {
        use anti_webui::widgets::{focus_ring, themed};
        let build: fn(&Theme) -> Sheet = match command {
            DialogCommand::Confirm => |theme: &Theme| {
                Sheet::new(|style| style.bg(theme.accent).text_color(theme.on_accent))
                    .hover(|style| style.bg(theme.accent_hover))
                    .active(|style| style.bg(theme.accent_pressed))
                    .focus_visible(|style| focus_ring(style, theme))
                    .disabled(|style| style.bg(theme.surface).text_color(theme.text_disabled))
            },
            DialogCommand::Cancel => |theme: &Theme| {
                let colors = crate::app::Gds::current().appearance().theme().colors();
                Sheet::new(|style| {
                    style
                        .bg(colors.surface)
                        .text_color(colors.surface_foreground)
                        .border_color(theme.border_strong)
                })
                .hover(|style| style.bg(colors.surface.mix(colors.surface_foreground, 0.08)))
                .active(|style| style.bg(colors.surface.mix(colors.surface_foreground, 0.12)))
                .focus_visible(|style| focus_ring(style, theme))
                .disabled(|style| style.text_color(theme.text_disabled))
            },
        };
        themed(build)
    }
}

impl IntoView for DialogDecision {
    fn into_view(self) -> View {
        use crate::appearance::tokens::{ICON_BUTTON_SIZE, ICON_SIZE, SMALL};
        let command = self.command;
        let label = self
            .label
            .clone()
            .unwrap_or_else(|| crate::i18n::text(command.label_id()));
        let accessible = label.clone();
        let disabled = self.disabled.clone();
        let hint_disabled = disabled.clone();
        let icon = self.icon;
        el! { .div.{
            flex; flex_none; items_center; justify_center; focusable; select_none;
            h:ICON_BUTTON_SIZE; px:10.0_f32; gap:8.0_f32; rounded:6.0_f32;
            if $(command == DialogCommand::Cancel) { border:1.0_f32 };
            sheet:move () => command.Self::sheet;
            disabled:move () => .disabled;
            access:move a => $(a.role(Role::Button).label(accessible.clone()));
            on_click:move _ => &self.{activate};
            dyn_style:s => $(s.text_size(crate::app::Gds::current().appearance().typography().scaled(SMALL)));
        }.[
            $(icon.view(ICON_SIZE)),
            .div.{flex; items_center; min_w:0.0_f32}.[
                label.text.{ellipsis},
                $(Anchor::new(command.default_stroke()).inline().when(move || !hint_disabled())),
            ],
        ] }.into_view()
    }
}
