# Code review: cleanup and dependency audit

Reviewed the workspace at `6ada883` against [agents.md](../../agents.md): dead
code, over-eager compatibility branches, implementation-detail tests, the root
dependency tree, FSD organization and documentation. This is a pre-release
project with no tagged release, so compatibility kept only for older unreleased
builds is not a contract. This record lists the findings, the removals with their
reasons, the dependency-tree changes and the work that is deliberately retained
or deferred.

## Summary

- Root `Cargo.toml` is grouped by capability; unused dependency features and one
  test-only dependency were removed, and `[build-dependencies]` now declares its
  real inputs. Versions, pins and Git patches are unchanged; `Cargo.lock` did not
  need an edit.
- The unreleased legacy `gds/settings.toml` import and the pre-`004` SQLite
  schema upgrade fixtures were removed together with the code only they
  exercised; current migration application, rollback, retry and data-retention
  coverage is unchanged, and the migration SQL itself is untouched.
- Dead code, over-splitting and duplicated parsing found by the audit were
  removed; platform-specific lint allowances and documentation-backed behavior
  were kept.
- Implementation-detail and duplicate-contract tests were removed from the
  shared library and both applications; workflow, protocol, security and
  data-integrity coverage was kept.
- The FSD layering and slice public-entry rules hold; only documentation drift
  was corrected.
- One unreferenced one-off validation record was deleted from
  `docs/development/`.
- The pre-existing flaky configuration-lock test was fixed with a bounded
  retry for transient `flock` holders; lock semantics, conflict detection and
  the public API are unchanged.

No user-visible behavior, protocol field or public API changed except where a
removal below states its reason.

## Root manifest and dependency tree

- `[dependencies]` is grouped by the capability feature that owns each package;
  Linux/Unix/macOS/Windows packages stay in their target tables, and build/dev
  dependencies sit before the profile tables. Versions, pins, Git revisions and
  `[patch.crates-io]` are unchanged; `Cargo.lock` needed no edits.
- `[build-dependencies]` `serde`/`toml` are no longer optional. `build.rs` parses
  `Cargo.toml` under `cfg(feature = "ui")`; they previously resolved only because
  same-named optional normal dependencies were enabled by unrelated
  capabilities. They are now unconditional build-script inputs.
- Removed dependency features with no code use: `nix/poll`, `nix/term`,
  `time/macros`, `uuid/v7`, `tonic/gzip` and
  `agent-client-protocol/process,schemars`. `bas-client` re-enables the ACP,
  tonic and nix features for the Agents build, so the resolved package set is
  unchanged. The Agents terminal tests keep `nix/term` via a
  `[target.'cfg(unix)'.dev-dependencies]` entry for `nix::pty` and `tcsetpgrp`;
  production builds no longer carry it.
- `regex` was only used by `bin/agents/src/shared/i18n/tests.rs`; it moved from
  the `agents` capability to `[dev-dependencies]`.
- Kept although they look redundant: `rust-embed` (no direct call; forces
  `debug-embed` for gpui-kit asset embedding), `gpui` (`gpui-fast`; pins the
  patched runtime with gpui-kit), and the explicit `image` decoder features
  (header probing; already enabled by GPUI).
- No unused direct dependency remains: every package is referenced by code, by
  the build script, or is one of the feature-only packages above.
- Verification: `cargo metadata --locked` resolves unchanged, and a pre/post
  dependency-spec diff shows only the intended feature edits. `cargo fmt --all
  --check`, the three feature-isolation checks and the workspace all-targets
  check pass with 0 warnings in a clean worktree.
- Deferred: `dirs` and `directories` are both direct dependencies (two
  `dirs::cache_dir` call sites in `bin/agents`); consolidating them needs
  application-code edits outside the manifest change. `chrono` (UI) versus
  `time` + `jiff` (Agents) overlap, but each is used by different formatting
  code.

## Obsolete migration compatibility

This repository has no tagged release (package version `0.0.1`), so migration
code exists only for layouts produced by earlier unreleased builds. Two groups
were removed.

- Suite-wide configuration import: `configuration::load_startup`, the `Loaded`
  wrapper with its `migration` error, `legacy_path`
  (`$XDG_CONFIG_HOME/gds/settings.toml`), the startup `settings-migrate-failed`
  notice and the Fluent key, plus the six tests that only exercised that
  import (`legacy_preferences_are_imported_once_without_touching_the_old_file`,
  `malformed_legacy_preferences_are_reported_without_writing_application_defaults`,
  `a_declarative_legacy_link_is_read_and_the_managed_source_is_untouched`,
  `a_failed_import_keeps_the_legacy_preferences_for_this_run`,
  `an_import_never_overwrites_a_configuration_created_concurrently`,
  `an_import_that_lost_to_a_concurrent_configuration_uses_that_configuration`).
  Each application owns its
  configuration file; the old suite-wide store has no released users, so the
  import, its retry-on-next-start behavior and its startup notice are dead
  compatibility weight. **User-visible removal:** someone still running an older
  unreleased build with only `gds/settings.toml` now starts from defaults
  instead of importing those values; the old file is left untouched. Current
  preference behavior (defaults, round-trip, malformed input, atomic
  replacement, read-only and symlink handling) keeps its tests.
- Pre-`004` SQLite schema upgrades: `directory_view_migration_preserves_existing_column_and_mode_choices`
  (upgrade from `001`) and `unified_places_migration_preserves_local_and_remote_memberships`
  (upgrade from `001`–`003`) were deleted. They reconstructed historical table
  shapes only to prove a one-time transition into the current schema. The
  `001`–`004` migration SQL files themselves are unchanged; current migration
  application, failed-migration rollback, first-migration retry, concurrent
  first open and data-retention coverage in `src/service/storage/tests.rs` is
  retained and passing.

## Dead code and leftovers

Method: `rg` and module-tree reachability over all 710 `.rs` files (roots:
`src/lib.rs`, each `bin/*/src/main.rs`), cross-reference counts for every
function definition outside `tests.rs`, duplicate function-body detection
(normalized body hash), a scan for `dbg!`/`todo!`/`unimplemented!`/commented-out
code/`#[allow(dead_code)]`, and a workspace all-targets check for compiler
dead-code and unused warnings.

Removed (uncalled code):

- `Command::matches_single_stroke`
  (`bin/files/src/features/file_operations/model/commands.rs`): zero
  references; the dynamic hint/badge paths use `binding()` and
  `default_stroke()` instead.
- `Notification::show_in` (`src/ui/feedback/notifications.rs`): zero references;
  `show()` already picks the active window and the private `show_to` remains.
- `Editor::set_enabled` plus its `enabled` field
  (`src/ui/forms/settings/form/editor.rs`): no caller and no test since it was
  introduced, so the flag was always `true`; the `!self.enabled` change guard,
  five `.disabled(!self.enabled)` render calls, the numeric-control disabled
  flag and the reset-button condition were constant branches. The method and
  field were removed and `docs/settings/config-editor.md` updated. **Deliberate
  public API removal:** no application could reach the disabled state; typed
  changes and programmatic `set_value` sync are unchanged.

Duplicated implementations folded (same behavior, one copy):

- The new-folder and paste-conflict dialogs carried a byte-identical private
  `Form` (input/suggestion/display/error state, `chosen_name`, `focus_input`,
  `refresh_error`, `on_input`, `Render`), and the rename dialog repeated the same
  field-plus-error render and focus body. `bin/files/src/pages/workspace/ui/name_form.rs`
  (`NameForm`, `field()`, `focus()`) is now used by all three dialogs.
  Placeholder message ids, lossless non-UTF-8 `OsString` suggestion handling,
  validation and error styling are preserved; net -160 lines.
- Agents had the same tilde-fence builder twice (`ui/chat/content.rs`,
  `ui/tool_content.rs`); it now lives in `ui/markdown.rs::code_block`.

Over-split slice folded:

- `features/file_preview` was a single-function projection (file entry to
  thumbnail `Source`) used only by workspace page rendering, not a user-triggered
  workflow. It is now
  `entities/file/api/directory/thumbnail.rs::thumbnail_source`, re-exported
  through the existing `entities::file::directory` facade, with the three page
  call sites updated. No new cross-slice dependency: pages already depend on
  entities. The `docs/development.md` features row is dropped by this cleanup.

Scan results: all 710 files reachable from their module roots (0 orphan files);
no `dbg!`/`todo!`/`unimplemented!`/commented-out code; the only
`#[allow(dead_code)]`s are three `cfg_attr(not(target_os = "linux"))` recycle
fields; the workspace all-targets check reports zero warnings before and after.

Deliberately kept:

- `WindowControls::session_hides_controls` versus `SurfaceOpacity::for_session`
  are not duplicate policy: `docs/vistual-language/README.md` fixes Niri-only
  controls (name list, `NIRI_SOCKET` fallback), while opacity intentionally
  covers a wider tiling list. Unifying them would change user-visible behavior.
- Files and Agents `terminal::Panel::{set_cwd,set_preferences}` are ~10-line
  setters over different panel state; a shared abstraction would add indirection
  without removing a real contract.
- The `features/navigation` (Agents) facade re-export is kept (downward-only
  dependency, no API change).

## Test coverage trims

### Shared library and Portal

Audited every test module; the suite already follows the project test policy, so
the trim is small.

Removed: two notification height tests that recomputed
`TOOLBAR_HEIGHT`/`TOAST_TOP_GAP`/`CARD_CHROME_HEIGHT`/`TOAST_INSET` to re-check
the `content_height`/`column_height` clamps. They asserted the formula instead of
a visible outcome; the values are three-line guards on the render path.

Retained with reasons: motion/focus tests (the only automated evidence for
reduced-motion and keyboard-focus visibility, with their policy documented in
[docs/vistual-language/motion.md](../vistual-language/motion.md)); keytips
placement invariants (never overlap, stay inside content, stay on their own
control) of a non-obvious layout algorithm; appearance provider parsers (external
schema to semantic-role mapping = palette data integrity); logs redaction and
bounded formatting; PTY control-sequence safety; thumbnail cache
identity/concurrency; remote address validation; desktop portal/watch workflows;
portal D-Bus protocol and path/filter identity; typography/terminal/theme
omitted-field round-trips that protect existing configuration; and the
window-control session tests that encode
[docs/vistual-language/README.md](../vistual-language/README.md).

Deferred: `ui/terminal/preferences/tests.rs`
`terminal_catalog_coexists_with_application_messages` reads the two application
`.ftl` catalogs from the shared crate. Kept as the single check of the shared
`Catalog` merge order (test-only fixture); recorded again in the FSD section.

### Files and Agents

Scope: test modules only under `bin/files/src` and `bin/agents/src`; no
production code or behavior change.

Removed (one-off verification):

- `bin/agents/src/pages/workspace/ui/timeline/tests.rs`:
  `synthetic_hot_cache_benchmark` — an `#[ignore]`-only timing harness
  (`Instant` + `eprintln!`, no assertion) kept for a one-off cache-switch
  measurement. Not a contract test.
- `bin/agents/src/pages/workspace/ui/tests/paged_history.rs`:
  `history_window_render_benchmark` — an `#[ignore]`-only harness that reads a
  developer-local `BAS_HISTORY_BENCH_FILE` JSON and asserts nothing; unusable in
  CI.

Consolidated duplicate contract coverage:

- `bin/files/src/widgets/settings/ui/controls/tests.rs`: seven per-field reset
  tests all asserted the same contract at different granularity. They were
  replaced by two table-driven tests:
  `generated_field_reset_only_changes_its_own_value` iterates every
  `ConfigEditor` field of `Preferences`, `typography::Preferences` and terminal
  `Preferences`; `control_reset_only_clears_its_own_field` iterates every
  hand-written `Field` variant and compares exact field values, proving the
  reset field reaches its default while every sibling keeps its value; the
  fixture modifies all eight controls (including the built-in palette and
  sidebar volumes) so every reset path is exercised. Coverage is now every
  field rather than a hand-picked subset, with about 140 fewer lines.

Removed (implementation/visual detail):

- `bin/files/src/pages/workspace/ui/drag_drop/tests.rs`: three tests that locked
  exact `IconName` choices of `FileIcon::for_entry` (static visual mapping, the
  icon analogue of the color/CSS assertions the policy excludes). The remaining
  drag-drop tests keep selection transfer, grab-point clamping and
  destination-skip behavior.

Retained deliberately:

- Clipboard cut/copy wire protocol, ACP conversation/replay protocol, session
  cache budgets, artwork-cache integrity, trash/recycle round-trips,
  transfer/copy edge cases, terminal process ownership, and non-UTF-8 path
  handling (key workflows, protocol, security, data integrity).
- GUI clipping/segment tests (`menu_rows_keep_full_labels_inside_fixed_width`,
  notification viewport bounds, breadcrumb segmentation) assert user-visible
  layout rather than private implementation, so they stay as regression guards.
- Agents appearance page GUI tests stay: unlike the unit tests they exercise
  save/reload plus live theme reapplication.
- XDG-isolating self-re-executing tests (trash/history) stay: the subprocess is
  the isolation mechanism, not a one-off debug hook.

## Code organization (FSD)

Method: enumerated every layer entry file and slice directory for `src/`,
`bin/files/src`, `bin/agents/src` and `bin/portal/src`; parsed all
`crate::<layer>::<slice>` imports to detect same-layer slice dependencies,
lower-to-upper-layer imports, reaches into private `api`/`model`/`ui` segments,
orphan `.rs` files not declared by any `mod`, and slice directories without a
`<slice>.rs` entry; checked every service source for GPUI / `crate::ui`
references.

Findings:

- No layering violations. Same-layer slice dependencies: 0. Lower-to-upper-layer
  imports: 0. Cross-slice imports of private `api`/`model`/`ui` segments: 0.
  Orphan files: 0 (`src` 198/198, Files 193/193, Agents 314/314, Portal 5/5
  reachable from their module roots). Every Pages/Widgets/Features/Entities
  slice directory has a matching `<slice>.rs` entry. No `mod.rs` layout. Both
  `main.rs` files only declare layers and call `app::run()`. `src/service`
  contains no GPUI or `crate::ui` reference, and there is no reverse UI
  dependency. Root capability direction holds: `appearance`, `i18n` and
  `persistence` do not import upward; `persistence` imports nothing above std.
- The only documented `@x` entity relation is present and used:
  `entities/file/x/location` (location models import `Layout`, `Sort`,
  `ViewMode`, `DirectorySnapshot`). No `x/preferences` relation exists.

Documentation drift fixed:

- The `docs/development.md` layer table now lists the actual slices: Files
  widgets `settings`/`remote_browser`/`help`, Files features
  `file_operations`/`pick_files`/`connect_server`, Files shared
  `i18n`/`lib` (clipboard)/`ui`, Agents widgets `settings`/`history`/`help`.
- The Files UI code-organization paragraph no longer cites the removed
  `toolbar.path`/`toolbar.filter` groups or `collections.*` modules; it points at
  the current `toolbar` inputs, `place_pages/network` and `devices` groups.
- The `@x` sentence now names only `entities/file/x/location`.
- `docs/apps/agents/README.md` now lists `help` among the widget-owned views.

Retained or deferred:

- `features/file_preview` was a one-function projection helper
  (`thumbnail_source`) used only by workspace page rendering. It was folded into
  `entities/file/api/directory/thumbnail.rs` by the over-splitting cleanup, and
  the `docs/development.md` features row is dropped by the same cleanup.
- `features/navigation` re-exports
  `entities::backend::{Profiles, presentation, shared}` as a facade for settings
  and workspace consumers. The dependency direction is still downward-only and
  no public API changes, so it was kept.

## Documentation

- Deleted `docs/development/motion-validation.md`. It was a one-off validation
  record (scope, reproduction commands, commit hashes, per-round test counts,
  transient-failure history and diagnostic limits) of a single change, and
  `docs/development.md` states that one-off validation scripts, logs, screenshots
  and per-round results do not belong in the project documentation. It was
  unreferenced, and its durable policy is already maintained in
  [docs/vistual-language/motion.md](../vistual-language/motion.md); it also named
  an old GPUI Kit version that the pinned Kit no longer matches. The file remains
  recoverable from Git history; its diagnostic history of the configuration-lock
  flake is preserved in the lock section below, so no actionable evidence was
  lost.
- All relative Markdown links under the repository resolve to existing files; no
  broken documentation link was found.
- `docs/apps/files/todo.md` still carries dated per-round verification
  paragraphs. Trimming them would remove product context the owner may still
  want, so this is deferred to the owner instead of being rewritten here.

## Configuration lock and the flaky `Busy` test (owner addendum)

`persistence::tests::writable_configuration_below_a_readonly_ancestor_can_be_created_and_replaced`
intermittently failed with `Error::Busy` on its second back-to-back write during
parallel workspace runs, which rejected otherwise-clean integration candidates.
`src/persistence.rs::File::write` opened `<path>.lock` and called `try_lock`
once, while holding the `flock` across `sync_all` and the atomic replacement.

Independent probes established that rustc's standard library already opens files
with `O_CLOEXEC`, so descriptor inheritance across `exec` was not the observable
mechanism. The window is the fork-to-`exec` interval of child processes spawned
by parallel terminal/auth tests: such a child shares the open file description
until it execs, so a `try_lock` in that interval observes `WouldBlock`.
`O_CLOEXEC` is still spelled explicitly as defense in depth.

Change:

- A shared lock-open helper composes `mode(0o600)` with
  `custom_flags(O_NOFOLLOW | O_CLOEXEC)`.
- Lock acquisition retries `WouldBlock` for a bounded 250 ms before returning
  `Error::Busy`; a genuinely held lock still reports `Busy` after that budget.
  `File`'s public API, content-conflict detection (`Error::Conflict`) and atomic
  replacement semantics are unchanged.
- Regression tests assert `FD_CLOEXEC` directly on the opened lock descriptor and
  poll for the lock to be free while the executed child lives (tolerating
  unrelated pre-exec children forked by parallel tests); a second test holds the
  lock and proves that a genuinely held lock still yields `Busy` after the
  production budget. The positive retry path has no unit test here; it is
  covered by the parallel-suite soak reported below. The original flaky test
  keeps both writes and all assertions; only a transient `Busy` caused by the
  harness's own subprocesses is retried for up to 10 s inside the test, while
  production still reports `Busy` after 250 ms. Nothing is ignored or serialized.
  Earlier revisions of the regression test were rejected by integration
  verification because a single-shot probe raced other tests' pre-exec children;
  the final shape removes that race.

## Verification

Baseline `6ada883` (before this review) and the final integrated tree at accepted
head `6ee913c` were checked with the same commands:

| Command | Baseline | Final |
| --- | --- | --- |
| `cargo fmt --all --check` | pass | pass |
| `cargo test --workspace --locked --features files,agents,portal,gpui-kit/test-support` | 545 passed, 1 ignored doctest (139 library, 177 Agents, 221 Files, 6 Portal, 2 settings derive) | 529 passed, 1 ignored doctest (139 library, 177 Agents, 205 Files, 6 Portal, 2 settings derive) |
| `cargo check --workspace --locked --features files,agents,portal,gpui-kit/test-support --all-targets` | pass, 0 warnings | pass, 0 warnings |
| `cargo check --lib --no-default-features` | pass | pass |
| `cargo check --lib --no-default-features --features storage` | pass | pass |
| `cargo check --bin xdg-desktop-portal-gds --no-default-features --features portal` | pass | pass |

The lock fix was soaked separately because the original failure was
load-dependent: after the fix, twelve consecutive parallel library-suite runs on
`86d5c6a` produced eleven green runs with zero `Error::Busy` failures (the one
failure was the separate storage concurrency test below), while the baseline
failed four of twelve runs on the persistence test; independent 5-run and 6-run
series on the accepted lock-fix head also passed without `Busy`. The removed
Files tests account for the per-target count drop (six legacy-import tests and
two pre-`004` places migration tests); the library count drops by two
implementation-detail tests and gains two lock regression tests.

### Storage concurrency flake

`service::storage::tests::concurrent_first_open_and_writers_serialize_without_lost_updates`
failed intermittently under heavy parallel host load with a second-level SQLite
busy error at `BEGIN IMMEDIATE`. The start barrier synchronizes test entry only,
so one worker can still be applying migrations inside `Options::open` while
another writes; SQLite answers that conflict with an immediate `SQLITE_BUSY`
instead of invoking the busy handler. The test now retries the whole
`BEGIN IMMEDIATE`/insert/commit attempt on busy/locked errors within a bounded
30 s deadline, mirroring what an application client does after `busy_timeout`.
Production storage behavior is unchanged, and the test still runs four
concurrent first opens and four concurrent writers and asserts the lossless
40-row result. Migration apply/rollback/retry tests are untouched.

### Agents icon-cache concurrency flake

`entities::agent::ui::icon::disk::tests::concurrent_handles_only_observe_complete_replacements`
could exhaust its 10 s retry budget under full-suite load: each cache operation
holds the cache `.lock` across its atomic replacement, and four threads doing
write/read rounds inside a 177-test parallel binary occasionally pushed the
budget over, while the panic hid the underlying I/O error. The test's bounded
retry budget is widened to 60 s and the panic now prints the I/O error kind, so a
real non-transient failure stays distinguishable. The complete-replacement
atomicity assertions and all cache behavior are unchanged.

### Terminal shutdown job-state race

`service::terminal::session::tests::shutdown_cleans_foreground_background_and_stopped_job_groups_before_completion`
read `/proc/<pid>/stat` for each cleaned-up job and panicked when a job exited
between the proc lookup and the read (`ESRCH`). `process_state` now maps `ESRCH`
to `None` exactly like `NOT_FOUND`, which is an outcome the test already accepts;
every job still has to be gone or a zombie after termination. Nothing is ignored
or serialized, and no production code or coverage changed.

## Retained or deferred

- `bin/files/src/features/file_operations/recycle.rs` keeps
  `#[cfg_attr(not(target_os = "linux"), allow(dead_code))]`: the annotated fields
  are used by the Linux trash backend only, so the allowance is
  platform-specific, not dead code.
- No live desktop, Wayland/X11 visual, IME, macOS or Windows validation was
  performed by this review; the checks above are automated Linux checks.
