# Task ownership and application shutdown

UI and services own work. Executors run it. The shared `gds::task_manager` module
connects these responsibilities without requiring a single executor.

```text
Application scope
├── UI-owned scopes: terminal surfaces, preview caches, audio players
├── Service scopes: connections and their history writers
└── Accepted operations: file mutations and persistence
          │
          ├── GPUI foreground / background executors
          ├── Tokio asynchronous / blocking tasks
          ├── Dedicated system threads
          └── Supervised child processes
```

## Ownership before execution

`Scope::default()` creates a root. `scope.child()` returns an `Owner` for a
subtree. Store that owner in the view or service responsible for the work.
Dropping the owner requests cancellation. Workers receive cloned tokens or
scopes, **not the owner**: retaining the owner in its worker would prevent
owner destruction from requesting shutdown.

Scopes can be nested by window, service or operation. They do not create new
thread pools. A service using both Tokio and a dedicated thread still has one
scope. Closing a subtree leaves its siblings running.

Register work **before** scheduling it. `track`, `spawn`, `spawn_blocking`, and
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
let task = owner.spawn(&runtime, async move {
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
})?;
// Store owner in the service. The root tracks completion even if task is detached.
```

For GPUI, wrap a resource-owning future with `scope.track(work)` before handing
it to the foreground or background executor. Use foreground execution only for
UI access. For an existing supervisor, `scope.register()` provides a completion
guard that must remain alive through the final cleanup.

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

Use `gds::ui::task_manager::init(cx)` and obtain the root with `scope(cx)`. Route the
last-window callback to `quit_when_idle(cx)`. Explicit quit should:

1. Flush pending UI edits and perform application-specific close preparation.
2. Remove native windows. If entity-owned quit callbacks are needed, retain their
   views with `retain_until_quit`.
3. Call `quit_when_idle`. It cancels the root and drains tracked work while the
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
Service-specific cancellation and cleanup stay in those services. Pure UI tasks
and existing owner-held subscriptions continue to use GPUI ownership.

This is not a blanket guarantee for every `spawn` in the repository. When adding
or touching a resource-owning task, identify its owner, cancellation policy and
completion path explicitly. `detach()` is acceptable only when completion is
tracked elsewhere or when the work is deliberately disposable/handoff work.

## Inspecting active work

The Debug window's **Tasks** tab shows the application's registered scope tree,
execution kinds, cancellation state, lifetime and registration sites. Open it
with Ctrl+Shift+L (Cmd+Shift+L on macOS) when started with `--enable-logs`.
Snapshots refresh while the Tasks tab is selected. They contain metadata only;
opening or closing the inspector does not own or cancel the observed work.

Diagnostic collection is off by default, including in development builds.
`--enable-logs` enables it before services start. Without this option, scopes
still cancel and drain work, but do not register diagnostic entries, format
labels or record start times and source locations. Standalone users of the
task-manager module opt in with `Scope::new(true)` when creating the root;
`Scope::default()` keeps inspection disabled.

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
