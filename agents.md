# GDS Agent & Maintainer Guidelines

## Core Philosophy: Quality, Gestalt & *Shì* (势)

**Coding is complex systems engineering—it demands a holistic grasp of the system rather than the mechanical assembly of isolated fragments.** Write code with architectural taste so every change moves GDS toward a cleaner, simpler whole instead of bolting on localized complexity to pass an immediate task. What we call "code quality" is **Quality** (*良质*) itself: perceived across facets as **Gestalt** (a whole greater than the sum of its parts), as ***Shì* (势)** (the relational configuration and tension among elements), and as **engineering art and aesthetics**.

Reject keyhole-style local patching:

- **Whole Before Part**: No struct, function, or module holds meaning in isolation; its meaning lives in its relationships with everything around it. Step back to perceive the surrounding topology and context before editing so new code grows naturally from the existing grain without bolted-on seams.
- **Shape Code Through Relationships**: Conceptually peer modules and dual operations should share symmetric structures and naming rhythms; subordinate details should recede beneath narrow interfaces; tightly coupled state and behavior belong together, while orthogonal concerns keep clear boundaries.
- **Use Aesthetic Intuition to Detect Imbalance**: Ugly, contorted code riddled with special cases and pass-through patches is rarely a mere stylistic blemish—it is an alarm that a local change has broken the system's Gestalt and *Shì*. When a new requirement feels awkward to force in, never jam it in just to minimize a local diff; return to the whole-system view, realign conceptual boundaries, and implement cleanly within the reshaped design.

## Project Goal & Platform Support

GDS (Gan Desktop Suit) is a suite of native desktop applications built around one shared visual language. Currently, `gds-files` and `gds-images` (on `zgui`) and the UI-independent `xdg-desktop-portal-gds` are active binary targets; `agents` and `nixes` retain their source on disk while excluded from active build, lint, and test targets until migrated.

- **Primary target**: Linux + Wayland (verified internally first on NixOS + Niri; user-facing docs present Linux + Wayland as the primary target without over-indexing on internal test setups).
- **Secondary target**: Linux + X11 (`zgui` winit X11 backend).
- **Future targets**: Windows and macOS.

## Tech Stack

1. **UI Runtime**: [`zgui`](https://github.com/zortax/zgui), using the pinned [`GanDesktop/zgui`](https://github.com/GanDesktop/zgui/tree/gds-0.1.0) (`gds-0.1.0`) fork
2. **Component Primitives & Styling**: `zgui-ui`, `zgui-ui-primitives`, `zgui-ui-tokens`, `zgui-ui-icons`, and Stylo CSS custom properties (`--gds-*` / `--zui-*`)
3. **Declarative UI Composition (Mandatory)**: Use `zgui`'s `view!` and `#[component]` macros (`css!`, `slot`, `style`, `variants`) alongside shared anatomical components in `src/ui/` (such as `Frame`, `SideBar`, `ContentPage`, `DialogActions`). Do not bypass `view!` with sprawling imperative element-builder chains.
4. **Internationalization**: Mozilla Fluent (`gds::i18n::{t, text, text_args}`).

## Repository & Crate Architecture

- **Single root crate**: Shared library code lives in `src/lib.rs` (default features empty). `gds-files` (`bin/files/src/`), `gds-images` (`bin/images/src/`), and `xdg-desktop-portal-gds` (`bin/portal/src/`) are declared as `[[bin]]` targets with `required-features` in the root `Cargo.toml`; unmigrated `bin/agents/` and `bin/nixes/` remain on disk outside active `[[bin]]` targets. Procedural macros live in `macros/` (e.g., `macros/gds-settings-derive`).
- **Strict feature isolation**: Optional modules and dependencies share capability feature names. `desktop-protocol` stays pure Serde; `portal` must never pull in `zgui`, SQLite, terminal, or remote file backends.
- **Packaging**: Build and distro definitions live in `packaging/` (`nix/`, `fedora/`, `debian/`, `arch/`, `linux/`, `cloudflare/`). Root `flake.nix`, `flake.lock`, and `shell.nix` delegate to `packaging/nix/`. Build outputs stay in `target/`.
- **Single license**: All first-party code shares the root `LICENSE` (Apache-2.0). Do not duplicate `LICENSE` or `NOTICE` inside `bin/*`.
- **Application identity & icons**: Use namespace `io.github.GanDesktop`. In-app identity icons use `assets/<app>/icon-solid.svg`; desktop launchers use `assets/<app>/icon-desktop.svg` and localized entries from `assets/<app>/desktop.toml`.
- **Product naming**: Use `GDS`, `GDS Files`, `GDS Images`, `GDS Agents`, `GDS Nixes`, and `GDS Portal` externally. Desktop launcher `Name` uses `Files` / `Images` / `Agents` without the `GDS` prefix (`(Dev)` appended for `install-dev`). `FileChooser Portal` refers only to the standard freedesktop interface.

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

*(Applies strictly to hand-written repository code, not generated code.)*

### Module Architecture & Organization

- **No `mod.rs`**: Use Rust modern module layout (`<module>.rs` alongside `<module>/<submod>.rs`, `<module>/assets/` for hard-coded static fixtures, and `<module>/tests.rs` for white-box unit tests).
- **Directory hierarchy mirrors conceptual hierarchy**: Sibling files in the same directory must represent peer concepts at the same architectural level. Group subordinate implementations (`impls/`) and domain models (`model/`) into dedicated subdirectories rather than flattening unrelated concepts together.
- **Deep modules & narrow interfaces**: Design for progressive disclosure of complexity:
  - Keep struct fields, helpers, and internal types private or `pub(crate)` by default; expose only the minimal high-level surface without leaking implementation mechanics.
  - Break overly long functions into single-responsibility helpers, but **do not extract trivial, single-call snippets without independent domain meaning into thin pass-through wrappers**. Every layer of indirection must genuinely hide complexity or enable reuse.
  - Re-export cohesive public surfaces from module roots so callers import cleanly without coupling to internal file splits, keeping `use` blocks and Git diffs minimal.
- **Methods over free functions**: Place operations that semantically belong to a type inside its `impl` block.

### Type Semantics & API Design

- **Make illegal states unrepresentable**:
  - **Payload-bearing `enum`s over parallel `Option`s**: Never combine a status field with parallel `Option` fields for phase-dependent data (e.g., `status`, `result: Option<T>`, `error: Option<E>`). Model mutually exclusive states directly as payload-bearing `enum` variants, and prefer exhaustive `match` arms without wildcard `_` catches so the compiler flags every call site when a variant is added.
  - **Parse, don't validate & Newtypes**: Parse raw inputs once at system boundaries via `TryFrom` / `FromStr` into invariant-preserving `Newtype`s or self-describing `enum`s, then trust those invariants internally. Reject ambiguous positional `bool` or bare `Option` parameters (e.g., `foo(false, None)`).
  - **Avoid raw tuples**: Define named structs with explicit field semantics instead of multi-element anonymous tuples in signatures.
- **Standard conversion traits**: Prefer `From` / `Into`, `TryFrom` / `TryInto`, and `FromStr` / `Display` over ad-hoc conversion method names. Use the `!` (never) type stabilized in Rust `1.100` as the associated error type for infallible implementations of fallible generic traits.
- **Intentional naming & parameter grouping**:
  - Leverage module namespaces (`system::Request`, `profile.name`) instead of stuttered prefixes (`SystemRequest`, `profile.client_name`), except when flattened into a unified `model` prelude where module context is lost.
  - Fold long positional parameter lists into cohesive structs separating target descriptor, primary payload, execution context, and optional settings.
- **Wire compatibility**: Reuse existing `*Json` wire structs where appropriate; mark newly added fields `Option<T>` with `#[skip_serializing_none]` so omitted fields stay backward-compatible.

### Core Design Patterns

- **Accessor Pattern over one-to-one Extension Traits**: Never abuse singleton `XxxExt` traits to bolt domain methods onto a concrete `struct` or `enum` (which pollutes flat namespaces, requires pervasive trait imports, and cannot carry module-scoped context). Instead, expose contextual view/accessor wrappers (generic over `Deref`/`DerefMut` or `Borrow` to preserve read/write borrow isolation and secondary namespaces like `repo.user_data().get_profile(id)`). Only an outermost composition facade may `Deref`/`DerefMut` to forward base methods; submodule accessors must never `Deref` back into the underlying context. Reserve extension traits strictly for extending a `trait` (`StreamExt: Stream`) or universal combinators on unmodifiable `std` types named after their capability (`Context`, `ByteSlice`).
- **Typed capabilities & proof tokens**: Hoist transactions, locks, and authorization capabilities to the orchestration layer and pass them explicitly—never open hidden transactions inside low-level methods or hide them in task-local state. When a workflow must complete a terminal step (e.g., committing a transaction or flushing an audit record), return an unforgeable proof token with a private constructor (e.g., `Committed<T>`) producible only by that terminal operation.
- **Typestate for complex multi-phase flows**: When a procedural flow accumulates phase-dependent `let mut Option<T>` locals or tangles progression, retries, and rollbacks, decompose it into payload-bearing state nodes (`Flow<S>`) + transition `enum`s whose transitions consume `self` by value and hold phase-scoped data directly in `state: S` (using zero-sized unit structs rather than `PhantomData` + `Option`). Apply with restraint—keep short, linear flows direct without unnecessary state structs.
- **Polymorphism dispatch ladder**: Do not reflexively reach for `Arc<dyn Service + Send + Sync>` out of OOP habit, nor let generics infect long-lived top-level containers. Choose the lightest dispatch that fits:
  1. **Closed, finite set → `enum` dispatch** (zero dynamic overhead, native async, exhaustive matching).
  2. **Localized static composition → generics `T: Trait`** at short-lived call/accessor boundaries.
  3. **Open heterogeneous collections or deliberate type-erasure boundaries → `dyn Trait`**.

### Async, Concurrency & Error Handling

- **Idiomatic Rust 2024**: Use `let ... else`, early returns, and loop break values to keep nesting flat. In `tokio::select!`, map branches to a local `Event` enum first and handle state transitions outside the macro.
- **Native async**: Prefer `impl Future` or explicit `BoxFuture`; avoid `async_trait` unless object safety requires it and the trade-off is justified.
- **Concurrent state management**:
  - **Short synchronous locks; never hold across `.await`**: Guard pure in-memory state with `std::sync::Mutex` (or `parking_lot`), complete reads/writes inside a tight lexical block so the guard drops immediately, and only then `.await`.
  - **Actor model for stateful async components**: Avoid sharing `Arc<tokio::sync::Mutex<BigState>>` across async I/O. Spawn an owning task driven lock-free via `loop` + `select!` behind a lightweight handle wrapping `mpsc::Sender<Command>` (with `oneshot::Sender` for replies).
- **Typed vs external errors**: Use `thiserror::Error` for structured internal/domain errors and `anyhow::Error` at uncontrolled external boundaries (CLI invocations, network/RPC, subprocess wiring). Document fallible public functions with `# Errors`.
- **Never swallow errors & ban bare `unwrap()`**: Handle or propagate every error explicitly without conflating distinct failure modes. Refactor control flow to avoid `unwrap()`; where an invariant is statically guaranteed, use `expect("clear invariant reason")`.

### Implementation Hygiene & Comments

- **Trust proven code**: Prefer standard combinators (`str::strip_prefix`, `split`, `trim`) over manual byte-index slicing, and inspect mature ecosystem crates before hand-rolling custom infrastructure.
- **Governing magic values**: Evaluate literals against four tiers: (1) one-off test fixtures inline; (2) self-evident trivial constants (`0`, `1`, `24` hours) inline; (3) runtime- or environment-tunable parameters in configuration structs with sensible defaults; (4) fixed domain/protocol constants as named `const` or `static` items.
- **High-signal, forward-looking comments**: Comment only non-obvious invariants, edge cases, or active compromises. Explain *why the current code behaves as it does* for future maintainers—never narrate deleted attempts or private chat history ("Previously we did A...", "Per point 2 from our chat..."). Cite authoritative sources (freedesktop/Wayland specs, RFCs, issues, commits) directly rather than writing prose essays.

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

## Testing & Verification Workflow

1. **Prioritize shipping features**: Do not get bogged down writing exhaustive micro-tests mid-development; verify core behavior and hand off cohesive milestones for developer testing.
2. **Delete one-off verification**: Temporary reproduction scripts, scratch harnesses, and one-time inspection fixtures belong in temporary directories and must never be committed.
3. **End-to-end first, observable contracts, minimal white-box unit tests**:
   - Prefer isolated E2E workflows that produce reproducible artifacts (see [docs/apps/files/testing.md](docs/apps/files/testing.md)) and black-box integration tests under `tests/`.
   - Place white-box unit tests in dedicated `<module>/tests.rs` submodules **only** for complex pure logic, security/wire invariants, or race/error boundaries that E2E tests cannot deterministically trigger.
   - Assert externally observable contracts—never write change-detector tests that lock colors, CSS/style class names, static copy, or internal struct layouts.
   - **Strict parallel isolation**: Never mutate process-global state (such as `std::env::set_var`) inside tests; inject environment-derived configuration explicitly. No mocks except at uncontrollable external process/service boundaries; no `sleep`-based timing assumptions; no branching/loops inside test bodies.
4. **Pre-delivery verification loop**: Writing code is not finishing the task. Before completing any code change, run and pass the checks for the affected targets in order (via `just` or inside the locked Nix dev shell `nix develop --impure --expr 'import ./shell.nix'`):
   1. Lint: `just lint` / `cargo clippy --all-targets -- -D warnings` (resolve all warnings)
   2. Format: `just fmt-check` / `rustfmt --edition 2024 --check` on touched files
   3. Test: `just test` / `cargo test` for all affected unit and integration tests
