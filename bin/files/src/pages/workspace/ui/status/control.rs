use super::{CONTROL_SIZE, StatusSummary};
use gds::ui::button::Button;
use gds_uim::{component, el};
use gpui_kit::{
    IntoElement,
    component::{
        ThemeStyled,
        button::ButtonVariants as _,
        popover::{Popover, PopoverState},
    },
    prelude::*,
    *,
};
use std::rc::Rc;

type OpenHandler = Rc<dyn Fn(&mut Window, &mut App)>;

#[component]
#[derive(IntoElement)]
pub(in crate::pages::workspace::ui) struct StatusControl {
    #[attr]
    id: &'static str,
    #[attr(into)]
    title: SharedString,
    #[attr]
    summary: StatusSummary,
    #[attr(option, into)]
    details: Option<AnyView>,
    #[attr(option)]
    on_open: Option<OpenHandler>,
}

impl Default for StatusControl {
    fn default() -> Self {
        Self {
            id: "status-control",
            title: SharedString::default(),
            summary: StatusSummary::default(),
            details: None,
            on_open: None,
        }
    }
}

impl RenderOnce for StatusControl {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let Self {
            id,
            title,
            summary,
            details,
            on_open,
        } = self;
        let trigger = el! {
            id.Button::new {
                ghost; px_0; h: CONTROL_SIZE.px; text_xs;
                accessibility_label: ~title; tooltip: title;
            } [summary { # id; }]
        };
        let content =
            move |_: &mut PopoverState, _: &mut Window, cx: &mut Context<PopoverState>| {
                el! { .div { popover_style: cx; } [
                    @if (let Some(details) = details.clone()) details, @end,
                ] }
            };
        let on_open_change = move |open: &bool, window: &mut Window, cx: &mut App| {
            if *open && let Some(on_open) = &on_open {
                on_open(window, cx);
            }
        };
        el! {
            id.Popover::new {
                appearance: false; anchor: Anchor::BottomLeft;
                trigger: trigger; content: content;
                on_open_change: on_open_change;
            }
        }
    }
}
