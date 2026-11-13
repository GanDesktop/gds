//! Application startup, route assembly and shutdown.
use gds::app::AppExt as _;
pub(crate) mod cli;
mod startup;
pub(crate) use startup::run;

use crate::shared::routes::{OpenSettings, Quit};
use crate::widgets::settings;
use gpui_kit::{App, Global, KeyBinding};
struct ActionsRegistered;
impl Global for ActionsRegistered {}
struct QuitPending;
impl Global for QuitPending {}

pub(crate) fn init(cx: &mut App) {
    if cx.has_global::<ActionsRegistered>() {
        return;
    }
    cx.set_global(ActionsRegistered);
    cx.gds().ui().register_actions();
    cx.gds().ui().init_terminal();
    crate::shared::config::terminal::init(cx);
    crate::pages::workspace::windows::bind_page_keys(cx);
    crate::features::navigation::install(crate::pages::workspace::routes(), cx);
    cx.gds().ui().time().apply(gds::ui::time::Format::Relative);
    cx.gds().ui().keytips().install();
    cx.gds().ui().logs().init(gds::ui::logs::Options {
        app_id: Some("io.github.GanDesktop.GdsAgents".into()),
        app_name: "GDS Agents".into(),
        background: |cx| {
            cx.global::<crate::shared::lib::appearance::Appearance>()
                .background
        },
        root: |view, window, cx| crate::shared::lib::appearance::window_root(view, window, cx),
        ignored_contexts: &[crate::shared::config::TERMINAL_TABS_CONTEXT],
    });
    let (settings, quit) = if cfg!(target_os = "macos") {
        ("cmd-,", "cmd-q")
    } else {
        ("ctrl-,", "ctrl-q")
    };
    cx.bind_keys([
        KeyBinding::new(settings, OpenSettings, None),
        KeyBinding::new(quit, Quit, None),
        KeyBinding::new(
            quit,
            gpui_kit::NoAction,
            Some(gds::ui::terminal::INPUT_CONTEXT),
        ),
        KeyBinding::new(
            quit,
            gpui_kit::NoAction,
            Some(crate::pages::workspace::TERMINAL_TABS_CONTEXT),
        ),
    ]);
    cx.on_action(on_open_settings);
    cx.on_action(on_quit);
}

fn on_open_settings(_: &OpenSettings, cx: &mut App) {
    cx.defer(|cx| {
        if let Err(error) = settings::window::open(cx) {
            tracing::error!(%error, "cannot open settings");
        }
    });
}

fn on_quit(_: &Quit, cx: &mut App) {
    request_quit(cx);
}

pub(crate) fn request_quit(cx: &mut App) {
    if cx.has_global::<QuitPending>() {
        return;
    }
    cx.set_global(QuitPending);
    settings::window::flush_pending(cx);
    let closed = crate::pages::workspace::windows::close_all(cx);
    cx.gds().tasks().retain_until_quit(closed);
    cx.gds().tasks().request_quit();
}

#[cfg(all(test, feature = "gui-tests"))]
mod tests;
