# Settings Center

Files and Agents share the Settings Center visual layout, form rows (`SettingRow`), numeric sliders (`NumberControl`), and derived editors (`#[derive(gds::ConfigEditor)]`), while keeping **strictly separate per-application configuration files and preference models**.

## 1. Navigation & Window Hosting

- **Entry & return**: Open via the top-left Application Menu or `Ctrl+,` (no bottom-bar gear or single-topic drawer). Settings replaces the workspace view inside the current window (`ui::page::stack`), placing an `ArrowLeft` back button in the top-left slot. Pressing `ArrowLeft` or `Esc` returns to the live workspace with tabs, selection, scroll offsets, unsent drafts, and running PTY terminals intact.
- **Keyboard cycling**: `Ctrl+Tab` and `Ctrl+Shift+Tab` cycle settings categories in sidebar order (with `Tab` / `Shift+Tab` hints shown on adjacent `NavItem`s while `Ctrl` is held) without touching underlying file tabs.
- **Read-only shortcuts live in Help (`F1`)**: Settings contains only editable preferences. Keyboard bindings are presented in the dedicated Help overlay (`F1` or Application Menu → Help) using the shared `ShortcutBadge`.

## 2. Application Pages & Scope

### GDS Files (`$XDG_CONFIG_HOME/gds/app/files/settings.toml`)

| Category | Preferences |
| --- | --- |
| **General** | Interface language (searchable Fluent catalog) and timestamp format (`Relative` vs `Absolute`) |
| **Appearance** | Theme source (`System` / `Built-in` / `Custom`), light/dark mode, built-in/custom palette selector, UI font family (`TextSystem::all_font_names`, marking missing saved fonts as *Not installed* without overwriting them), base font size (`12–24 px`), main & sidebar background opacities, rounded window corners, and interface animations |
| **Files** | Sidebar volume list visibility and type-to-filter / browsing preferences *(hidden-file visibility is toggled per-view via `Ctrl+H` or the view menu rather than Settings)* |
| **Terminal** | Enable/disable embedded terminal dock, shell executable, per-line arguments / login toggle, monospace font, font size, and scrollback limit (disabling hides UI entry points while keeping existing sessions alive) |
| **Extensions** | Baloo/KIO file tags toggle and optional `balooctl` / `kioclient` binary overrides |
| **Performance** | Progressive directory loading, metadata probing, preview cache, and transfer tuning parameters (read-only behind an explicit risk confirmation gate) |

### GDS Agents (`$XDG_CONFIG_HOME/gds/app/agents/settings.json`)

Provides local **Appearance** (matching suite theme, font, opacity, rounded corners, and interface animations), **Session / Input**, and **Terminal** pages alongside **BAS Connections**, **Agent Installation**, and **Service Management** (which explicitly displays the target BAS server address and uses revision-checked RPCs so remote daemon mutations are never confused with local UI preferences).

## 3. Controls & Immediate-Apply Semantics

- **Standardized controls**: Booleans use `ToggleSwitch` (`44 × 24 px`), enumerations use `36 px` dropdowns / `SearchChoiceField` (never ad-hoc button strips), and numbers/percentages use value-displaying sliders stepped at component precision (preventing float noise like `0.6999999` from triggering spurious writes). Settings content caps at `max-width: 880 px` on wide displays, and `SettingRow` wraps controls onto a second line when window width falls below `600 px`.
- **Per-field reset**: Every editable row compares its value against `Preferences::default()`. Divergent fields reveal a compact reset icon button in `SettingReset` without shifting layout; resetting restores only that single field.
- **Immediate apply, background save**: Real value changes emit `Event::Apply` immediately and enqueue an atomic disk write—there is no Save button or bottom confirmation bar. Programmatic sync (`set_preferences` / `set_value`) never re-emits change events.
- **Distinct failure feedback**: Validation or runtime-apply failure rolls back to the previous valid preference and notifies the user. Disk-write failure (including read-only or declaratively managed symlink configs) keeps the applied in-memory state active and reports *"Applied, but could not be saved"* via the top-right notification overlay.

## 4. Code Organization

```text
src/ui/forms/settings.rs + settings/      # SettingRow, NumberControl, typed form state
macros/gds-settings-derive/               # #[derive(gds::ConfigEditor)] macro crate
bin/files/src/widgets/settings/           # Files Settings Center & pages/
bin/files/src/entities/preferences/       # Files TOML persistence & PreferencesExt
bin/agents/src/widgets/settings/          # Agents Settings Center, search & BAS management pages
```

See [Derived Configuration Editors](config-editor.md) for generating typed settings forms with `#[derive(gds::ConfigEditor)]`.
