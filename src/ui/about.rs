//! Application identity, credits and license shared by native hosts.
mod metadata;
use crate::{
    app::Gds,
    ui::{Icon, IconButton, page::*},
};
use anti_webui::prelude::*;
use gds_uim::el;
pub use metadata::{APPLICATIONS, Application, Credit};
pub const WIDTH: f32 = 480.0;
pub const HEIGHT: f32 = 360.0;
#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    Overview,
    Credits,
    License,
}
pub struct About {
    application: &'static Application,
    section: Signal<Section>,
    error: Signal<Option<String>>,
    focus: NodeRef,
}
impl About {
    pub fn new(application: &'static Application) -> Self {
        Self {
            application,
            section: signal(Section::Overview),
            error: signal(None),
            focus: node_ref(),
        }
    }
    fn identity(label: &'static str, value: &'static str) -> Div {
        el! { .div.{flex;items_center;gap: 8.0_f32}.[
            (move () => label.crate::i18n::text).text.{w: 120.0_f32}, value.text.{flex_1;select_text},
            label.IconButton::new.{icon:Icon::Copy;label_fn:()=>"gds-about-copy".crate::i18n::text;on_click:_=>$(set_clipboard(ClipboardKind::Clipboard,value.to_owned()))},
        ] }
    }
    fn website(&self, label: impl Fn() -> String + 'static, url: &'static str) -> Div {
        let error = self.error;
        el! { label.link.{on_click:move _=>${
            Gds::current().tasks().spawn("Open project website", async move {
                tokio::process::Command::new("xdg-open").arg(url).status().await
            }).then(move |result| match result {
                Ok(Ok(status)) if status.success()=>error.set(None),
                Ok(Ok(status))=>error.set(Some(format!("xdg-open: {status}"))),
                Ok(Err(problem))=>error.set(Some(problem.to_string())),
                Err(problem)=>error.set(Some(problem.to_string())),
            });
        }} }
    }
    fn content(&self) -> Div {
        let app = self.application;
        let section = self.section;
        match section.get() {
            Section::Overview => el! {.div.{flex;flex_col;gap: 12.0_f32}.[
                $(Self::identity("gds-about-version",app.version)), $(Self::identity("gds-about-build-tag",app.build_tag)),
                .div.{flex;flex_wrap;gap: 16.0_f32}.[$(self.website(|| crate::i18n::text("gds-about-website"), app.website)),$(self.website(|| crate::i18n::text("gds-about-feedback"), app.feedback))],
                .div.{flex;flex_wrap;gap: 8.0_f32}.[
                    $(self.section_button("gds-about-credits", Icon::Info, Section::Credits)),
                    $(self.section_button("gds-about-license", Icon::Scale, Section::License)),
                ],
            ]},
            Section::Credits => el! {.div.{flex;flex_col;gap: 12.0_f32}.[
                "gds-about-authors".crate::i18n::text.text, $(app.authors.join(", ")).text,
                "gds-about-projects".crate::i18n::text.text,
                ..$(app.credits.iter().map(|credit| {
                    let name=credit.name; let url=credit.website;
                    el!{.div.{flex;gap: 8.0_f32}.[$(self.website(move || name.to_owned(), url))]}
                })),
            ]},
            Section::License => el! { .div.{flex;flex_col;gap: 16.0_f32}.[
                app->license.text.{font_weight: FontWeight::SEMIBOLD}, app->license_text.text.{select_text},
                "Lucide".text.{font_weight: FontWeight::SEMIBOLD},
                include_str!("icon/LICENSE").text.{select_text},
            ] },
        }
    }
    fn section_button(&self, label: &'static str, icon: Icon, destination: Section) -> Div {
        let section = self.section;
        let focus = self.focus;
        el! { .div.{
            flex; items_center; gap: 6.0_f32; px: 8.0_f32; h: 24.0_f32;
            rounded: 6.0_f32; border: 1.0_f32; focusable; cursor: CursorIcon::Pointer;
            dyn_style: s => $(s.border_color(theme().border_strong).bg(theme().surface));
            hover: s => $(s.bg(theme().hover));
            on_click: move _ => ${section.set(destination); focus.focus(); };
            on_key_down: move event => ${
                if matches!(event.key, Key::Named(NamedKey::Space | NamedKey::Enter)) {
                    event.prevent_default();
                    event.stop_propagation();
                    section.set(destination);
                    focus.focus();
                }
            };
        }.[ $(icon.view(14.0_f32)), (move () => label.crate::i18n::text).text ] }
    }
}
impl IntoView for About {
    fn into_view(self) -> View {
        let section = self.section;
        let error = self.error;
        let app = self.application;
        let focus = self.focus;
        request_frame(move |_| focus.focus());
        el!{.ContentPage::default.{
            node_ref: focus; focusable;
            on_key_down: move event => ${
                let back = event.key == Key::Named(NamedKey::Escape)
                    || (event.modifiers.alt && event.key == Key::Named(NamedKey::ArrowLeft));
                if back && section.get_untracked() != Section::Overview {
                    event.prevent_default();
                    event.stop_propagation();
                    section.set(Section::Overview);
                }
            };
        }.[
            .PageHeader::default.[
                $(app.artwork().size(crate::appearance::tokens::SIDEBAR_ICON_SIZE).flex_none()),
                .PageTitle::default.[app->name.text],
            ],
            .PageContent::default.{px: 24.0_f32; pb: 16.0_f32; overflow_y_scroll}.[
                (move () => ${
                    let destination = section.get();
                    el! { .div.{flex; flex_col; min_h: 0.0_f32; gap: 16.0_f32}.[
                        if $(destination != Section::Overview) [
                            .div.{flex; items_center; gap: 8.0_f32}.[
                                "about-back".IconButton::new.{icon:Icon::Back;label_fn:()=>"gds-about-back".crate::i18n::text;on_click:move _=>${section.set(Section::Overview); focus.focus();}},
                                $(crate::i18n::text(if destination == Section::Credits {"gds-about-credits"} else {"gds-about-license"})).text.{font_weight:FontWeight::SEMIBOLD},
                            ],
                        ],
                        $(self.content()),
                    ] }
                }).dyn_view,
                (move()=>error.{get}.{unwrap_or_default}).text,
            ],
        ]}.into_view()
    }
}
