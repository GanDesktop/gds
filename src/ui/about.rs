//! Shared About content, independent of its dialog, page or window host.

mod metadata;

pub use metadata::{APPLICATIONS, Application, Credit};

use gds_uim::el;
use gpui_kit::{
    assets::IconName,
    base::SelectableText,
    component::{
        Icon, Sizable as _,
        clipboard::Clipboard,
        description_list::{DescriptionItem, DescriptionList},
        dialog::DialogTitle,
        link::Link,
    },
    prelude::*,
    *,
};

use crate::{
    appearance::tokens,
    i18n::t,
    ui::{
        IconButton, Shortcut,
        button::Button,
        page::{ContentPage, PageContent, PageHeader, PageTitle},
    },
};

pub const WIDTH: f32 = 480.;
pub const HEIGHT: f32 = 360.;
const DETAILS_CONTEXT: &str = "AboutDetails";

gpui_kit::actions!(gds_about, [Back]);

#[derive(Clone, Copy, Default, Eq, PartialEq)]
enum Section {
    #[default]
    Overview,
    Credits,
    License,
}

pub struct About {
    application: &'static Application,
    section: Section,
    focus: FocusHandle,
}

impl About {
    pub fn new(application: &'static Application, cx: &mut Context<Self>) -> Self {
        cx.bind_keys([
            KeyBinding::new("escape", Back, Some(DETAILS_CONTEXT)),
            KeyBinding::new("alt-left", Back, Some(DETAILS_CONTEXT)),
        ]);
        Self {
            application,
            section: Section::Overview,
            focus: cx.focus_handle(),
        }
    }

    fn show_credits(&mut self, _: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.show(Section::Credits, window, cx);
    }

    fn show_license(&mut self, _: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.show(Section::License, window, cx);
    }

    fn show_overview(&mut self, _: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.show(Section::Overview, window, cx);
    }

    fn show(&mut self, section: Section, window: &mut Window, cx: &mut Context<Self>) {
        self.section = section;
        self.focus.focus(window, cx);
        cx.notify();
    }

    fn on_back(&mut self, _: &Back, window: &mut Window, cx: &mut Context<Self>) {
        cx.stop_propagation();
        self.show(Section::Overview, window, cx);
    }

    fn identity(
        id: &'static str,
        label: SharedString,
        value: &'static str,
        cx: &App,
    ) -> DescriptionItem {
        let accessible: SharedString = format!("{label}: {value}").into();
        let content = el! {
            .div { flex; items_center; gap_2; min_w_0; } [
                (format!("{id}-text"), value).SelectableText::new,
                id.Clipboard::new {
                    value: value; accessibility_label: accessible;
                    tooltip: t!(@cx, "gds-about-copy");
                },
            ]
        };
        DescriptionItem::new(label).value(content.into_any_element())
    }

    fn overview(&self, cx: &mut Context<Self>) -> AnyElement {
        let app = self.application;
        let version = Self::identity(
            "about-version",
            t!(@cx, "gds-about-version"),
            app.version,
            cx,
        );
        let build = Self::identity(
            "about-build-tag",
            t!(@cx, "gds-about-build-tag"),
            app.build_tag,
            cx,
        );
        let on_credits = cx.listener(Self::show_credits);
        let on_license = cx.listener(Self::show_license);
        el! {
            .div { # "about-overview"; flex; flex_col; min_h_0; gap_4; } [
                .DescriptionList::new { columns: 1; bordered: false; small; } [version, build],
                .div { flex; flex_wrap; items_center; gap_4; } [
                    "about-website".Link::new { href: $app.website; } [t!(@cx, "gds-about-website")],
                    "about-feedback".Link::new { href: $app.feedback; } [t!(@cx, "gds-about-feedback")],
                ],
                .div { flex; flex_wrap; gap_2; } [
                    "about-credits".Button::new { small; icon: IconName::Info;
                        label: t!(@cx, "gds-about-credits"); on_click: on_credits;
                    },
                    "about-license".Button::new { small; icon: IconName::Scale;
                        label: t!(@cx, "gds-about-license"); on_click: on_license;
                    },
                ],
            ]
        }.into_any_element()
    }

    fn credits(&self, cx: &App) -> AnyElement {
        let app = self.application;
        let projects = app.credits.iter().map(|credit| {
            el! {
                $(credit.name).Link::new { href: $credit.website; } [$credit.name]
            }
        });
        let projects = el! { .div { flex; flex_col; items_start; gap_2; } [..projects] };
        let authors =
            DescriptionItem::new(t!(@cx, "gds-about-authors")).value(app.authors.join(", "));
        let projects =
            DescriptionItem::new(t!(@cx, "gds-about-projects")).value(projects.into_any_element());
        el! {
            .div { flex; flex_col; gap_4; } [
                .DescriptionList::new { columns: 1; bordered: false; small; } [authors, projects],
            ]
        }
        .into_any_element()
    }

    fn details(&self, cx: &mut Context<Self>) -> AnyElement {
        let title = if self.section == Section::Credits {
            t!(@cx, "gds-about-credits")
        } else {
            t!(@cx, "gds-about-license")
        };
        let on_back = cx.listener(Self::show_overview);
        let shortcut = Shortcut::new(Box::new(Back), Some(DETAILS_CONTEXT));
        let content = match self.section {
            Section::Credits => self.credits(cx),
            Section::License => el! {
                .div { flex; flex_col; gap_4; text_sm; } [
                    .div { font_weight: FontWeight::SEMIBOLD; } [$self.application.license],
                    .div { whitespace_normal; } [
                        ("about-license-text", $self.application.license_text).SelectableText::new,
                    ],
                ]
            }
            .into_any_element(),
            Section::Overview => unreachable!("the overview has its own body"),
        };
        el! {
            .div { flex; flex_col; flex_1; min_h_0; gap_4; } [
                .div { flex; items_center; flex_shrink_0; gap_2; } [
                    "about-back".IconButton::new { icon: IconName::ArrowLeft;
                        label: t!(@cx, "gds-about-back"); shortcut: shortcut; on_click: on_back;
                    },
                    .DialogTitle::new [title],
                ],
                .div { # "about-details"; flex_1; min_h_0; min_w_0; overflow_y_scroll; } [content],
            ]
        }
        .into_any_element()
    }
}

impl Render for About {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let app = self.application;
        let body = if self.section == Section::Overview {
            self.overview(cx)
        } else {
            self.details(cx)
        };
        el! {
            .ContentPage::default { # "about-content"; text_size: tokens::SMALL;
                track_focus: $&self.focus; on_action: (cx, Self::on_back).Context::listener;
                @if (self.section != Section::Overview) key_context: DETAILS_CONTEXT; @end;
            } [
                .PageHeader::default [
                    .Icon::default { path: $app.icon; size: tokens::SIDEBAR_ICON_SIZE.px; flex_shrink_0; },
                    .PageTitle::default [$app.name],
                ],
                .PageContent::default { px_6; pb_4; } [body],
            ]
        }
    }
}

impl Focusable for About {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

#[cfg(all(test, feature = "gui-tests"))]
mod tests;
