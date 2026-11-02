# Terminal backend boundary

GDS uses `alacritty_terminal = 0.26.0` for the terminal model, grid, selection,
ANSI/UTF-8 parser and synchronized-update parser state. `portable-pty = 0.9.0`
provides PTY allocation, handles and resize. Neither requires a standalone
Alacritty installation. No third-party implementation is copied into GDS.

## Why not the upstream tty/event loop?

The public APIs are usable without GPUI or winit. They were evaluated against
upstream commit `94e7c8874e526b1e67b349d9ba30ddf81669119e`, not rejected because
of a GUI dependency. The current decision is **not** to adopt that event loop:

| API boundary | GDS contract that must remain |
| --- | --- |
| `EventLoop::channel()` sends to unbounded `mpsc`; `Msg::Input` feeds a private FIFO, with no public write-progress acknowledgment. | Admission remains byte-bounded until bytes actually reach the PTY; terminal replies have priority over queued user input. A bounded feeder alone cannot bound the hidden downstream queue. |
| Event callbacks run while the terminal is locked. The parser and read loop expose no after-parse query hook. | OSC replies must consult dynamic overrides after parsing, then current theme defaults, without relocking the terminal from its callback. |
| `OnResize` has no error result; registration/read/write failures in `EventLoop::spawn` are logged rather than returned as a structured terminal outcome. | Startup, resize and transport errors must reach the service status and cleanup barrier. |
| Native Unix `tty::from_fd` constructs its own command and ignores failed `chdir`; native `Pty::drop` signals/waits the direct child. | Literal argv, pinned working-directory identity, exec-error reporting, inherited descriptor/signal-mask hygiene, and Linux foreground/background/stopped-job cleanup belong to the host. |

Adopting this version without weakening those contracts would require a custom
`EventedPty`, an admission/write-acknowledgment bridge, a deferred-query supervisor
and the existing process-session cleanup. That adds parallel bookkeeping rather
than removing it. Reconsider the decision when upstream exposes suitable bounded
write/priority, post-parse and error-reporting hooks. Do not add a second parser
or I/O path merely to claim reuse.

Sources: [event loop](https://github.com/alacritty/alacritty/blob/94e7c8874e526b1e67b349d9ba30ddf81669119e/alacritty_terminal/src/event_loop.rs),
[Unix tty](https://github.com/alacritty/alacritty/blob/94e7c8874e526b1e67b349d9ba30ddf81669119e/alacritty_terminal/src/tty/unix.rs).

## Readiness and ownership

The existing per-session worker runs a Tokio current-thread driver. It uses
`AsyncFd` for PTY readiness, `Notify` for queued control work, Tokio SIGCHLD
notification for child exit, and the parser's exact synchronized-update deadline.
There is no additional I/O worker or second parser, and no dependency on the
GUI application's runtime. Blocking spawn and final cleanup remain off the UI
thread.

This replaces the custom `poll` loop, nonblocking UnixStream wake pair, wake-byte
write/drain logic and fixed 25 ms progress polling. SIGCHLD subscription is
established before the first child-status check. Only an actual `WouldBlock`
clears readiness; exhausting an I/O budget retains readiness for the next pass.
EOF/EIO enters cleanup and final drain rather than spinning on a hung-up PTY.
The retained queues and partial-write state are host backpressure/priority
policy, not a second transport alongside an upstream event loop.

## Retained host policy

- One owner reads/writes each PTY and advances one persistent parser. Read and
  write budgets avoid starving control updates; fragmented UTF-8/ANSI and final
  output continue through that same parser.
- Separate byte-limited input and reply queues retain accounting across partial
  writes. Replies precede the next queued user message; an already-started
  message is not interleaved. Queue overflow is explicit, not silently dropped.
- Resize clamps dimensions/history before terminal reflow. OSC52 never reads or
  writes the desktop clipboard. Color queries use emulator overrides and the
  shared latest [appearance palette](terminal-theme.md).
- Launch is cancellable before or during startup. No UI call joins the worker.
  Dropping the final owner requests cleanup; explicit close/quit futures wait
  for actual completion, not merely a shutdown flag.
- Linux cleanup enumerates process groups in the PTY session, sends HUP and CONT,
  allows a short grace period, re-enumerates before KILL, and always reaps the
  direct child. Completion is published after owned PTY handles are dropped.

## Verification and limits

Theme work was integrated and verified separately before changing backend I/O.
The stage-one accepted checkpoint is
`74f0b578946c6c3010bdf369a8b9eb6db6db87ad`; it passed theme/terminal regression
suites and Files/Agents compilation. The backend change and its additional
regressions are separate reviewable commits, not a combined theme/I/O rewrite.

Run validation in the existing development environment, without installation:

```sh
nix develop --impure --expr 'import ./shell.nix' --command \
  cargo test --locked --lib --features gui-tests appearance::theme -- --test-threads=1
nix develop --impure --expr 'import ./shell.nix' --command \
  cargo test --locked --lib --features gui-tests terminal -- --test-threads=1
nix develop --impure --expr 'import ./shell.nix' --command \
  cargo check --locked --features files,agents --bins
```

Linux PTY regression tests are not a desktop visual acceptance test. Interactive
Wayland IME, transparency, font fallback and real application close behavior
still need manual acceptance. Windows PTY is unsupported; macOS and other Unix
behavior is not claimed verified. Non-Linux process-group discovery is weaker.
Processes that leave the PTY session, gain privileges, or outlive a GDS crash are
not guaranteed to be reclaimed. There is no remote terminal or persistent daemon.
