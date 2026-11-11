# Task ownership and application shutdown

The application owns its services and decides their startup order from configuration.
`gds::service::task_manager::Supervisor` owns the Tokio runtime and the root task
scope. Services receive scopes, never runtime handles. TM controls admission,
execution, cancellation and completion; each service implements its resource cleanup.
Desktop integration (D-Bus and FileManager1) is one application service.

The GPUI App context binds this service through `cx.gds().tasks()` using
`gds::app::AppExt`. GPUI continues to own windows and UI tasks. The independent
`gds::ui::task_manager` view receives only a read-only `Inspector`; the core service
has no GPUI, logging or rendering dependency.

```text
Application assembly: configuration → TM → required services → listen
├── TM Supervisor → private Tokio runtime
│   └── root scope → service/window scopes → registered work
├── desktop integration, storage, connections, terminal services
└── GPUI windows and optional diagnostic views
```

## Ownership before execution

Desktop applications install one root in their App context and obtain it through
`cx.gds().tasks().scope()`. Services receive a scope derived from that
root; they do not create independent roots. `Supervisor::new(inspect)` creates the application executor; keep the supervisor
alive until the UI loop and service drain have completed. `Scope::with_runtime`
and `Scope::with_handle` support integration with an existing executor at a
process bootstrap or test boundary. `Scope::default()` is a tracking-only root;
it cannot schedule Tokio work. `scope.child()` returns an `Owner` for a
subtree. Store that owner in the view or service responsible for the work.
Dropping the owner requests cancellation. Workers receive cloned tokens or
scopes, **not the owner**: retaining the owner in its worker would prevent
owner destruction from requesting shutdown.

Scopes can be nested by window, service or operation. They do not create new
thread pools. A service using both Tokio and a dedicated thread still has one
scope. Closing a subtree leaves its siblings running.

Register work **before** scheduling it. `track`, `spawn`, `spawn_cancellable`, `spawn_blocking`, and
`spawn_thread` register completion with the scope and every ancestor. Registration
and scope shutdown serialize admission: shutdown rejects new work, including work
started through an existing descendant handle. A dropped or unpolled tracked
future releases its registration; registration is not a promise to execute it.

## Cancellation and completion are separate

The token requests cancellation. The completion registration represents actual
resource cleanup. `scope.shutdown().await` seals admission, cancels the subtree,
and waits for all its registered work. Do not call it inside a task registered
in that same subtree: that would wait for itself.

Choose a policy for every operation:

| Work | Policy |
| --- | --- |
| UI-only updates, debounce timers, disposable reads | Keep a GPUI task on its owner; dropping the owner can cancel it. No persistence or child cleanup may depend on it. |
| Long-lived services and cancellable operations | Observe a child token, stop accepting work, finish cleanup, then return. |
| Accepted writes and file mutations | Register with the application before starting; finish the accepted operation even if its original UI disappears. Cooperatively cancellable batches may stop between items. |
| Dedicated threads | Observe a token or a wakeable channel; release devices/handles before the tracked function returns. |
| Child processes | Terminate according to service policy and reap before releasing task completion. |
| External applications intentionally launched for the user | Hand off ownership; do not kill them when GDS closes. |

Cancelling a token does not abort a future or interrupt a blocking system call.
Use an interruptible API, close a channel, or poll with a bounded interval where
necessary. An accepted blocking operation can instead finish normally.

Do not wrap a resource supervisor in a generic `select!` that drops it on
cancellation. That can discard the very code that must terminate a child, flush
a writer, or close a remote operation. Put the cancellation branch **inside**
the supervisor and run its cleanup after the branch.

## Executor adapters

```rust,ignore
let owner = application.child();
let cancel = owner.token();
let task = owner.spawn(async move {
    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            command = commands.recv() => {
                let Some(command) = command else { break };
                handle(command).await;
            }
        }
    }
    writer.flush().await;
});
// Store owner in the service. The root tracks completion even if task is detached.
```

For GPUI, wrap a resource-owning future with `scope.track(work)` before handing
it to the foreground or background executor. Use foreground execution only for
UI access. For an existing supervisor, `scope.register()` provides a completion
guard that must remain alive through the final cleanup.

`spawn` returns a `Task<T>` whose await result distinguishes closed admission,
missing executor and execution failure. Dropping it detaches already registered
work. `spawn_cancellable` is for disposable I/O: root/subtree cancellation drops
the future and returns a closed-scope error, even while its handle is retained.
Use it only when dropping the future safely releases its resources.
`cancel_on_drop` additionally aborts disposable work when its caller disappears.
`Group` provides a service-owned set of child tasks through the same admission gate.

Tokio `spawn_blocking` suits finite blocking operations. A started blocking task
cannot be stopped by aborting its join handle. Dedicated threads remain suitable
for device affinity or persistent blocking loops; `spawn_thread` tracks their
function lifetime. Keep/join its returned handle when OS-thread termination or
thread-local destructors themselves are part of a service's contract.

`task_manager::process::output` captures an asynchronous child, handles cancellation
and timeout, and waits for reaping; on Unix it terminates the child's process
group. `ChildGuard` provides kill/wait ownership for synchronous workers such
as FFmpeg. It supervises the direct child. The terminal service retains its
specialized session/process-group handling for foreground, background and stopped
jobs. Register the entire supervisor rather than independently registering every
implementation thread inside it.

## Desktop application integration

Import `gds::app::AppExt` and initialize through the context accessor at the
start of the GPUI application callback:

```rust,ignore
use gds::app::AppExt as _;

// Before the GPUI callback: let supervisor = Supervisor::new(enable_logs)?;
cx.gds().initialize(supervisor.scope(), logs);
let scope = cx.gds().tasks().scope();
// Construct services, install request handlers, then mark startup complete.
cx.gds().tasks().ready(keep_alive);
```

`initialize` installs task ownership, appearance foundations and the optional
log store, in that order. Startup passes `enable_logs` to both the supervisor and
the log collector; diagnostic collection is fixed when the root is constructed.
Applications extend `Gds` with local accessor traits: Files uses
`cx.gds().files().initialize(...)`. `FilesExt` only returns the concrete borrowed
`Files` accessor; initialization is an inherent method on that accessor. Obtaining
it starts no services and transfers no ownership. Configuration remains input data.
Services and lifecycle operations use `cx.gds().tasks()`, including
`quit_when_idle`, `request_quit` and `signals`. The task-only entry point is
`cx.gds().tasks().init(scope)` and needs only the `app` feature.
Obtaining either accessor does not install any globals. `scope()` requires an
installed service and never creates one implicitly. The diagnostics choice is fixed for that application's lifetime.
Installing a log store or opening a diagnostic window cannot change it. Build native
applications with `with_quit_mode(gpui_kit::QuitMode::Explicit)` so closing the last
window does not stop the event loop before TM finishes draining. Route the
last-window callback to `cx.gds().tasks().quit_when_idle()`.

The binding has three phases: Starting, Running and Closing. `ready(keep_alive)`
transitions Starting to Running only after required services are ready.
`quit_when_idle` acts only in Running, with no windows and `keep_alive == false`.
Explicit quit works in every phase; a late startup continuation cannot reopen
admission. Files resolves Browser, Picker or Service mode from configuration,
claims desktop integration, loads storage, installs the UI and initial window,
marks readiness and then starts desktop request handling. Service mode stays alive
without windows. Startup failures route through the same shutdown path and return failure.

Files keeps the orchestration in `bin/files/src/app/startup.rs`: `launch` resolves
configuration, logging and the supervisor; the GPUI callback invokes
the async `Application::start`, which returns `Result`, binds TM and shutdown handling, awaits `Services::start`
for desktop/storage I/O, then calls `Services::start_ui` on the main thread.
`startup/ui.rs` defines the Files accessor and installs presentation, the preferences
service, volume discovery and theme hot reload before any window is created.
`cx.gds().preferences()` reads or edits the application's current preferences;
its ordered background writer drains accepted snapshots on shutdown. Failed saves
leave the published in-memory state active and report the persistence error.
Location storage owns a single flush hook for its live window queues.
`DesktopIntegration::listen` only receives commands and routes them to ready windows.
GPUI supplies its App context inside `run`, so its event loop remains available
during asynchronous startup; business request handling begins after readiness.
A one-shot channel at the GPUI entry boundary returns the startup result to the
process entry point after the event loop exits; initialization methods do not
receive shared error containers.

Explicit quit should:

1. Flush pending UI edits and perform application-specific close preparation.
2. Remove native windows. If entity-owned quit callbacks are needed, retain their
   views with `retain_until_quit`.
3. Call `request_quit`. It cancels the root and drains tracked work while the
   event loop and executors remain alive, then calls `cx.quit()`.

`request_quit` supplies the generic close path. Applications with preparation
requirements, such as Agents, supply their own request function. `signals`
routes Unix SIGINT/SIGTERM through that same function. SIGKILL cannot be handled.
Startup continuations must check `is_shutting_down` before opening new windows.

GPUI's native quit callback is a bounded fallback (currently 200 ms total), not
the normal service drain. Do not postpone essential writes until that fallback.
Keep runtimes alive until the application drain completes. The coordinator itself
must remain outside the scope it drains.

## Existing integrations

Terminal supervisors, Agents connections/history workers and audio threads,
preview generation/FFmpeg, Files storage shutdown and accepted file operations,
Agents accepted navigation/ACP writes, and Nixes evaluation use the shared root.
Service-specific cancellation and cleanup stay in those services. Terminal I/O
uses a tracked native worker and the shared executor, without creating a second
runtime. Startup registers Files persistence cleanup before admission closes;
cleanup runs inline instead of attempting to spawn after shutdown. Pure UI tasks
and existing owner-held subscriptions continue to use GPUI ownership.

This is not a blanket guarantee for every `spawn` in the repository. When adding
or touching a resource-owning task, identify its owner, cancellation policy and
completion path explicitly. `detach()` is acceptable only when completion is
tracked elsewhere or when the work is deliberately disposable/handoff work.

## Inspecting active work

The Debug window's **Tasks** tab shows the application's registered scope tree,
execution kinds, cancellation state, lifetime and registration sites. Open it
with Ctrl+Shift+L (Cmd+Shift+L on macOS) when started with `--enable-logs`.
The view is created only when a diagnostic window is opened and an inspector is
available. Snapshots refresh while the Tasks tab is selected. They contain metadata only;
opening or closing the inspector does not own or cancel the observed work.

Diagnostic collection is off by default, including in development builds.
`--enable-logs` enables it before services start. Without this option, scopes
still cancel and drain work, but do not register diagnostic entries, format
labels or record start times and source locations. There is no inspector handle,
task view, polling timer, or diagnostic reporting channel in this mode. The
`app` feature includes context binding without loading the `ui` module; the
`task-manager` feature can still be used without GPUI. Standalone users of the
task-manager module opt in with `Supervisor::new(true)` when creating the root;
`Supervisor::new(false)` keeps inspection disabled. Tracking-only roots use
`Scope::new(inspect)`.

`scope.inspector()` returns an optional read-only handle. Its weak reference does
not retain the service or its scopes; it returns empty snapshots after they are
released. It cannot derive work or request cancellation.

Use `named_child` for resource or service names. For externally scheduled work,
`track_named` and `register_named` attach a task label and execution kind. Tokio,
blocking and thread adapters supply their execution kind automatically; unnamed
tasks display their registration file. A supervisor row can cover several
internal tasks or child processes. Disposable UI tasks and unregistered work
are outside this view.

Active means registered and incomplete, including I/O waits. Cancelling means
the scope's token has been cancelled; it does not prove the worker has observed
the request. The row remains until completion is released after cleanup, then
disappears. Completed task history is not retained.

## Verification

Test lifecycle behavior independently of a renderer: parent/child cancellation,
sibling isolation, rejected late admission, cleanup after cancellation, and
completion after actual process reaping. Use controllable channels rather than
timing assertions to keep cleanup pending. UI tests should verify that windows
disappear while cleanup is pending and that another window keeps the application
alive. Preserve service tests for data flush and process-group ownership.

## Task names

Application and service call sites supply responsibility-based diagnostic names with
`spawn_named`, `spawn_cancellable_named`, `spawn_blocking_named`,
`spawn_thread_named`, `block_on_named` or `Group::spawn_named`. Names describe the
work, such as `Watch storage volumes`, `Save preferences` or `Read directory`;
source files and line numbers remain separate diagnostic metadata. The unnamed
adapters retain the filename fallback for compatibility. Naming does not enable
inspection: without `--enable-logs`, names are not copied into task records and no
inspection records are created.
