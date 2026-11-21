# Embedded Terminals

GDS Files and GDS Agents share the local PTY terminal engine in `gds::service::terminal` and its `zgui` view adapter in `gds::ui::terminal`. Terminals execute directly on the host running GDS (Linux local PTYs); there is no persistent background daemon, remote SSH terminal RPC, or implicit Agent shell-execution grant.

## Architecture & Layering

```text
Files / Agents Host: session collection, tabs, dock split, cwd & shortcuts
                                  │
                                  ▼
              gds::ui::terminal::Terminal (zgui adapter)
               focus, monospace shaping, grid paint, IME
                                  │  plain Rust input/state/events
                                  ▼
           gds::service::terminal::Terminal (session owner)
             async launch, bounded I/O queues, PTY cleanup
                                  │
           alacritty_terminal 0.26 (grid & ANSI/UTF-8 parser)
                                  │
                    portable-pty 0.9 + user shell
```

- **Strict UI/service boundary**: `service::terminal` has zero `zgui`, UI, Files, or BAS dependencies. `Terminal::prepare(scope, options)` returns a `Terminal` handle and blocking `Launch` task; callers run `launch.run()` off the UI thread, await `terminal.ready()`, and interact via `Session`.
- **Dedicated session worker**: Each PTY runs a single Tokio current-thread worker combining `AsyncFd` readiness, `Notify` control wakes, `SIGCHLD` child-exit monitoring, byte-bounded input/reply queues (giving terminal OSC replies priority over queued keystrokes), and exact synchronized-update deadlines. For why GDS runs its own worker instead of upstream Alacritty's event loop, see [Terminal PTY Loop & Directory Policy](../decisions/terminal-pty-loop.md).
- **Process-group cleanup**: Closing a terminal or window awaits actual cleanup (`shutdown()`), enumerating Linux process groups in the PTY session (`SIGHUP` + `SIGCONT`, grace period, `SIGKILL` fallback, and direct-child reaping) without prompting for confirmation.
- **Security & isolation**: Ordinary PTYs run with the desktop user's permissions. Terminal output is untrusted: URLs never auto-execute, `OSC 52` never reads or writes the system clipboard, and terminal I/O is never auto-uploaded into Agent conversations.

## Appearance & Font Rendering

- **Shared semantic palette**: `src/appearance/theme/terminal.rs` owns default foreground/background, inverse cursor pair, selection pair, and 16 ANSI colors—importing `terminal.{light,dark}` (foreground/background, cursor, selection, and 16 ANSI entries) from Noctalia v5 or `dank16.color0..color15` from DMS when present, and falling back to GDS's maintained light/dark ANSI table otherwise (partial or malformed terminal sections reject the update atomically; indices `16–255` use the standard xterm cube and gray ramp).
- **Live synchronization**: `ui::terminal` observes the installed appearance snapshot and updates a shared RGB palette created *before* shell launch so live, hidden, and still-starting sessions answer OSC color queries with the latest theme without restarting shells. Truecolor cells and emulator OSC overrides take precedence until reset.
- **Single-pass surface & monospace grid**: Default background uses the window's content material painted once; `Dim` adjusts RGB rather than text alpha. System Fontconfig resolves concrete monospace families so CJK, Nerd Font glyphs, emoji, cursor, and IME share device-pixel-aligned cell advances.

## Host Integration: Files vs Agents

Both applications place the terminal dock beneath the workspace across two columns (sharing the sidebar's width and collapsed state on the left tab rail) and expose a single window-level toggle button in the bottom-right footer (`Ctrl+\``):

| Aspect | GDS Files | GDS Agents |
| --- | --- | --- |
| **Ownership** | Window-level collection (independent of file tabs; switching or closing file tabs never kills shells; unavailable in picker mode) | Per-workspace collection on declared **local IPC** connections only (never enabled on remote or loopback TCP connections) |
| **Empty dock reveal** | Opening an empty dock on a valid local folder spawns one shell automatically; closing the last shell hides/empties without auto-restarting | Managed per workspace; switching workspaces restores each workspace's shells, active tab, and split height |
| **Working directory** | Uses current real local folder (falls back to `$HOME` on virtual/remote/unmounted pages). Header **Open current folder in new terminal tab** spawns a fresh tab—never injects `cd` into a live PTY | Starts in the workspace's primary directory; switching conversations inside the workspace leaves shells untouched |
| **Tab naming & controls** | `F2` sets a custom instance title immune to shell title updates; dock header supports maximize/restore across both columns | Header provides multi-instance switcher (when `> 1` instance), close, and new-terminal actions |

## Input & Shell Configuration

- **Focus routing**: Inside the PTY input scope, `Ctrl+Shift+C/V` copy/paste, `Ctrl+N/W` open/close terminal tabs, `F6` returns focus to the workspace, and `Ctrl+,` / `Ctrl+\`` remain window-level; `Ctrl+C/D/B/L/R`, `Tab` / `Shift+Tab` (bypassing window focus traversal), arrows, and `Esc` pass directly to the foreground process. See [Focus Scope Routing](../visual-language/shortcuts.md#4-focus-scope-routing).
- **Shell resolution**: Uses explicit Terminal settings first, then the account's default shell, then `$SHELL` (interactive non-login by default, ignoring non-interactive Nix dev-shell `BASH` overrides). Nix packages ship `xterm-256color` terminfo while preserving `TERMINFO_DIRS`.
