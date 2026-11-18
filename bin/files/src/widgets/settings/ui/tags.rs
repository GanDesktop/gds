use super::{Preferences, View};
use crate::entities::tags;
use anti_webui::prelude::*;
use gds::{
    app::Gds,
    i18n,
    ui::settings::{SettingsActions, SettingsControl, SettingsLabel, SettingsRow, compact_switch},
};
use gds_uim::el;

#[derive(Clone)]
enum State {
    Checking,
    Ready,
    Failed(String),
}

#[derive(Clone, Copy)]
pub(super) struct Probe {
    state: Signal<State>,
    refresh: Signal<()>,
}

impl Probe {
    pub(super) fn new(preferences: Signal<Preferences>) -> Self {
        let state = signal(State::Checking);
        let refresh = signal(());
        let commands = memo(move || preferences.with(|p| p.tags.commands.clone()));
        effect(move || {
            refresh.get();
            let commands = commands.get();
            state.set(State::Checking);
            Gds::current()
                .tasks()
                .spawn("Check tag backend", async move {
                    tags::connect(&commands).await
                })
                .then(move |result| {
                    state.set(match result {
                        Ok(Ok(_)) => State::Ready,
                        Ok(Err(error)) => State::Failed(error),
                        Err(error) => State::Failed(error.to_string()),
                    });
                });
        });
        Self { state, refresh }
    }

    fn ready(self) -> bool {
        self.state.with(|state| matches!(state, State::Ready))
    }

    fn checking(self) -> bool {
        self.state.with(|state| matches!(state, State::Checking))
    }

    fn status(self) -> String {
        self.state.with(|state| match state {
            State::Checking => i18n::text("tags-checking"),
            State::Ready => i18n::text("tags-ready"),
            State::Failed(error) => error.clone(),
        })
    }
}

impl View {
    pub(super) fn extensions(&self) -> Div {
        let probe = self.tags;
        let preferences = self.preferences;
        let view = self.clone();
        let enabled = Binding::new(
            move || preferences.with(|p| p.tags.enabled),
            move |enabled| {
                if !enabled || probe.ready() {
                    view.apply(|p| p.tags.enabled = enabled);
                }
            },
        );
        let reset = self.reset(
            "tags-enabled",
            |p| p.tags.enabled,
            |p, v| p.tags.enabled = v,
        );
        let commands = self.subform(|p| p.tags.commands.clone(), |p, v| p.tags.commands = v);
        el! { .div.{ flex; flex_col }.[
            (() => "tags-title".i18n::text).text.{ text_size: 18.0_f32; font_weight: FontWeight::SEMIBOLD; mb: 16.0_f32 },
            .SettingsRow::default.[
                .SettingsLabel::default.[(() => "tags-enabled".i18n::text).text],
                .SettingsActions::default.[reset, .SettingsControl::default.[
                    $(compact_switch(enabled, move || !enabled.get() && !probe.ready())),
                ]],
            ],
            .div.{ flex; items_center; gap: 12.0_f32; py: 12.0_f32 }.[
                (move () => probe.{status}).text.{ flex_1; min_w: 0.0_f32; dyn_style: s => $(s.text_color(theme().text_muted)) },
                (() => "tags-detect".i18n::text).button.{
                    flex_none; h: 24.0_f32; px: 8.0_f32; py: 0.0_f32;
                    dyn_style: s => $(s.bg(theme().page).border_color(theme().border_strong));
                    disabled: move () => probe.{checking}; on_click: move _ => $(probe.refresh.set(()));
                },
            ],
            commands,
        ] }
    }
}
