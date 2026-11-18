# GDS Agent & Maintainer Guidelines

## Core Mindset

Write code with holistic architectural taste. Every change should move GDS toward a cleaner, simpler system rather than adding localized complexity just to pass an immediate task.

## Project Goal & Platform Support

GDS (Gan Desktop Suit) shares a native visual language. This experimental branch builds Files with AntiWebUI and the UI-independent Portal; Agents, Nixes and Images retain their source but are excluded from binary targets.

- **Primary target**: Linux + Wayland (verified internally first on NixOS + Niri; user-facing docs present Linux + Wayland as the primary target without over-indexing on internal test setups).
- **Current Files backend**: Wayland only; X11 is outside this migration.
- **Future targets**: Windows and macOS.

## Tech Stack

1. **UI Runtime**: [AntiWebUI](https://github.com/bea4dev/AntiWebUI), using the pinned GanDesktop fork.
2. **Component Primitives**: AntiWebUI native builders, widgets, signals and owners.
3. **Declarative UI Composition (Mandatory)**: Use v2 of **[gds-uim](https://github.com/GanDesktop/gds-uim)** (`el!`, `m!`) to construct UI trees. Declare reusable multi-part components in anatomical form (like `ContentPage::default()`) and instantiate structural components via `Default::default()`. Do not bypass `gds-uim` with sprawling imperative builder chains.
4. **Internationalization**: Mozilla Fluent (`gds::i18n::t` / `tr!`).

## Repository & Crate Architecture

- **Single root crate**: Shared library code lives in `src/lib.rs` (default features empty). Applications live in `bin/<app>/src/` (`files`, `agents`, `portal`, `nixes`) and are declared as `[[bin]]` targets with `required-features` in the root `Cargo.toml`. Procedural macros live in `macros/` (e.g., `macros/gds-settings-derive`).
- **Strict feature isolation**: Optional modules and dependencies share capability feature names. `desktop-protocol` stays pure Serde; `portal` must never pull in a UI runtime, SQLite, terminal, or remote file backends.
- **Packaging**: Build and distro definitions live in `packaging/` (`nix/`, `fedora/`, `debian/`, `arch/`, `linux/`, `cloudflare/`). Root `flake.nix`, `flake.lock`, and `shell.nix` delegate to `packaging/nix/`. Build outputs stay in `target/`.
- **Single license**: All first-party code shares the root `LICENSE` (Apache-2.0). Do not duplicate `LICENSE` or `NOTICE` inside `bin/*`.
- **Application identity & icons**: Use namespace `io.github.GanDesktop`. In-app identity icons use `assets/<app>/icon-solid.svg`; desktop launchers use `assets/<app>/icon-desktop.svg` and localized entries from `assets/<app>/desktop.toml`.
- **Product naming**: Use `GDS`, `GDS Files`, `GDS Agents`, `GDS Nixes`, and `GDS Portal` externally. Desktop launcher `Name` uses `Files` / `Agents` without the `GDS` prefix (`(Dev)` appended for `install-dev`). `FileChooser Portal` refers only to the standard freedesktop interface.

## Dependencies & Runtime Boundaries

- **No vendored third-party source**: Never copy upstream crates into `vendor/`. Maintain required forks on versioned branches in `GanDesktop/*`, reference them directly via Cargo Git dependencies (`branch`/`rev`) without `[patch.crates-io]`, pin commits in `Cargo.lock` + `packaging/nix/cargo-lock.nix`, and document active forks in [docs/decisions/upstream-patches.md](docs/decisions/upstream-patches.md).
- **Tokio for application I/O, AntiWebUI for UI**: All application async I/O (processes, signals, timers, `zbus`, SQLite, SFTP/SMB, PTY) runs on Tokio owned by `service::task_manager::Supervisor`. Do not introduce `async-io`, `async-process`, or `async-signal` in application code. Services accept task `Scope`s rather than raw Tokio runtime handles, and `src/service/*` never depends on `ui` or AntiWebUI types.
- **Application slices (FSD)**: GUI binaries follow Feature-Sliced Design (`app → pages → widgets → features → entities → shared`). Cross-slice imports go strictly through `<slice>.rs` public APIs; same-layer slices never depend on each other except explicit entity relationships exposed under `entities/<provider>/x/<consumer>`.
- **Context accessors**: `Gds::current()` returns the typed application services (`tasks()`, `appearance()`, `i18n()`, `ui()`). Native signals and callbacks belong to explicit reactive owners. App-wide channels use `Gds::with`; window work uses the window owner and its supervised task scope. Do not recreate GPUI contexts or entities.
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

### UIM & Native Ownership Conventions

- Import `el`, `m`, and `t` / `tr` locally at call sites rather than writing full paths.
- Use UIM v2 explicit attribute and child application (`.div.{ ... }.[ ... ]`). Read the installed `gds-uim` skill and current grammar; do not mix in the old implicit-block syntax.
- Prefer member methods or pre-bound closures over deeply nested inline event handlers.
- Construct state under its stable owner. A transient `dyn_view` subtree must not own signals or delivery channels used after that subtree is replaced. Create reactive views inside their owning callbacks.
- Release `RefCell` borrows before publishing signal changes, reporting notices, or invoking callbacks that may render or read the same state. Use weak window references and request identities for asynchronous completion; keep cleanup work supervised until it actually finishes.

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
