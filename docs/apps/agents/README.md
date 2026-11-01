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

Frontend preferences and service configuration have separate ownership. Local preferences apply to this application. Service management identifies the connected Bas service and uses revision-aware RPC updates; disconnected edits remain drafts. Agents does not write the daemon's local configuration file.

## Configuration and state

| Data | Default location |
| --- | --- |
| Frontend settings | `$XDG_CONFIG_HOME/gds/agents/settings.json` |
| Connection history cache | `$XDG_CACHE_HOME/gds/agents/history-v1/history.sqlite` |
| Navigation state | The user state directory under `gds/agents` |

XDG directories use their normal user-directory fallbacks. Override the settings file with `--settings-file` or `GDS_AGENTS_SETTINGS_FILE`. BasG preferences are not imported automatically. Settings writes use the shared atomic persistence mechanism, including conflict detection and protection for managed files. The cache is rebuildable and has an application-managed 512 MiB budget.

In-memory diagnostic logs and the log viewer require `--enable-logs`; Ctrl+Shift+L opens the viewer when enabled. [Embedded terminals](../terminal.md#agents) are available only for local IPC workspaces.

## Implementation boundaries

Application code is in [`bin/agents/src`](../../../bin/agents/src), organized as `app → pages → widgets → features → entities → shared`. Session and backend entities own protocol state and history; widgets own settings and history views; features own navigation, authentication and session deletion. Frontend configuration lives in `shared/config`.

The root manifest pins `bas-client` to a Git revision. The frontend does not embed the Bas daemon or CLI. Agent protocol, authentication, installation and session models remain application code. Shared appearance, typography, Frame, navigation, notifications, shortcuts, clipboard access, timestamps and terminal rendering come from `gds`.

Project-owned code uses the root Apache-2.0 [license](../../../LICENSE). Dependencies retain their own licenses. Development commands and shared service boundaries are documented in [development](../../development.md).
