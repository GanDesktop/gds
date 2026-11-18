# Files migration to AntiWebUI

## Scope and outcome

Migrate GDS Files on `experiment/files-antiwebui` to AntiWebUI and UIM v2,
using the Wayland desktop integration in `GanDesktop/AntiWebUI`. The result
must be a real Files application using the existing domain models and services,
not a second demonstration executable or a GPUI compatibility layer.

Agents and Nixes are excluded from compilation during this experiment. Images
is outside the migration scope and must not enter the Files dependency graph.
The Portal remains a UI-independent consumer of the existing picker protocol.
X11 and accessibility implementation are outside this stage, as agreed.

Preserve Files' browsing, navigation, tabs, selection, list/grid presentation,
watching, file operations and undo, clipboard, picker, preferences, inspection,
remote locations, volumes, tags, and terminal integration. A framework gap must
be reported and resolved at its responsible layer; do not silently substitute
an empty page, no-op command, or disconnected control.

## Architecture decisions

- Replace the UI runtime directly. Retain the single root crate, capability
  features, FSD slice boundaries, configuration formats, and service APIs.
- Use UIM v2 from the current `gds-uim` release commit. Read the installed
  skill and the checkout's grammar and AntiWebUI example before writing UI.
- Use the versioned AntiWebUI fork, Wayland only. Record dependency revisions
  and required renderer integration in the existing upstream-patches document.
  Resolve Cargo dependencies centrally; do not vendor dependencies.
- Keep application I/O on the existing Tokio `Supervisor` and `Scope`.
  Deliver results to AntiWebUI owners with its native reactive primitives.
  Cancellation, window lifetime, and stale navigation results remain explicit.
- Use a concrete application-root `Gds` with typed accessors. Shared state lives
  for the shell root; UI task channels and cancellation scopes belong to the
  relevant window owner. Do not reproduce GPUI's context/entity API.
- Shared UI uses the existing anatomy: roots and named parts such as `Frame`,
  `SideBar`, `ContentPage`, headers, content, and actions. Structural builders
  start at `Default::default()` and implement `IntoView` / `Styled` as needed.
  Create reactive subtree views inside their owning callbacks.
- Model alternatives with enums containing their state data. Put operations
  on domain types and expose cohesive accessors. Avoid parallel tag/data fields,
  generic service locators, procedural helper collections, and giant callback
  dispatchers. Reuse existing pure domain logic instead of rewriting it.
- Large, cohesive replacements are acceptable. Do not keep duplicate runtime
  implementations or invent abstractions for future application migrations.

## Parallel work and file ownership

The lead writes this plan, coordinates interfaces, reviews changes, and returns
issues to their implementing agents. Product code is written by subagents.
The host currently permits four simultaneous agents including the lead, so
three implementation lanes run concurrently and are reassigned as work lands.

| Lane | Initial ownership | Deliverable |
| --- | --- | --- |
| Foundation | Root manifest/lock, build integration, `src/app*`, `src/appearance*`, `src/i18n*`, affected macro glue and packaging | Wayland dependency graph; application services, appearance, localization, task delivery and window lifecycle contracts |
| Shared UI | `src/ui*`, Files `widgets/`, component-facing asset adapters | Anatomical AntiWebUI components and Files widgets using UIM v2 |
| Files | `bin/files/src/` except `widgets/` | Application bootstrap, existing domain adapters, workspace, workflows, picker, clipboard and desktop requests |

Integration ownership refinement: after establishing the shared context, the
foundation lane also owns `bin/files/src/app.rs`, `app/`, and `main.rs`. The
Files lane concentrates on entities, features, shared adapters, and workspace
behavior. Foundation also owns the minimal completion/shutdown additions in
`service/task_manager/{execution,supervisor}.rs`; those remain UI-independent.
After the foundation compile, that lane takes `pages/workspace/advanced.rs`
and the `connect_server` feature for remote/device/tag integration and desktop
commands. Files retains local operations, navigation, workspace composition,
clipboard, and picker behavior; shared state changes are coordinated explicitly.
The shared UI lane subsequently owns `workspace/inspection.rs` and
`workspace/terminals.rs`, integrating details and terminal sessions with the
workspace, then takes the existing `workspace/e2e.rs` and native runner's runtime
adaptation after the application compiles. Foundation additionally owns
`workspace/menus.rs`, dependency pins, and the exclusive Cargo validation lane.

Agents publish their public interface decisions early and coordinate through
the lead. They may read all sources, but edit outside their lane only after
ownership is explicitly transferred. They do not reset other agents' work,
commit concurrently, spawn extra agents, or run overlapping Cargo builds.
The lead can split a completed lane into focused follow-up assignments.

## Construction sequence

1. Establish the native application context, task completion bridge, theme and
   localization accessors, and anatomical component contracts. Pin dependencies
   and isolate excluded application targets.
2. Migrate Files' model adapters and workspace to those contracts, retaining
   domain/service behavior. Build the browsing shell, list/grid surfaces, tabs,
   toolbar/sidebar, keyboard navigation, and asynchronous directory watching.
3. Connect file operations, clipboard, dialogs, picker/Portal parenting and
   activation, preferences, remote/volume/tag pages, inspection, and terminal.
4. Review the assembled implementation for actual feature wiring, ownership,
   idiomatic Rust, UIM v2, anatomical composition, and accidental scope growth.
   Fix substantive findings in the responsible lane.
5. Validate the converged implementation and update relevant current-state
   documentation and dependency pins. Record any environment-only validation
   limits accurately, separately from unfinished implementation.

## Validation policy

Do not add tests by default and do not run full suites during construction.
Use source/API inspection first; perform a focused compile once the dependent
pieces form a meaningful whole. Coordinate a single Cargo validation lane.

At integration, build the Files Wayland target and the independent Portal,
run relevant existing pure-domain/service tests, and run an isolated real
Wayland smoke workflow if the environment supports it. Check startup, folder
navigation, selection, view switching, rename/create/trash and undo, watcher
updates, and picker completion. Exercise clipboard and parenting where the
compositor/input environment permits. Adapt existing checks only where the
runtime change requires it; do not write tests that mirror the new structure.

## Review gates

- The Files dependency graph contains AntiWebUI and UIM v2, with no GPUI runtime.
- Existing service and persistence semantics remain intact.
- Controls invoke real behavior; failures are surfaced rather than swallowed.
- Native subscriptions/tasks end with their owner and ignore stale results.
- Shared components preserve anatomy and domain code uses stateful Rust types.
- Verification describes what actually ran, without claiming untested desktop
  behavior or disabled features as complete.

## UI fidelity repair

The initial migration was rejected. Its review checked compilation and individual
interactions but failed to preserve the existing interface. Passing those checks
does not establish visual or usability parity. GDS changes remain in the branch's
working tree, uncommitted.

Use the pre-migration source in `HEAD`, the visual-language documentation, and
the actual Wayland captures in `target/files-e2e/run-800d4fel` as the baseline.
Compare those with `target/files-e2e/run-l9kjz35v`. Match window size, preferences,
theme, directory contents, and UI state before assessing differences.

Observed regressions include replacement of breadcrumbs with a truncated plain
address field, relocation and crowding of folder filtering, oversized button
table headers, loss of the compact icon sidebar and tab anatomy, changed grid
spacing and icon treatment, and duplicated settings/navigation affordances.
Repair the original layout and interaction hierarchy rather than redesign it.

| Lane | Repair ownership |
| --- | --- |
| Files | Workspace chrome, breadcrumbs, sidebar/tabs, page actions, table/grid, focus and responsive behavior |
| Shared UI | Shared anatomical components, control sizing/materials/icons, Settings and inspection/terminal presentation |
| Foundation | Original-source/reference audit, appearance and initial window parity, menus and native screenshot validation; exclusive Cargo lane |

Each lane must inspect the before/after screenshots and the original source
before coding. Coordinate shared component contracts before changing callers.
Retain the migrated services and functional behavior. Do not add broad tests or
new product abstractions. The lead reviews screenshots and source and returns
specific failures to their owners.

Acceptance requires direct visual comparison of list and grid, expanded and
collapsed sidebar, settings, menus/dialogs, and constrained window layouts,
plus existing relevant input workflows after the presentation changes converge.
The reference screenshots are evidence of the original design, not a pixel-test
fixture. Verify navigation, readable filenames and addresses, reachable actions,
and focus behavior in the actual rendered result.

Reference details recovered from the original implementation:

- Initial window: `960 × 640`; the original GPUI window border reserves a
  `20 px` shadow inset on each edge, leaving a `920 × 600` visible frame in the
  archived capture. AntiWebUI currently has no such inset; compare equal visible
  content sizes separately from initial window allocation. The `1080 px`
  compact breakpoint preserves the user's expanded/sidebar and
  details preferences for wider windows. Compact is a presentation state.
- Shared geometry: `52 px` toolbar, `36 px` footer, `58/222 px` sidebar,
  `14 px` ordinary text, `32 px` action targets, and `18 px` table inset.
- Lists: `30 px` unobtrusive headings and `40 px` rows. Grid uses the original
  measured geometry, a `48 px` icon in a `100 px` artwork region, and two filename
  lines. Rendering, pointer hit testing, and keyboard navigation share geometry.
  The current pre-migration list tokens are `13/12/11 px` for names, metadata,
  and headings, with the user's typography scale applied. Timestamp cells retain
  exact-time hover text. The inherited text line height is `1.618034`.
- Settings replaces Files navigation/chrome, with one sidebar, aligned controls,
  reserved reset slots before values, and row separators. Its reference is
  `target/tags-cachyos/02-settings.png`; expanded Files and details are shown in
  `target/tags-cachyos/14-preview.png`.
  Details sits beside the complete browsing page, starting directly below the
  window toolbar. Its presence reduces the browsing header's available width.
- Footer task, selection, and clipboard controls retain their existing popovers.
  Long data remains available through full-value tooltips/copy; display truncation
  must use shaped text rather than arbitrary character counts.

## Repair implementation and evidence

The repaired interface restores the original workspace hierarchy, compact sidebar,
breadcrumbs, page actions, list/grid geometry, typography, button state, full-height
details, Settings navigation, and full-width terminal dock. Constrained layouts keep
tabs, locations, picker actions, and dialog actions reachable. Ordinary form fields
use the native styled text-field widget. The terminal retains its session while its
surface is hidden. Context menus belong to the stable window layer and retain their
target location and selection.

The lead inspected the rendered artifacts directly:

- `target/files-e2e/run-xk8fvf17`: equal-visible-size list/grid, Settings at ordinary
  and enlarged text sizes, compact Settings, corrected details, and actual Wayland
  presentation. Its initial compact terminal capture exposed issues corrected below.
- `target/files-e2e/run-xjx4z6kh`: native application/context popup scenes, long-name
  grid with enlarged text, `520 × 360` list with devices enabled, corrected terminal,
  and the scrolled private-key connection form with reachable actions. The visual
  scenario and real Wayland application both shut down successfully.
- `target/files-e2e/run-n0za9_q0` and `run-oqh96foq`: compact Open/Save picker
  controls; Save retains an outlined filename field and does not show Read-only.

These temporary visual scenarios are explicitly labelled separately from the
durable file-operation workflow. Their hooks have been removed. The final
[durable workflow and Wayland run](../target/files-e2e/run-kc76r2bm/passed.txt)
passes rename, undo/redo, folder creation, trash/restore, watching, grid, tab
reordering, type-to-search, IME, search navigation, and mouse history. Actual
Wayland startup, compositor presentation, and graceful shutdown also pass.
The final Files build/check, production and all-target strict Clippy, formatting,
and whitespace checks pass. The Portal also checks independently, and its dependency
tree remains free of the UI runtime, SQLite, terminal, and remote-file backends.
Its isolated check emits two manifest warnings for unused `serde`/`toml` build
dependencies when GUI build-script paths are disabled.
The matching original terminal reference is
`target/vm-e2e/run-20261116T162237-699191Z/cachyos/F19_terminal_writes_in_current_directory/final.png`.

AntiWebUI is now pinned to `6fcb4e63511e0ab5ca0b2ee73a21a3ba39d2eff5`, pushed
to the fork with its Cargo/Nix pins synchronized. Besides shaped text truncation,
Wayland size constraints and popup options, the repair fixes tooltip timer ownership
and reactive-value destruction outside the runtime borrow. Those two defects were
reproduced by switching pages and closing a window containing terminal sessions;
targeted checks in the existing native test modules pass. No X11 work was added.

## Earlier runtime migration evidence

The following evidence predates the UI fidelity repair. It records the runtime
migration milestone and does not validate the repaired interface or establish
parity with the original UI.

UIM v2 remains pinned to `b67c2a3bb6071b6a92c8f1d0aa739928397c9cb6`.
At this earlier milestone, AntiWebUI was pinned to
`08d8902e1d42ba96242c6713a081e8cdd5a5cd2d`, pushed to
the fork's `gds-desktop-integration` branch. Required patches preserve same-offer
clipboard reads and conditional replacement, foreign parenting and activation,
callback ownership, type-to-search/IME delegation, reduced motion, and native
select indicators. Relevant existing core, protocol, interaction, and animation
checks passed. The X11 backend is unchanged.

The Files dependency graph contains AntiWebUI, UIM v2, and one wgpu 30.0.1 Git
identity, without GPUI or gpui-kit. Portal builds independently without a GUI
framework. Obsolete Files GPUI UI implementations were removed; relevant pure
domain and service regression tests were retained or adapted to native owners.

Validation completed:

- Runnable Files build with zero warnings; strict production and all-target
  Clippy gates; final formatting and whitespace checks.
- Existing workspace tests and doctests: 196 library, 84 Files, 6 Portal, and
  3 macro tests passed. One existing real-Baloo integration test remains ignored.
- The [final native workflow and Wayland run](../target/files-e2e/run-l9kjz35v/passed.txt)
  passed. Native input/rendering covers rename, undo/redo, folder creation,
  trash/restore, filesystem watching, grid layout, tab dragging, immediate
  shortcuts after tab clicks, type-to-search, IME, search navigation, and mouse
  history. A separate real Wayland application launch verifies a rendered frame,
  compositor capture, opt-in logs, and graceful shutdown.
- Temporary component inspections verified Settings rendering; PTY input/output,
  session identity across hide/reopen, session creation/closure; and picker
  selection emitting one protocol result and one native close request. Those
  temporary hooks were removed. Their artifacts remain in
  `target/files-e2e/run-t3a96rpc` and `target/files-e2e/run-h3p9xzkx`.

Run `just dev files` to try the application, or `just test-files-native` to repeat
the durable workflow. Physical devices, authenticated remote servers, external
clipboard peers, and full desktop Portal integration require their real
environments. Accessibility-dependent VM workflows remain outside this stage.
