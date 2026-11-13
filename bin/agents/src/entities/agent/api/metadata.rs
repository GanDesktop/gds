//! Agent artwork is shared by every window, independently of session connections.

use bas::service::tls::ConnectionOptions as Target;
use std::collections::BTreeMap;

use bas::{
    config::agent::{AgentSpec, Command},
    management::{Catalog, Source},
};
use gds::service::task_manager::Scope;
use gpui_kit::{App, AppContext, Context, Entity, Global, Subscription, Task};

#[derive(Clone)]
struct Shared(Entity<Metadata>);
impl Global for Shared {}

#[derive(Default)]
struct Server {
    icons: BTreeMap<String, String>,
    commands: BTreeMap<String, Command>,
    loaded: bool,
    requested: bool,
    queued: bool,
    changes_revision: Option<u64>,
    task: Option<Task<()>>,
}

pub(crate) struct Metadata {
    tasks: Option<Scope>,
    servers: BTreeMap<Target, Server>,
    _changes: Subscription,
}

impl Metadata {
    fn new(tasks: Option<Scope>, cx: &mut Context<Self>) -> Self {
        let changes = cx.observe_global::<crate::entities::agent::Changes>(|this, cx| {
            let changes = cx.global::<crate::entities::agent::Changes>();
            let Some(server) = changes.server.clone() else {
                return;
            };
            let revision = changes.revision;
            let state = this.servers.entry(server.clone()).or_default();
            if state.changes_revision == Some(revision) {
                return;
            }
            state.changes_revision = Some(revision);
            this.refresh(server, cx);
        });
        Self {
            tasks,
            servers: BTreeMap::new(),
            _changes: changes,
        }
    }

    /// Look up the agent that owns a thread; unknown agents keep the caller's fallback icon.
    pub(crate) fn icon(&self, server: &Target, agent: &str) -> Option<String> {
        self.servers.get(server)?.icons.get(agent).cloned()
    }

    fn ensure(&mut self, server: Target, cx: &mut Context<Self>) {
        let state = self.servers.entry(server.clone()).or_default();
        if state.loaded || state.requested {
            return;
        }
        self.refresh(server, cx);
    }

    fn refresh(&mut self, server: Target, cx: &mut Context<Self>) {
        let Some(tasks) = self.tasks.as_ref() else {
            return;
        };
        let state = self.servers.entry(server.clone()).or_default();
        if state.requested {
            state.queued = true;
            return;
        }
        let request = crate::entities::agent::catalog::Service::new(server.clone(), tasks.clone())
            .catalog(false);
        self.begin_request(server, request, cx);
    }

    fn begin_request(
        &mut self,
        server: Target,
        request: impl Future<Output = Result<Catalog, String>> + 'static,
        cx: &mut Context<Self>,
    ) {
        self.servers.entry(server.clone()).or_default().requested = true;
        let request_server = server.clone();
        let task = cx.spawn(async move |metadata, cx| {
            let server = request_server;
            let result = request.await;
            let _ = metadata.update(cx, |metadata, cx| {
                let state = metadata.servers.entry(server.clone()).or_default();
                state.requested = false;
                state.task = None;
                let queued = std::mem::take(&mut state.queued);
                if let Ok(catalog) = result {
                    metadata.apply_catalog(server.clone(), &catalog, cx);
                }
                if queued {
                    metadata.refresh(server, cx);
                }
            });
        });
        self.servers.entry(server).or_default().task = Some(task);
    }

    fn apply_catalog(&mut self, server: Target, catalog: &Catalog, cx: &mut Context<Self>) {
        let state = self.servers.entry(server).or_default();
        state.loaded = true;
        // Removed configurations can still own persisted threads; retain their last artwork.
        if merge_icons(&mut state.icons, &mut state.commands, catalog) {
            cx.notify();
        }
    }
}

/// Start one cached catalog request; subsequent windows share this entity.
pub(crate) fn init(server: Target, tasks: Scope, cx: &mut App) {
    if cx.try_global::<crate::entities::agent::Changes>().is_none() {
        cx.set_global(crate::entities::agent::Changes::default());
    }
    if cx.try_global::<Shared>().is_none() {
        let metadata = cx.new(|cx| Metadata::new(Some(tasks), cx));
        cx.set_global(Shared(metadata));
    }
    ensure(server, cx);
}

pub(crate) fn entity(cx: &App) -> Option<Entity<Metadata>> {
    cx.try_global::<Shared>().map(|shared| shared.0.clone())
}

/// Ensure metadata once when opening/reconnecting a workspace, never from a render timer.
pub(crate) fn ensure(server: Target, cx: &mut App) {
    if let Some(metadata) = entity(cx) {
        metadata.update(cx, |metadata, cx| metadata.ensure(server, cx));
    }
}

/// Reuse a settings catalog already fetched by another GUI action.
pub(crate) fn publish(server: Target, catalog: &Catalog, cx: &mut App) {
    if let Some(metadata) = entity(cx) {
        metadata.update(cx, |metadata, cx| {
            metadata.apply_catalog(server, catalog, cx)
        });
    }
}

fn merge_icons(
    icons: &mut BTreeMap<String, String>,
    commands: &mut BTreeMap<String, Command>,
    catalog: &Catalog,
) -> bool {
    let official: BTreeMap<_, _> = catalog
        .entries
        .iter()
        .filter(|entry| entry.source == Source::Registry && !entry.icon.is_empty())
        .map(|entry| (entry.id.as_str(), entry.icon.as_str()))
        .collect();
    let mut changed = false;
    for entry in &catalog.entries {
        let command = entry.spec.as_ref().and_then(AgentSpec::command);
        let icon = if !entry.icon.is_empty() {
            Some(entry.icon.as_str())
        } else {
            entry
                .spec
                .as_ref()
                .and_then(|spec| command_icon(spec, &official))
        };
        if let Some(icon) = icon {
            if icons.get(&entry.id).is_none_or(|previous| previous != icon) {
                icons.insert(entry.id.clone(), icon.into());
                changed = true;
            }
        } else if !official.is_empty()
            && command.is_some_and(|current| {
                commands
                    .get(&entry.id)
                    .is_some_and(|previous| previous != current)
            })
        {
            changed |= icons.remove(&entry.id).is_some();
        }
        if (icon.is_some() || !official.is_empty())
            && let Some(command) = command
        {
            commands.insert(entry.id.clone(), command.clone());
        }
    }
    changed
}

fn command_icon<'a>(spec: &AgentSpec, official: &BTreeMap<&str, &'a str>) -> Option<&'a str> {
    let command = spec.command()?;
    let filename = command.program.file_name()?.to_str()?;
    let executable = filename
        .strip_suffix(".exe")
        .or_else(|| filename.strip_suffix(".cmd"))
        .unwrap_or(filename);
    if let Some(icon) = official.get(executable) {
        return Some(icon);
    }
    // Managed JS launchers retain an exact package directory, even when their executable is Node.
    if executable == "node" {
        let script = std::path::Path::new(command.args.first()?);
        let mut parts = script.components();
        while let Some(part) = parts.next() {
            if part.as_os_str() != "node_modules" {
                continue;
            }
            let package = parts.next()?.as_os_str().to_str()?;
            let package = if package.starts_with('@') {
                parts.next()?.as_os_str().to_str()?
            } else {
                package
            };
            if let Some(icon) = official.get(package) {
                return Some(icon);
            }
        }
    }
    None
}

#[cfg(all(test, feature = "gui-tests"))]
mod tests;
