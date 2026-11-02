# Terminal appearance contract

The appearance palette is the sole source of terminal defaults: foreground,
background, cursor and cursor text, selection and selected text, and eight normal
plus eight bright ANSI colors. Terminal rendering converts these roles to the
emulator's color table; it does not derive ANSI colors from UI status roles.

DMS imports `dank16.color0` through `color15`, selecting each entry's `light` or
`dark` value using the same resolved mode as the Material palette. Explicit mode
preferences override the exported active mode; System follows it. `default` is
never substituted. A missing `dank16` supports older exports; a present but
incomplete, null, wrongly typed or invalid requested scheme rejects the entire
update, retaining the last valid installed appearance.

Built-in themes, Qt/KDE imports and legacy DMS exports share the maintained
light/dark ANSI fallback in `src/appearance/theme/terminal.rs`. Their terminal
foreground/background and selection pairs follow the provider's resolved UI
roles; cursor text/background form the inverse default text pair. ANSI ink is
independent of error/success/warning colors. No standalone Alacritty installation,
configuration file or user-specific fixture is required.

The UI instance observes the installed appearance snapshot, updating a shared
service palette created before process launch. Live, hidden and still-starting
sessions therefore answer color queries using the latest defaults without a
paint pass or shell restart. The service knows only RGB values, not GPUI or
appearance types. Existing appearance hot reload remains the only file watcher.

Application truecolor and emulator-owned OSC overrides take precedence over the
theme. Resetting an override reveals the latest theme, not the launch theme.
Indices 16–255 retain the standard xterm cube and gray ramp. Default background
opacity comes from the content material and is painted once; explicit cell,
selection and cursor backgrounds remain opaque.

Automated provider, rendering and real-PTY tests cover these contracts. Desktop
visual acceptance (especially transparency, font fallback and IME on Wayland)
remains a separate manual check; these tests do not establish Windows/macOS
support.
