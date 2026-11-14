//! Shared Linux XDG Settings Portal snapshot and change stream.
use super::theme::{Mode, provider::system};
use crate::service::task_manager::{Scope, Task as IoTask};
use futures_lite::StreamExt as _;
use gpui_kit::{App, Global, Rgba, Task};
use std::{
    collections::HashMap,
    sync::{OnceLock, RwLock},
    time::Duration,
};
use tokio::sync::watch;
use zbus::zvariant::OwnedValue;

#[cfg(all(test, feature = "gui-tests"))]
mod tests;

const NAMESPACE: &str = "org.freedesktop.appearance";
const REDUCED_MOTION_KEY: &str = "reduced-motion";
const COLOR_SCHEME_KEY: &str = "color-scheme";
const ACCENT_COLOR_KEY: &str = "accent-color";
const PRIME_TIMEOUT: Duration = Duration::from_millis(300);

type Settings = HashMap<String, HashMap<String, OwnedValue>>;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Snapshot {
    pub color_scheme: Option<Mode>,
    pub accent_color: Option<Rgba>,
    pub primed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct State {
    reduced_motion: bool,
    appearance: Snapshot,
}

impl Default for State {
    fn default() -> Self {
        Self {
            reduced_motion: false,
            appearance: Snapshot::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Update {
    reduced_motion: bool,
    appearance_changed: bool,
}

static APPEARANCE: OnceLock<RwLock<Snapshot>> = OnceLock::new();

fn appearance_lock() -> &'static RwLock<Snapshot> {
    APPEARANCE.get_or_init(|| RwLock::new(Snapshot::default()))
}

pub(crate) fn snapshot() -> Snapshot {
    *appearance_lock()
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn update_appearance(next: Snapshot) -> bool {
    let mut guard = appearance_lock()
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let changed = if guard.primed {
        guard.color_scheme != next.color_scheme || guard.accent_color != next.accent_color
    } else {
        next.color_scheme.is_some() || next.accent_color.is_some()
    };
    *guard = next;
    changed
}

/// Prime the initial portal appearance snapshot before synchronous GNOME theme resolution.
pub(crate) async fn prime() {
    if snapshot().primed {
        return;
    }
    let result = tokio::time::timeout(PRIME_TIMEOUT, async {
        let connection = zbus::Connection::session().await?;
        let proxy = portal_proxy(&connection).await?;
        read(&proxy).await
    })
    .await;
    match result {
        Ok(Ok(state)) => {
            let _ = update_appearance(state.appearance);
        }
        Ok(Err(error)) => {
            tracing::debug!(%error, "appearance portal unavailable during startup prime");
        }
        Err(_) => {
            tracing::debug!("appearance portal prime timed out; continuing with defaults");
        }
    }
}

struct Watch {
    io: IoTask<()>,
    _updates: Task<()>,
}
impl Global for Watch {}
impl Drop for Watch {
    fn drop(&mut self) {
        self.io.abort();
    }
}

pub(super) fn follow(tasks: &Scope, cx: &mut App) {
    if cx.has_global::<Watch>() {
        return;
    }
    let (sender, mut receiver) = watch::channel(None);
    let cancel = tasks.token();
    let io = tasks.spawn_named("Watch appearance portal", async move {
        let result = tokio::select! {
            _ = cancel.cancelled() => return,
            result = listen(sender) => result,
        };
        if let Err(error) = result {
            tracing::warn!(%error, "appearance portal unavailable; keeping the last system preference");
        }
    });
    let updates = cx.spawn(async move |cx| {
        while receiver.changed().await.is_ok() {
            let update = *receiver.borrow_and_update();
            if let Some(update) = update {
                cx.update(|cx| {
                    super::motion::system_changed(update.reduced_motion, cx);
                    if update.appearance_changed && system::is_gnome() {
                        super::theme::Observer::refresh_if_active(cx);
                    }
                });
            }
        }
    });
    cx.set_global(Watch {
        io,
        _updates: updates,
    });
}

async fn portal_proxy(connection: &zbus::Connection) -> zbus::Result<zbus::Proxy<'_>> {
    zbus::Proxy::new(
        connection,
        "org.freedesktop.portal.Desktop",
        "/org/freedesktop/portal/desktop",
        "org.freedesktop.portal.Settings",
    )
    .await
}

async fn listen(sender: watch::Sender<Option<Update>>) -> zbus::Result<()> {
    let connection = zbus::Connection::session().await?;
    listen_on(connection, sender).await
}

async fn listen_on(
    connection: zbus::Connection,
    sender: watch::Sender<Option<Update>>,
) -> zbus::Result<()> {
    let proxy = portal_proxy(&connection).await?;
    // Subscribe first so a change during the initial read is not lost. ReadAll
    // is available in v1 and avoids the legacy Read method's nested variants.
    let mut changes = proxy.receive_signal("SettingChanged").await?;
    publish(&proxy, &sender).await?;
    while let Some(message) = changes.next().await {
        let (namespace, key, _value): (String, String, OwnedValue) =
            message.body().deserialize()?;
        if namespace == NAMESPACE
            && matches!(
                key.as_str(),
                REDUCED_MOTION_KEY | COLOR_SCHEME_KEY | ACCENT_COLOR_KEY
            )
        {
            // Read the current values rather than replay a possibly queued signal
            // from before the initial snapshot. This is event-driven, not polling.
            publish(&proxy, &sender).await?;
        }
    }
    Ok(())
}

async fn publish(
    proxy: &zbus::Proxy<'_>,
    sender: &watch::Sender<Option<Update>>,
) -> zbus::Result<()> {
    let state = read(proxy).await?;
    let appearance_changed = update_appearance(state.appearance);
    sender.send_replace(Some(Update {
        reduced_motion: state.reduced_motion,
        appearance_changed,
    }));
    Ok(())
}

async fn read(proxy: &zbus::Proxy<'_>) -> zbus::Result<State> {
    let settings: Settings = proxy.call("ReadAll", &(vec![NAMESPACE],)).await?;
    Ok(parse_settings(&settings))
}

fn parse_settings(settings: &Settings) -> State {
    let Some(namespace) = settings.get(NAMESPACE) else {
        return State {
            reduced_motion: false,
            appearance: Snapshot {
                color_scheme: None,
                accent_color: None,
                primed: true,
            },
        };
    };
    let reduced_motion = namespace
        .get(REDUCED_MOTION_KEY)
        .and_then(|value| u32::try_from(value).ok())
        == Some(1);
    let color_scheme = namespace
        .get(COLOR_SCHEME_KEY)
        .and_then(|value| u32::try_from(value).ok())
        .and_then(|value| match value {
            1 => Some(Mode::Dark),
            2 => Some(Mode::Light),
            _ => None,
        });
    let accent_color = namespace.get(ACCENT_COLOR_KEY).and_then(parse_accent_color);
    State {
        reduced_motion,
        appearance: Snapshot {
            color_scheme,
            accent_color,
            primed: true,
        },
    }
}

fn parse_accent_color(value: &OwnedValue) -> Option<Rgba> {
    let cloned = value.try_clone().ok()?;
    let (r, g, b) = <(f64, f64, f64)>::try_from(cloned).ok()?;
    if [r, g, b]
        .iter()
        .all(|channel| channel.is_finite() && (0.0..=1.0).contains(channel))
    {
        Some(Rgba {
            r: r as f32,
            g: g as f32,
            b: b as f32,
            a: 1.0,
        })
    } else {
        None
    }
}
