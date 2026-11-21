# GDS Agent & Maintainer Guidelines

## Core Mindset

Write code with holistic architectural taste. Every change should move GDS toward a cleaner, simpler system rather than adding localized complexity just to pass an immediate task.

## Project Goal & Platform Support

GDS (Gan Desktop Suit) is a suite of native desktop applications built around one shared visual language. Currently, `gds-files` (on `zgui`) and the UI-independent `xdg-desktop-portal-gds` are active binary targets; `agents`, `nixes`, and `images` retain their source on disk while excluded from active build, lint, and test targets until migrated.

- **Primary target**: Linux + Wayland (verified internally first on NixOS + Niri; user-facing docs present Linux + Wayland as the primary target without over-indexing on internal test setups).
- **Secondary target**: Linux + X11 (`zgui` winit X11 backend).
- **Future targets**: Windows and macOS.

## Tech Stack

1. **UI Runtime**: [`zgui`](https://github.com/zortax/zgui), using the pinned [`GanDesktop/zgui`](https://github.com/GanDesktop/zgui/tree/gds-0.1.0) (`gds-0.1.0`) fork
2. **Component Primitives & Styling**: `zgui-ui`, `zgui-ui-primitives`, `zgui-ui-tokens`, `zgui-ui-icons`, and Stylo CSS custom properties (`--gds-*` / `--zui-*`)
3. **Declarative UI Composition (Mandatory)**: Use `zgui`'s `view!` and `#[component]` macros (`css!`, `slot`, `style`, `variants`) alongside shared anatomical components in `src/ui/` (such as `Frame`, `SideBar`, `ContentPage`, `DialogActions`). Do not bypass `view!` with sprawling imperative element-builder chains.
4. **Internationalization**: Mozilla Fluent (`gds::i18n::{t, text, text_args}`).

## Repository & Crate Architecture

- **Single root crate**: Shared library code lives in `src/lib.rs` (default features empty). `gds-files` (`bin/files/src/`) and `xdg-desktop-portal-gds` (`bin/portal/src/`) are declared as `[[bin]]` targets with `required-features` in the root `Cargo.toml`; unmigrated `bin/agents/` and `bin/nixes/` remain on disk outside active `[[bin]]` targets. Procedural macros live in `macros/` (e.g., `macros/gds-settings-derive`).
- **Strict feature isolation**: Optional modules and dependencies share capability feature names. `desktop-protocol` stays pure Serde; `portal` must never pull in `zgui`, SQLite, terminal, or remote file backends.
- **Packaging**: Build and distro definitions live in `packaging/` (`nix/`, `fedora/`, `debian/`, `arch/`, `linux/`, `cloudflare/`). Root `flake.nix`, `flake.lock`, and `shell.nix` delegate to `packaging/nix/`. Build outputs stay in `target/`.
- **Single license**: All first-party code shares the root `LICENSE` (Apache-2.0). Do not duplicate `LICENSE` or `NOTICE` inside `bin/*`.
- **Application identity & icons**: Use namespace `io.github.GanDesktop`. In-app identity icons use `assets/<app>/icon-solid.svg`; desktop launchers use `assets/<app>/icon-desktop.svg` and localized entries from `assets/<app>/desktop.toml`.
- **Product naming**: Use `GDS`, `GDS Files`, `GDS Agents`, `GDS Nixes`, and `GDS Portal` externally. Desktop launcher `Name` uses `Files` / `Agents` without the `GDS` prefix (`(Dev)` appended for `install-dev`). `FileChooser Portal` refers only to the standard freedesktop interface.

## Dependencies & Runtime Boundaries

- **No vendored third-party source**: Never copy upstream crates into `vendor/`. Maintain required forks on versioned branches in `GanDesktop/*`, reference them directly via Cargo Git dependencies (`branch`/`rev`) without `[patch.crates-io]`, pin commits in `Cargo.lock` + `packaging/nix/cargo-lock.nix`, and document active forks in [docs/decisions/upstream-patches.md](docs/decisions/upstream-patches.md).
- **Tokio for application I/O, `zgui` for UI**: All application async I/O (processes, signals, timers, `zbus`, SQLite, SFTP/SMB, PTY) runs on Tokio owned by `service::task_manager::Supervisor`. Do not introduce `async-io`, `async-process`, or `async-signal` in application code. Services accept task `Scope`s rather than raw Tokio runtime handles, and `src/service/*` never depends on `ui` or `zgui` types.
- **Application slices (FSD)**: GUI binaries follow Feature-Sliced Design (`app → pages → widgets → features → entities → shared`). Cross-slice imports go strictly through `<slice>.rs` public APIs; same-layer slices never depend on each other except explicit entity relationships exposed under `entities/<provider>/x/<consumer>`.
- **Context accessors**: `gds::app::Gds` (`Gds::current()` / `Gds::try_current()`) exposes typed suite services (`tasks()`, `appearance()`, `i18n()`, `ui()`) inside the `zgui` reactive root. App-wide reactive state and watchers register under `Gds::with`; window-scoped work uses its reactive `Owner`, `Tasks::owner_scope`, and `Tasks::spawn_ui`.
- **Opt-in diagnostics**: CLI flags use `clap`. In-memory logs and the debug/task viewer (`Ctrl+Shift+L`) are created **only** when `--enable-logs` is passed.

## UI & Reusability Principles

1. **Let icons and UI speak first**: Keep labels minimal and calm, backed by concise hover tooltips and accessible names.
2. **Consistent visual language**: Identical semantics must look and behave identically across every GDS app. Codify shared rules in [docs/visual-language/](docs/visual-language/README.md).
3. **Design for suite reuse**: Build general components in `src/ui/` rather than app-specific one-offs whenever multiple apps share the pattern.

## Documentation Principles

1. **Keep root `README.md` clean**: Do not modify `README.md` unless explicitly requested.
2. **Docs are not a diary**: Separate **Current State** (`docs/development.md`, `docs/visual-language/`, `docs/apps/`, `docs/settings/`), **Historical Compromises & Decisions** (`docs/decisions/`), and **Future Plans** (`docs/roadmap/`). Revise facts in place and delete obsolete descriptions; never append dated work logs, debugging transcripts, or per-round verification diaries.
3. **Language policy**: All developer, architecture, visual-language, decision, and roadmap documentation (including this file) must be written in **English**. User-facing usage/installation manuals are written in English first, with localized translations placed under `<doc-dir>/i18n/<lang>/` using matching filenames.
4. **Progressive disclosure**: Keep entry documents concise and scannable. Present mental models, boundaries, and tables upfront, and link to focused sub-documents or source files instead of dumping implementation minutiae into prose walls.

## Rust Coding Standards

*(Applies to hand-written repository code, not generated code.)*

### Module Layout & Exports

- **No `mod.rs`**: Use Rust modern module layout (`foo.rs` alongside `foo/bar.rs`, `foo/assets/`, and `foo/tests.rs`).
- **Semantic hierarchy**: Group related implementations (`impls/`) and models (`model/`) into logical subtrees rather than flattening unrelated concepts into one directory.
- **Clean re-exports**: Re-export cohesive public surfaces from module roots so callers can import cleanly without noisy git diffs.
- **Methods over free functions**: Place operations that semantically belong to a type inside its `impl` block.
- **Avoid redundant prefixes & tuples**: Use module namespaces (`system::Request`, `profile.name`) instead of stuttered identifiers (`SystemRequest`, `profile.client_name`). Prefer named structs over multi-element tuples or functions with long parameter lists.
- **Wire compatibility**: Reuse existing `*Json` wire structs where appropriate; mark newly added fields `Option<T>` with `#[skip_serializing_none]`.

### Async, Control Flow & Error Handling

- **Idiomatic Rust 2024**: Use `let ... else`, early returns, and loop break values to keep nesting flat. In `tokio::select!`, map branches to a local `Event` enum first and handle state transitions outside the macro.
- **Native async**: Prefer `impl Future` or `BoxFuture`; avoid `async_trait` unless object safety requires it and the trade-off is justified.
- **Typed vs external errors**: Use `thiserror::Error` for internal domain/service errors and `anyhow::Error` at uncontrolled external boundaries (CLI invocations, network/RPC integration). Document fallible public functions with `# Errors`.
- **Never swallow errors**: Handle or propagate every error explicitly. Replace unavoidable `unwrap()` calls with `expect("clear invariant reason")`.
- **Magic values**: Trivial constants (`0`, `1`, `24` hours) and one-off test literals are fine inline; user-tunable values belong in configuration defaults; fixed non-trivial domain constants belong in named `const` or `static` items.
- **Prefer proven standard/ecosystem APIs**: Use `str::strip_prefix`, `split`, `trim`, and battle-tested crates rather than hand-rolled byte slicing or half-baked wheels.

### `zgui` & Reactive Ownership Conventions

- Import `t`, `view`, `component`, and `zgui::prelude::*` locally at call sites rather than writing repetitive full paths.
- Style components via `css!` / Stylo stylesheets using semantic `--gds-*` and `--zui-*` custom properties; avoid hardcoding theme colors in component logic.
- Prefer member methods or helper callbacks over deeply nested inline `on:click` / `on:pointer_down` closures.
- **Stable owner lifetimes & `RefCell` borrow discipline**: Construct signals, `RenderEffect`s, and `on_cleanup_local` guards under their stable reactive `Owner` (window or `Gds::with` root), never inside transient reactive branches that unmount early. Always drop `RefCell` borrows before updating signals (`RwSignal::set` / `update`), reporting notices, or invoking callbacks that may re-enter or render the same state. Guard async completions with weak references, task scopes, and monotonic generations.
- **Capture handles before `on_cleanup_local`**: `zgui` runs `on_cleanup_local` callbacks with **no active `Owner`**. Never call `use_local_context`, `try_use_*`, or `*::current()` inside a cleanup closure—capture the needed handle (`Windows`, `KeytipRegistry`, `RwSignal`, etc.) when registering the cleanup.
- **Provide context only inside `#[component]` or `Scoped::new`**: Dynamic `{move || ...}` view holes (`in_hole`) evaluate their closure inside `RenderEffect`'s child `Owner` while building the returned `AnyView` under the parent hole `Owner`. Never call `provide_local_context` / `.provide()` inside plain `fn(...) -> AnyView` helpers; use `#[component]` or `Scoped::new` so the provider and its subtree share one `Owner`.
- **Token-guarded cleanup on shared slots**: Conditional flow (`Show` / `Branch`) builds the incoming branch *before* running the outgoing branch's `cleanup()`, and exit animations keep outgoing subtrees mounted briefly. Never write unguarded `*slot = None` inside `on_cleanup_local`; key shared registrations by a monotonic ID/generation, or keep state inside the component instance.
- **Canonical modal & menu hosts**: Build modal dialogs on `ModalDialog` / `ConfirmDialog` (`src/ui/feedback/dialog.rs`) and menus on `#[component]` menu hosts (`src/ui/menu.rs`) rather than hand-wiring raw `DialogContent` or `OverlayState` + `MenuContext`. Opening any modal dialog must dismiss active transient menus/popovers first.
- **Isolate distinct `AnyView` element branches**: `AnyView::rebuild` dispatches by `TypeId`; when two conditional branches both return `AnyView::new(view! { box(...) })` (the same `Element<Tag>` type), `ElementState::apply` patches the existing DOM node in place and retains any `style:*`, `attr:*`, `a11y:*`, or `class` (when incoming `classes` is empty) omitted by the new branch. Use `zgui::view::Either` (`Left` / `Right`), `Show`, `AnyView::new(())` for empty branches, and CSS classes instead of branch-only inline `style:*` overrides.

### Comments

- **Sparse and forward-looking**: Comment only non-obvious invariants or active compromises. Explain *current behavior and constraints* for future maintainers—never narrate deleted past attempts ("Previously we did A, then switched to B").
- **Cite authoritative sources**: Link directly to specifications (freedesktop, Wayland, RFCs), upstream issues, or source commits instead of writing long self-justifying essays.

## Testing Philosophy

1. **Prioritize shipping features**: Do not get bogged down writing exhaustive micro-tests mid-development; verify core behavior and hand off cohesive milestones for developer testing.
2. **Delete one-off verification**: Temporary reproduction scripts, scratch harnesses, and one-time inspection fixtures belong in temporary directories and must never be committed.
3. **End-to-end first, minimal white-box unit tests**:
   - Prefer isolated E2E workflows that produce reproducible artifacts (see [docs/apps/files/testing.md](docs/apps/files/testing.md)).
   - Write white-box unit tests (in `<module>/tests.rs`) **only** for complex pure logic or race/error boundaries that E2E tests cannot deterministically trigger.
   - Never write change-detector tests that lock colors, CSS/style class names, static copy, or internal struct layouts.
   - No mocks except at uncontrollable external process/service boundaries; no `sleep`-based timing assumptions; no branching/loops inside test bodies; full isolation between tests.
