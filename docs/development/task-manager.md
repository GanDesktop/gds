# Task Ownership & Application Shutdown

`gds::service::task_manager` coordinates cancellation and completion barriers across Tokio tasks, blocking pool jobs, dedicated OS threads, child processes, and UI-thread futures. The core service has no dependency on `zgui`, `ui`, or logging.

```text
Application Bootstrap: Configuration → Supervisor → Services → Windows → Listen
├── Supervisor (owns private multi-threaded Tokio runtime)
│   └── Root Scope → Service / Window Child Scopes → Tracked Work
├── Application Services (storage, D-Bus, remote sessions, PTY workers)
└── zgui Windows & Optional Read-Only Task Inspector (--enable-logs)
```

## Scope Hierarchy & Admission

- **Single root per process**: Desktop apps construct one `Supervisor::new(enable_logs)` before entering `zgui` and expose its root `Scope` via `Gds::current().tasks().scope()`. Services receive derived `Scope`s—never raw Tokio `Handle`s.
- **Owners vs workers**: `scope.child()` (or `named_child`) returns an `Owner`. Store the `Owner` in the view or service managing that lifetime, and pass cloned `Scope`s or cancellation tokens into spawned workers. Retaining an `Owner` inside its own worker prevents drop-driven cancellation.
- **Serialized admission**: `track`, `spawn`, `spawn_cancellable`, `spawn_blocking`, and `spawn_thread` register completion with the scope and every ancestor *before* scheduling execution. Once `shutdown()` begins on a scope, new registrations on that scope or any descendant are rejected immediately.

## Cancellation vs Completion

Requesting cancellation (via token or dropping an `Owner`) is distinct from completion (releasing resources). `scope.shutdown().await` seals admission, triggers cancellation across the subtree, and awaits all registered work. Never call `shutdown().await` from inside a task registered in that same subtree.

| Workload | Required Policy |
| --- | --- |
| Disposable UI updates & debounce timers | Keep `zgui::reactive::Task` on its reactive `Owner`; disposing the owner cancels it immediately. |
| Long-lived services | Observe `cancel.cancelled()` inside the service loop, stop accepting requests, flush/close resources, then return. |
| Accepted writes & file mutations | Register on the application scope before starting so accepted writes finish cleanly even if the originating view closes. |
| Dedicated OS threads (`spawn_thread`) | Observe a token or wakeable channel and release device/file handles before returning. |
| Child processes (`process::output`, `ChildGuard`) | Terminate process groups according to service policy and reap before releasing the completion guard. |
| User-launched external apps | Hand off ownership to the desktop session; never kill them when GDS exits. |

> [!IMPORTANT]
> Never wrap a resource supervisor in an outer `tokio::select!` that drops the supervisor future upon cancellation—doing so discards the cleanup code that flushes buffers or reaps child processes. Handle `cancel.cancelled()` *inside* the supervisor loop and run cleanup afterward.

```rust,ignore
let owner = app_scope.named_child("Preferences writer");
let cancel = owner.token();
let _task = owner.spawn_named("Drain preference snapshots", async move {
    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            cmd = commands.recv() => {
                let Some(cmd) = cmd else { break };
                handle(cmd).await;
            }
        }
    }
    writer.flush().await;
});
```

### Adapter Semantics

- `spawn` / `spawn_named`: Returns `Task<T>` reporting closed admission, missing executor, or panic. Dropping `Task<T>` detaches the handle while keeping completion tracked by the scope.
- `spawn_cancellable`: Aborts the future immediately when its scope cancels (use only when dropping the future mid-await is resource-safe). `cancel_on_drop` also aborts if the caller drops `Task<T>`.
- `Group`: Dynamic set of child tasks gated by the same scope admission.
- `scope.track(future)`: Wraps a resource-owning future scheduled onto the UI thread (`zgui::reactive::spawn_local`) or another async executor.

## `zgui` Lifecycle Integration (`Gds` & `Tasks`)

Native apps install `Gds` inside the `zgui` reactive root (`Gds::install` / `Gds::install_with_logs`), bridging the Tokio runtime into `zgui-tokio` (`install_handle` / `enter_here`) and shutting down cleanly after windows close:

```rust,ignore
use gds::app::Gds;

let gds = Gds::install_with_logs(supervisor.scope(), logs);
// ... initialize storage, preferences, and initial windows ...
// Window-scoped Tokio work uses gds.tasks().spawn_ui(name, work, on_complete);
```

Closing windows cancels their child reactive `Owner`s and derived `Tasks::owner_scope` handles. When the `zgui` event loop exits, `Gds` teardown cancels the root scope and `Supervisor::shutdown` seals admission and drains all tracked service cleanup before dropping the Tokio runtime.

## Task Inspection (`--enable-logs`)

When started with `--enable-logs`, opening the Debug window (`Ctrl+Shift+L` / `Cmd+Shift+L`) exposes a read-only **Tasks** tab backed by `scope.inspector()` (weak reference; never retains scopes or cancels work).

- Shows live scope hierarchy, responsibility labels (`spawn_named`), execution kind, elapsed duration, source location, and `Active` vs `Cancelling` status.
- Without `--enable-logs`, `Supervisor::new(false)` allocates zero diagnostic records, labels, or timers.
