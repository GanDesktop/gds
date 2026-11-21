# Companion Applications & Platform Roadmap

## 1. GDS Nixes: Type-Aware Editing Milestone

GDS Nixes is currently a read-only Home Manager flake browser ([Nixes Overview](../apps/nixes/README.md)). Because arbitrary hand-written Nix expressions cannot be safely rewritten from evaluated values alone, the editing milestone will separate four explicit stages:

1. **Type-aware draft controls**: Editable switches, scalar inputs, list editors, and structured submodule forms with schema validation.
2. **Application-owned override module**: Persist user edits into an explicit, declarative override module imported by the user's `flake.nix` rather than mutating arbitrary existing `.nix` files.
3. **Isolated evaluation preview**: Re-evaluate drafts on Tokio with diff and error reporting before committing changes.
4. **Explicit generation switch**: User-triggered build and activation, plus optional `nix-darwin` target discovery and persisted app preferences.

## 2. Suite Keyboard Shortcut Remapping

Shortcuts across Files and Agents currently use a single data-driven registry (`Shortcut` / `ShortcutBadge` / `keytips`) presented as a read-only reference in Help (`F1`). A future milestone may add per-application keybinding customization and conflict detection in Settings without altering the single-source dispatch contract.

## 3. End-to-End Test Expansion

- Extend the [desktop VM workflows](../development/vm-e2e.md) with `--portal-request` roundtrips, multi-window `FileManager1` D-Bus activation, column editor persistence and the remaining desktop/service scenarios. CLI file picking and location view recovery already have VM workflows.
- Extend the shared VM framework to GDS Agents, covering local mock-BAS stream reconnection and compact/full workspace transitions.

## 4. Companion App Migrations & Secondary Operating Systems

- **Companion Apps**: Migrate `gds-agents` and `gds-nixes` from legacy GPUI sources onto `zgui` and restore their `[[bin]]` targets.
- **macOS & Windows**: Establish native PTY process-group management, live OS reduced-motion change notifications, multi-MIME file clipboard offers, platform volume backends, and desktop visual/IME acceptance testing.
