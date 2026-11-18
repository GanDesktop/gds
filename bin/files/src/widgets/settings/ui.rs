//! Reactive settings pages editing the persisted application schema.
mod appearance;
mod pages;
mod tags;
use crate::entities::preferences::{Preferences, PreferencesEditor};
use anti_webui::prelude::*;
use gds::ui::{
    Icon, IconButton, NavItem,
    settings::{
        PERCENTAGE_RANGE, SettingsActions, SettingsControl, SettingsDescription, SettingsLabel,
        SettingsReset, SettingsRow, compact_switch,
        form::{Editor, Model},
        setting_slider,
    },
    sidebar::SidebarPresentation,
};
use gds_uim::el;
pub use pages::Page;
use std::rc::Rc;

#[derive(Clone)]
pub struct View {
    preferences: Signal<Preferences>,
    on_apply: Rc<dyn Fn(Preferences)>,
    page: Signal<Page>,
    themes: appearance::Themes,
    tags: tags::Probe,
    page_focus: NodeRef,
    performance_unlocked: Signal<bool>,
}
impl View {
    pub fn new(preferences: Signal<Preferences>, on_apply: impl Fn(Preferences) + 'static) -> Self {
        Self {
            preferences,
            on_apply: Rc::new(on_apply),
            page: signal(Page::General),
            themes: appearance::Themes::new(),
            tags: tags::Probe::new(preferences),
            page_focus: node_ref(),
            performance_unlocked: signal(false),
        }
    }
    fn apply(&self, change: impl FnOnce(&mut Preferences)) {
        let previous = self.preferences.get_untracked();
        let mut next = previous.clone();
        change(&mut next);
        self.themes.normalize_source(&mut next.appearance.source);
        let language_changed = next.language != previous.language;
        (self.on_apply)(next);
        if language_changed {
            self.page_focus.focus();
        }
    }
    fn subform<T: Model>(
        &self,
        get: fn(&Preferences) -> T,
        set: fn(&mut Preferences, T),
    ) -> Editor<T> {
        let local = signal(get(&self.preferences.get_untracked()));
        let preferences = self.preferences;
        effect(move || {
            local.set_if_changed(get(&preferences.get()));
        });
        let view = self.clone();
        Editor::new(local)
            .reset_focus(self.page_focus)
            .on_change(move |value| view.apply(|preferences| set(preferences, value)))
    }
    fn reset<T: PartialEq + 'static>(
        &self,
        label: &'static str,
        get: fn(&Preferences) -> T,
        set: fn(&mut Preferences, T),
    ) -> SettingsReset {
        let preferences = self.preferences;
        let view = self.clone();
        el! { .SettingsReset::default.[
            (move()=>$(preferences.with(get) != get(&Preferences::default())), move()=>${
                let view = view.clone();
                el! { label.IconButton::new.{icon:Icon::Reset;label_fn:()=>"form-reset".gds::i18n::text;
                    on_click:move _=>${
                        view.apply(|preferences| set(preferences,get(&Preferences::default())));
                        view.page_focus.focus();
                    };
                } }
            }).show,
        ] }
    }
    fn toggle(
        &self,
        label: &'static str,
        help: &'static str,
        get: fn(&Preferences) -> bool,
        set: fn(&mut Preferences, bool),
    ) -> impl IntoView {
        let preferences = self.preferences;
        let view = self.clone();
        let value = Binding::new(
            move || preferences.with(get),
            move |value| view.apply(|preferences| set(preferences, value)),
        );
        el! { .SettingsRow::default.[
            .SettingsLabel::default.[ (move () => label.gds::i18n::text).text,
                if $(!help.is_empty()) [.SettingsDescription::default.[(move () => help.gds::i18n::text).text]],
            ],
            .SettingsActions::default.[$(self.reset(label,get,set)), .SettingsControl::default.[$(compact_switch(value, || false))]],
        ] }
    }
    fn opacity(
        &self,
        label: &'static str,
        get: fn(&Preferences) -> f32,
        set: fn(&mut Preferences, f32),
    ) -> impl IntoView {
        let preferences = self.preferences;
        let view = self.clone();
        let value = Binding::new(
            move || preferences.with(get) * 100.0,
            move |next| view.apply(|p| set(p, next / 100.0)),
        );
        el! { .SettingsRow::default.[
            .SettingsLabel::default.[(move()=>label.gds::i18n::text).text],
            .SettingsActions::default.[$(self.reset(label,get,set)), .SettingsControl::default.[
                (value,PERCENTAGE_RANGE,false).setting_slider.{flex_1;min_w:0.0_f32},
                (move()=>$(format!("{:.0}%",value.get()))).text.{w:76.0_f32;text_align:TextAlign::Right},
            ]],
        ] }
    }
    fn contents(&self) -> ViewHandle {
        match self.page.get() {
            Page::General => {
                let view = self.clone();
                el! { .div.[
                    $(PreferencesEditor::new(self.preferences).reset_focus(self.page_focus)
                        .on_change(move |next| view.apply(|preferences| *preferences = next))),
                ] }.into_view()
            }
            Page::Appearance => self.appearance().into_view(),
            Page::Files => el! { .div.[
                $(self.toggle("sidebar-show-volumes", "", |p|p.files.sidebar.show_volumes, |p,v|p.files.sidebar.show_volumes=v)),
                $(self.toggle("auto-focus-search", "", |p|p.files.auto_focus_search, |p,v|p.files.auto_focus_search=v)),
            ] }.into_view(),
            Page::Terminal => {
                let preferences = self.preferences;
                let view = self.clone();
                el! { .div.{flex; flex_col; gap: 16.0_f32}.[
                    $(self.toggle("files-terminal-enabled", "", Preferences::terminal_enabled, |p,v|p.terminal_enabled=Some(v))),
                    (move () => preferences.{with:Preferences::terminal_enabled}, move () => $(view.subform(|p|p.terminal.clone(), |p,v|p.terminal=v))).show,
                ] }.into_view()
            }
            Page::Extensions => self.extensions().into_view(),
            Page::Performance => self.performance().into_view(),
        }
    }
}
type ViewHandle = anti_webui::prelude::View;
impl View {
    fn adjacent(page: Page, backwards: bool) -> Page {
        let offset = if backwards { Page::ALL.len() - 1 } else { 1 };
        Page::ALL[(page.index() + offset) % Page::ALL.len()]
    }
    pub fn cycle_page(&self, backwards: bool) {
        self.page
            .set(Self::adjacent(self.page.get_untracked(), backwards));
        self.page_focus.scroll_to(0.0, 0.0);
        self.page_focus.focus();
    }
    /// Uses the host frame's same sidebar width; this never adds another surface.
    pub fn sidebar(&self, presentation: SidebarPresentation) -> Div {
        let page = self.page;
        let focus = self.page_focus;
        el! { .div.{flex;flex_col;size_full;min_h:0.0_f32;gap:4.0_f32;p:gds::appearance::tokens::SIDEBAR_PADDING;overflow_y_scroll}.[
            ..Page::ALL.{into_iter;map:destination=>${
                el! { destination.{label_id}.NavItem::new.{
                    label_fn:move()=>destination.{label_id}.gds::i18n::text;
                    icon:destination.{icon};
                    active_when:move()=>$(page.get() == destination);
                    presentation: presentation;
                    keytip_when: "ctrl-tab", move () => $(Self::adjacent(page.get(), false) == destination);
                    keytip_when: "ctrl-shift-tab", move () => $(Self::adjacent(page.get(), true) == destination);
                    on_click:move _=>${ page.set(destination); focus.scroll_to(0.0, 0.0); focus.focus(); };
                } }
            }},
        ] }
    }
    pub fn content(&self) -> Div {
        self.themes.refresh();
        let page = self.page;
        let focus = self.page_focus;
        request_frame(move |_| focus.focus());
        let contents = self.clone();
        let navigation = self.clone();
        el! { .div.{flex;flex_col;size_full;min_h:0.0_f32;min_w:0.0_f32; overflow_y_scroll; p:24.0_f32; node_ref:focus; focusable;
            on_key_down: move event => ${
                let modifiers = event.modifiers;
                if modifiers.primary() && !modifiers.alt && event.key == Key::Named(NamedKey::Tab) {
                    navigation.cycle_page(modifiers.shift);
                    event.prevent_default();
                    event.stop_propagation();
                }
            };
        }.[
            (move()=>page.{get}.{label_id}.gds::i18n::text).text.{mb:20.0_f32;dyn_style:s=>$(s.text_size(gds::app::Gds::current().appearance().typography().scaled(20.0)))},
            (move()=>${ let _ = page.get(); untrack(|| contents.contents()) }).dyn_view,
        ] }
    }
    pub fn footer_text(&self) -> impl IntoView + use<> {
        el! { (()=>"settings-auto-apply".gds::i18n::text).text.{dyn_style:s=>$(s.text_size(gds::app::Gds::current().appearance().typography().scaled(12.0)))} }
    }
}
impl IntoView for View {
    fn into_view(self) -> ViewHandle {
        self.content().into_view()
    }
}
