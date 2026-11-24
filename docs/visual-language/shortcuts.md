# Shortcuts, Keycaps & Focus Scopes

GDS shares one shortcut vocabulary, keycap component (`ShortcutBadge`), hover layout (`ShortcutHint`), and hold-to-reveal overlay (`keytips`) across the suite. Every shortcut is defined once as `(semantic action ID, effective binding, scope)` and consumed identically by key dispatch, tooltips, menu trailing columns, and the read-only Help view (`F1`).

## 1. Suite Core Bindings

Mnemonic letters follow English action semantics and remain invariant across UI languages.

| Action ID | Action | Default Binding | Scope |
| --- | --- | --- | --- |
| `settings.open` | Open / focus Settings Center | <kbd>Ctrl</kbd> <kbd>,</kbd> | Explicit window-level (works inside inputs & terminals) |
| `panel.terminal` | Show / hide terminal dock | <kbd>Ctrl</kbd> <kbd>`</kbd> | Explicit window-level |
| `tab.new` | New tab | <kbd>Ctrl</kbd> <kbd>N</kbd> | Active file workspace or focused terminal region |
| `tab.close` | Close active tab | <kbd>Ctrl</kbd> <kbd>W</kbd> | Active file workspace or focused terminal region |
| `panel.sidebar` | Expand / collapse sidebar | <kbd>Ctrl</kbd> <kbd>B</kbd> | Workspace (passed through to PTY when terminal input is focused) |
| Help | Open keyboard shortcuts reference | <kbd>F1</kbd> | Workspace / Settings page stack |

### Region Access & Notifications (`Alt`)

`Alt` serves as the suite's non-destructive region-focus modifier—moving focus without opening items or altering selection (`Enter` activates once focused; `Esc` returns to main content):

| Action | Default Binding | Behavior |
| --- | --- | --- |
| Focus Locations / Volumes / Tabs | `Alt+L` / `Alt+V` / `Alt+T` | Moves keyboard cursor to the sidebar group (`Alt+V` reveals hidden volumes for the session) |
| Focus Main Content | `Alt+Space` | Focuses central table/grid or conversation area |
| Focus Notifications | `Alt+N` | Focuses newest notification; pressing again or `Esc` restores prior focus |
| Dismiss Current / All Notifications | `Alt+X` / `Alt+Shift+X` | Hides notification(s) without cancelling background tasks |

### Modal Dialogs (`Dialog` Scope)

Registered exclusively while a modal dialog is open, intercepting keys before background views or text inputs:

- **Confirm primary action**: <kbd>Alt</kbd> <kbd>A</kbd> (alongside <kbd>Enter</kbd>)
- **Cancel / close**: <kbd>Alt</kbd> <kbd>C</kbd> (alongside <kbd>Esc</kbd>)

## 2. Files Default Bindings

| Action | Binding | Notes |
| --- | --- | --- |
| New window | `Ctrl+Alt+N` | Independent browser window |
| Application menu | `Alt+M` (compat `F10`) | `Shift+F10` opens context menu for current selection |
| Edit path / Filter directory | `Ctrl+L` / `Ctrl+F` (`F3`) | Preserves native text-input editing while focused |
| Back / Forward / Parent | `Alt+Left` / `Alt+Right` / `Alt+Up` | Per-tab location history |
| Switch / Jump tabs | `Ctrl+Tab`, `Ctrl+Shift+Tab`, `Ctrl+1..9` | Cycles within active tab group or jumps to tab 1–9 |
| Open folder in new tab | `Ctrl+Enter` | Opens selected folder in a new file tab |
| Toggle hidden / Grid-List / Details | `Ctrl+H` / `Ctrl+G` / `Ctrl+P` | `Ctrl+P` applies only in file workspaces |
| Sort menu / Column config | `Alt+Shift+S` / `Alt+Shift+V` | Opens sort menu or column draft dialog |
| Bookmark / Unbookmark folder | `Ctrl+D` | Toggles favorite state for current loaded folder |
| Select all / Clear selection | `Ctrl+A` / `Ctrl+Shift+A` | `PgUp` / `PgDn` / `Home` / `End` / `Shift+Arrows` navigate & extend |
| New folder / Rename / Refresh | `Ctrl+Shift+N` / `F2` / `Ctrl+R` (`F5`) | `F2` opens single-item rename dialog |
| Cut / Copy / Paste / Copy path | `Ctrl+X` / `Ctrl+C` / `Ctrl+V` / `Ctrl+Shift+C` | `Esc` cancels active drag first, then pending cut state |
| Move to Trash / Delete permanently | `Delete` / `Shift+Delete` | `Shift+Delete` requires destructive confirmation |
| Trash: Restore / Delete / Empty | `Alt+R` / `Alt+D` / `Alt+Shift+D` | Active only on `trash:///` |
| Clear Recent / Mount / Unmount | `Alt+C` / `Ctrl+M` / `Ctrl+Shift+M` | Scoped to Recent page and Volumes view/directory |
| Terminal: Copy / Paste / Return focus | `Ctrl+Shift+C` / `Ctrl+Shift+V` / `F6` | Dragging files into terminal pastes shell-quoted paths without `Enter` |

## 3. `ShortcutBadge`, Tooltips & Modifier Keytips

- **Keycaps (`ShortcutBadge`)**: Each physical key renders as an independent `22 px`-tall (`4 px` radius, `4 px` gap) keycap using `--gds-surface`, `1 px` `--gds-border`, and `--gds-muted-foreground` (`--gds-text-xs`)—never a single `"Ctrl+N"` pill and never separated by literal `+` signs. Modifier order is strictly `Ctrl → Alt → Shift → Super → Key` (using `Ctrl`/`Alt`/`Shift` labels on Linux). Never wrap a key combination across lines.
- **Button tooltips (`ShortcutHint`)**: Appear after `400 ms` pointer hover or immediately on keyboard focus (arrowless `--gds-panel` surface), placing the short Fluent action label on the left and `ShortcutBadge` on the right. Never fabricate bindings for unbound actions, and **only display a shortcut when the button's target matches the keyboard dispatch target** (e.g., an inactive tab's close button omits `Ctrl+W`).
- **Hold-to-reveal keytips (`keytips`)**: Holding `Ctrl` alone or `Alt` alone immediately reveals compact `--gds-panel` suffix badges (`--gds-text-2xs`) on eligible controls in the active focus scope (`Ctrl+R` → `R`, `Ctrl+Shift+Tab` → `Shift+Tab`, `Alt+A` → `A`; `Ctrl+Alt` / `AltGr` suppresses hints):
  - Icon buttons & collapsed `NavItem`s show a corner overlay badge (`BottomLeadingCorner` when a top-right `CornerAction` is revealed); expanded `NavItem`s place hints at the row tail; text buttons expand inline over `140 ms` (`ease-out cubic`). Floating badges run collision nudging and automatically cull hints occluded by active top-right notification cards.
  - Releasing the modifier or losing window activation clears all keytips immediately.

## 4. Focus Scope Routing

| Active Focus Scope | Dispatch Contract |
| --- | --- |
| **File workspace / tab list** | `Ctrl+N` / `Ctrl+W` open/close file tabs; `Ctrl+P` toggles details. |
| **Terminal dock (tab rail & header)** | `Ctrl+N` / `Ctrl+W` open/close terminal instances without touching file tabs. |
| **Terminal PTY input** | `Ctrl+N` / `Ctrl+W`, `Ctrl+Shift+C/V`, `F6`, `Ctrl+,`, and `Ctrl+\`` are handled by GDS; `Ctrl+C/D/B/L/R`, `Esc`, arrows, and `Tab`/`Shift+Tab` go straight to the PTY (bypassing window `Tab` focus traversal). |
| **Text inputs (path, filter, forms)** | Preserve native cursor/selection/clipboard editing; unhandled window actions propagate outward. |
| **Settings & Help (`ui::page::stack`)** | `Ctrl+Tab` / `Ctrl+Shift+Tab` cycle settings categories; `Esc`, `Alt+Left`, `Ctrl+W`, or the top-left back arrow pop one overlay level and restore prior focus without closing underlying file tabs. |
| **Modal dialog** | Captures `Alt+A` / `Enter` and `Alt+C` / `Esc` exclusively; blocks background workspace shortcuts and keytips. |

Collapsing a region that currently holds focus (such as hiding the terminal dock) returns focus to its trigger control or the active workspace rather than stranding focus in a hidden view.
