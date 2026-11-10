# GDS documentation

GDS is a native desktop application suite built with GPUI. Linux with Wayland is the primary support target; X11 is secondary. Windows and macOS support is not established.

Build and installation definitions live in `packaging/nix/` and
`packaging/fedora/`. The root `flake.nix`, `flake.lock` and `shell.nix` remain the
Nix entry points; generated packages and build caches stay under `target/`.

| Component | Purpose | Documentation |
| --- | --- | --- |
| GDS Files | Local and remote file browsing, volumes and file selection | [Files](apps/files/README.md) |
| GDS Agents | Session workspace for an independently running Bas service | [Agents](apps/agents/README.md) |
| GDS Portal | Desktop FileChooser backend | [Desktop integration](apps/files/nixos.md#file-chooser-portal) |

- [Fedora RPM installation](apps/files/fedora.md)
- [NixOS installation](nixos.md)
- [File picker CLI and protocol](apps/files/picker.md)
- [Network locations](apps/files/network.md)
- [Volumes and sidebar](apps/files/devices-and-sidebar.md)
- [Embedded terminals](apps/terminal.md)

Developer references:

- [Development and architecture](development.md)
- [Task ownership and application shutdown](development/task-manager.md)
- [Visual language](vistual-language/README.md), [themes](vistual-language/theme.md) and [shortcuts](vistual-language/shortcuts.md)
- [Settings](settings/README.md) and [derived configuration editors](settings/config-editor.md)
- [Component composition](vistual-language/composition.md), [sidebar anatomy](vistual-language/sidebar.md) and [About](components/about.md)
