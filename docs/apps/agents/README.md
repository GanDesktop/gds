# GDS Agents

GDS Agents is a native session workspace for an independently running Bas service. It provides conversations, streaming replies, attachments, Markdown, authentication, permission requests, history and Agent management.

## Running

```sh
just dev agents
just dev agents --compact
just dev agents --server unix:///path/to/bas/service.sock
just build agents
just release agents
```

The equivalent Cargo entry is `cargo run --features agents --bin gds-agents -- ...`. The default is the full workspace; `--compact` opens the input popup. `--cwd` sets the working directory for new sessions. `--server` or `BAS_SERVER` overrides the saved connection. Connection configuration supports local IPC, Unix sockets, Windows pipes, TCP and TLS.

Nix provides `gds-agents` as a package and application:

```sh
nix build .#gds-agents
nix run .#gds-agents
```

The `gds.nixosModules.gds-agents` module installs the frontend through `programs.gds-agents.enable`. It does not start or configure Bas. See [NixOS installation](../../nixos.md).

## Workspace and settings

Session management, Agent installation, About and settings appear inside the main window. The top-left arrow returns one level at a time; session management, Agent installation and About also accept Esc, Alt+Left and Ctrl+W (Cmd+W on macOS). Opening these views preserves the workspace, unsent drafts and backend sessions. Restoring a historical session returns to its connection's workspace.

Compact/full mode is a window control in the top-right toolbar. Management views temporarily expand a compact window and restore its mode on return.

Workspaces are persistent Bas entities with stable IDs, names, primary directories, revisions and archive states. Sessions reference their workspace by ID. The sidebar reads the daemon's workspace catalog, including empty workspaces; adding an empty workspace does not launch an Agent. Typing a prompt, selecting an Agent or explicitly creating a session starts session preparation. The workspace selector sits below the application icon and global pinned sessions. Ordinary sessions and the bottom terminal dock belong to the selected workspace; switching restores its last selected session and terminal instances. Drafts, workspace selections and terminal panels live in memory; GDS separately persists the selected session, pins, bookmarks and read cursors.

Archiving a workspace stops and archives all its sessions on Bas. Restoring the workspace leaves those sessions archived until individually restored. Deleting a workspace requires confirmation in GDS: an empty workspace uses the default checked delete, and confirming deletion of a nonempty workspace sends `force=true` to delete all associated sessions and Bas history, including archived sessions. Project files are retained. Both operations are performed by the daemon rather than a sequence of client-owned session requests. Failed cascades retain the workspace and refresh the catalog so partial progress remains visible.

The frontend requires the workspace protocol pinned in `Cargo.lock`. Bas session records must contain workspace IDs; this development change does not synthesize workspaces from old records. Existing services and data files must be handled explicitly when switching builds.

The conversation rail keeps loaded turns at a fixed spacing. Each consecutive range of unloaded turns becomes one compact menu button; opening it shows prompt summaries, and selecting a turn loads that turn's body. Earlier and later unloaded ranges stay on their respective sides of the loaded turns. Ctrl-click toggles a turn bookmark in either the rail or the menu.

Frontend preferences and service configuration have separate ownership. Local preferences apply to this application. Service management identifies the connected Bas service and uses revision-aware RPC updates; disconnected edits remain drafts. Agents does not write the daemon's local configuration file.

## Configuration and state

| Data | Default location |
| --- | --- |
| Frontend settings | `$XDG_CONFIG_HOME/gds/agents/settings.json` |
| Connection history cache | `$XDG_CACHE_HOME/gds/agents/history-v1/history.sqlite` |
| Navigation state | `$XDG_STATE_HOME/gds-agents/<connection-key>.json` |

XDG directories use their normal user-directory fallbacks. Override the settings file with `--settings-file` or `GDS_AGENTS_SETTINGS_FILE`. BasG preferences are not imported automatically. Settings writes use the shared atomic persistence mechanism, including conflict detection and protection for managed files. The cache is rebuildable and has an application-managed 512 MiB budget.

In-memory diagnostic logs and the log viewer require `--enable-logs`; Ctrl+Shift+L opens the viewer when enabled. [Embedded terminals](../terminal.md#agents) are available only for local IPC workspaces.

## Implementation boundaries

Application code is in [`bin/agents/src`](../../../bin/agents/src), organized as `app → pages → widgets → features → entities → shared`. Session and backend entities own protocol state and history; widgets own settings, history and help views; features own navigation, authentication and session deletion. Frontend configuration lives in `shared/config`.

The root manifest uses the `gds-client-multiplex` branch of `GanDesktop/bas`; `Cargo.lock` pins the BAS revision. `bas-client` depends on `bas-protocol` for shared models and wire types; test services depend explicitly on `bas-server`. The frontend does not embed the Bas daemon or CLI. Agent protocol, authentication, installation and session models remain application code. Shared appearance, typography, Frame, navigation, notifications, shortcuts, clipboard access, timestamps and terminal rendering come from `gds`.

Bas runs as a headless service. GDS Agents owns message assembly, rich-content conversion and window visibility; it does not send foreground-presence RPCs or rely on daemon desktop notifications.

Project-owned code uses the root Apache-2.0 [license](../../../LICENSE). Dependencies retain their own licenses. Development commands and shared service boundaries are documented in [development](../../development.md).

Prompt submission uses the Bas management API independently of observation. Its admission receipt includes the actual Agent ACP request ID. History queries restore existing content; `ObserveAcp` supplies protocol messages for live rendering. The independent `Attach` subscription supplies BAS-owned Session state and service failures, without conversation content or Agent results. Receipt and observation arrival order does not change presentation. Other session commands, interactive subscriptions and authentication use `bas.v1.AcpTransport`; session callback connections skip duplicate content replay. Multiple frontends can subscribe to the same session and answer its pending interactions; the first valid answer resolves the request for all subscribers. Disconnecting a frontend does not cancel an Agent request. See the [multiplexing contract](../../development/agents-acp-multiplexing.md) and [event-stream boundaries](../../development/agents-event-streams.md).

Bas owns background execution. GDS keeps its cursor-based history and acceptance queries on the Bas management API, so reconnecting restores the timeline without resending a prompt. Callback IDs belong to one ACP connection; responses from a disconnected connection are rejected. The frontend requires the pinned Bas protocol; it has no old-protocol fallback or historical-record conversion.
