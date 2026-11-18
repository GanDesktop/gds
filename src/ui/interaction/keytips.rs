//! Shortcuts revealed by holding Ctrl or Alt in the window.
use anti_webui::prelude::*;
use gds_uim::el;
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    time::Duration,
};

const HOLD: Duration = Duration::from_millis(140);
const SUFFIX_REVEAL: Transition = Transition::ms(140);
const SUFFIX_GAP: f32 = 6.0;
const BADGE_PADDING: f32 = 4.0;
const BADGE_BORDER: f32 = 1.0;
const BADGE_SIZE: f32 = 10.0;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Modifier {
    Ctrl,
    Alt,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Scope {
    Window,
    Dialog(Signal<bool>),
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct Held {
    ctrl: bool,
    alt: bool,
}

impl Held {
    fn group(self) -> Option<Modifier> {
        match (self.ctrl, self.alt) {
            (true, false) => Some(Modifier::Ctrl),
            (false, true) => Some(Modifier::Alt),
            _ => None,
        }
    }
}

#[derive(Clone, Copy)]
pub struct Keytips {
    physical: Signal<Option<(Scope, Held)>>,
    shown: Signal<Option<(Scope, Modifier)>>,
    generation: Signal<u64>,
    enabled: Memo<bool>,
    owner: Owner,
}

thread_local! {
    static WINDOWS: RefCell<HashMap<u64, Keytips>> = RefCell::new(HashMap::new());
    static SCOPE: Cell<Scope> = const { Cell::new(Scope::Window) };
}

impl Keytips {
    pub fn install(enabled: impl Fn() -> bool + 'static) -> Self {
        let window = current_window().id().get();
        let enabled = memo(enabled);
        let hints = Self {
            physical: signal(None),
            shown: signal(None),
            generation: signal(0),
            enabled,
            owner: Owner::current().expect("keytips are mounted under a window owner"),
        };
        WINDOWS.with_borrow_mut(|windows| windows.insert(window, hints));
        on_cleanup(move || {
            WINDOWS.with_borrow_mut(|windows| windows.remove(&window));
        });
        effect(move || {
            let enabled = enabled.get();
            if !window_active() {
                untrack(|| hints.release());
            } else if !enabled {
                untrack(|| hints.release_scope(Scope::Window));
            }
        });
        hints
    }

    fn current() -> Option<Self> {
        let window = current_window().id().get();
        WINDOWS.with_borrow(|windows| windows.get(&window).copied())
    }

    pub fn release(self) {
        self.physical.set_if_changed(None);
        self.shown.set_if_changed(None);
        self.generation
            .update(|generation| *generation = generation.wrapping_add(1));
    }

    fn release_scope(self, target: Scope) {
        if self
            .physical
            .get_untracked()
            .is_some_and(|(scope, _)| scope == target)
        {
            self.release();
        }
    }

    pub fn key_down(self, event: &KeyboardEvent) {
        self.key(Scope::Window, event, true);
    }

    pub fn key_up(self, event: &KeyboardEvent) {
        self.key(Scope::Window, event, false);
    }

    fn accepts(self, scope: Scope) -> bool {
        match scope {
            Scope::Window => self.enabled.get(),
            Scope::Dialog(open) => open.is_alive() && open.get(),
        }
    }

    fn key(self, scope: Scope, event: &KeyboardEvent, pressed: bool) {
        if !untrack(|| self.accepts(scope)) {
            return;
        }
        let previous = self.physical.get_untracked();
        let mut held = Held {
            ctrl: event.modifiers.ctrl,
            alt: event.modifiers.alt,
        };
        match event.key {
            Key::Named(NamedKey::Control) => held.ctrl = pressed,
            Key::Named(NamedKey::Alt) => held.alt = pressed,
            _ => return,
        }
        if previous == Some((scope, held)) {
            return;
        }
        self.physical.set(Some((scope, held)));
        self.shown.set_if_changed(None);
        let generation = self.generation.get_untracked().wrapping_add(1);
        self.generation.set(generation);
        let Some(group) = held.group() else {
            return;
        };
        self.owner.with(|| {
            anti_webui::core::animation::set_timeout(HOLD, move || {
                if self.generation.get_untracked() == generation
                    && self.accepts(scope)
                    && window_active()
                {
                    self.shown.set(Some((scope, group)));
                }
            });
        });
    }
}

#[derive(Clone, Copy)]
pub(crate) struct DialogScope(Signal<bool>);

impl DialogScope {
    pub fn new(open: impl Fn() -> bool + 'static) -> Self {
        let visible = signal(untrack(&open));
        let scope = Self(visible);
        effect(move || {
            let open = open();
            visible.set_if_changed(open);
            if let Some(tips) = Keytips::current() {
                tips.release_scope(if open {
                    Scope::Window
                } else {
                    Scope::Dialog(visible)
                });
            }
        });
        scope
    }

    pub fn view(self, build: impl FnOnce() -> View) -> View {
        let previous = SCOPE.replace(Scope::Dialog(self.0));
        let view = build();
        SCOPE.set(previous);
        view
    }

    pub fn key(self, event: &KeyboardEvent, pressed: bool) {
        if let Some(tips) = Keytips::current() {
            tips.key(Scope::Dialog(self.0), event, pressed);
        }
    }
}

#[derive(Default)]
enum Placement {
    #[default]
    Corner,
    Trailing,
    Inline,
}

/// An inert badge placed next to or inside its control.
pub struct Anchor {
    stroke: &'static str,
    placement: Placement,
    available: Box<dyn Fn() -> bool>,
}

impl Anchor {
    pub fn new(stroke: &'static str) -> Self {
        Self {
            stroke,
            placement: Placement::Corner,
            available: Box::new(|| true),
        }
    }

    pub fn trailing(mut self) -> Self {
        self.placement = Placement::Trailing;
        self
    }
    pub fn inline(mut self) -> Self {
        self.placement = Placement::Inline;
        self
    }
    pub fn when(mut self, available: impl Fn() -> bool + 'static) -> Self {
        self.available = Box::new(available);
        self
    }
}

impl IntoView for Anchor {
    fn into_view(self) -> View {
        let Some(tips) = Keytips::current() else {
            return fragment(Vec::<View>::new()).into_view();
        };
        let scope = SCOPE.get();
        let (modifier, keys) = if let Some(keys) = self.stroke.strip_prefix("ctrl-") {
            (Modifier::Ctrl, keys)
        } else if let Some(keys) = self.stroke.strip_prefix("alt-") {
            (Modifier::Alt, keys)
        } else {
            return fragment(Vec::<View>::new()).into_view();
        };
        if keys.starts_with("alt-") || keys.starts_with("ctrl-") {
            return fragment(Vec::<View>::new()).into_view();
        }
        let label = keys
            .split('-')
            .map(|key| match key {
                "shift" => "Shift".to_owned(),
                "space" => "Space".to_owned(),
                "left" => "←".to_owned(),
                "right" => "→".to_owned(),
                "up" => "↑".to_owned(),
                "down" => "↓".to_owned(),
                "tab" => "Tab".to_owned(),
                key => key.to_ascii_uppercase(),
            })
            .collect::<Vec<_>>()
            .join("+");
        let trailing = matches!(self.placement, Placement::Trailing);
        let inline = matches!(self.placement, Placement::Inline);
        let available = self.available;
        let shown = move || tips.shown.get() == Some((scope, modifier)) && available();
        if inline {
            return Self::suffix(label, shown);
        }
        el! { (shown, move () => ${
            el! { .div.{
                pointer_events_none; absolute; right: 1.0_f32; z_index: 1;
                if trailing { top: 50.0_f32.pct; translate_y: -8.0_f32 } else { top: 1.0_f32 };
            }.[ $(Self::badge(label.clone(), false)) ] }
        }).show }
        .into_view()
    }
}

impl Anchor {
    fn badge(label: String, inline: bool) -> Div {
        el! { .div.{
            pointer_events_none; flex_none;
            px:BADGE_PADDING; py:1.0_f32; rounded:4.0_f32; border:BADGE_BORDER;
            whitespace_nowrap; line_height:$(LineHeight::Relative(1.2));
            dyn_style: move s => ${
                let theme = theme();
                s.bg(theme.overlay)
                    .text_color(if inline { theme.text_muted } else { theme.text })
                    .border_color(if inline { theme.border } else { theme.border_strong })
                    .text_size(crate::app::Gds::current().appearance().typography().scaled(BADGE_SIZE))
            };
        }.[label.text] }
    }

    fn suffix(label: String, shown: impl Fn() -> bool + 'static) -> View {
        let reveal = follow(
            move || if shown() { 1.0_f32 } else { 0.0_f32 },
            SUFFIX_REVEAL,
        );
        let dimensions = memo({
            let label = label.clone();
            move || {
                let context = crate::app::Gds::current();
                let typography = context.appearance().typography();
                let style = Style::default()
                    .font_family(typography.family().as_str())
                    .text_size(typography.scaled(BADGE_SIZE));
                let (width, _) = svg::measure_text(&label, &style);
                (
                    width.ceil() + 2.0 * (BADGE_PADDING + BADGE_BORDER),
                    typography.scaled(SUFFIX_GAP),
                )
            }
        });
        el! { .div.{
            flex; items_center; flex_none; overflow_hidden; pointer_events_none;
            dyn_style: move s => ${
                let progress = reveal.get();
                let (width, gap) = dimensions.get();
                if progress == 0.0 { s.hidden() } else { s.w((width + gap) * progress) }
            };
        }.[
            $(Self::badge(label, true)).{dyn_style: move s => ${
                let (width, gap) = dimensions.get();
                s.w(width).ml(gap).opacity(reveal.get())
            }},
        ] }
        .into_view()
    }
}
