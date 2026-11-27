# GDS Documentation

GDS (Gan Desktop Suit) is a suite of native desktop applications built around a unified visual language. Currently, **GDS Files** and **GDS Images** run on [`zgui`](https://github.com/GanDesktop/zgui/tree/gds-0.1.0) alongside the UI-independent **GDS Portal**; **Linux + Wayland** is the primary target, with **Linux + X11** as secondary support.

Documentation is organized into **Current State**, **Technical Decisions & Compromises**, and **Roadmap**:

## Applications & Installation

| Component | Role | Usage & Integration | Feature Reference |
| --- | --- | --- | --- |
| **GDS Files** | Native local & SFTP/SMB file manager and picker (`zgui`) | [User Guide](../bin/files/README.md) · [简体中文](../bin/files/docs/i18n/zh-CN/README.md) | [Files Overview](apps/files/README.md) |
| **GDS Portal** | `org.freedesktop.impl.portal.FileChooser` backend | [Portal README](../bin/portal/README.md) · [NixOS Portal Setup](apps/files/nixos.md#file-chooser-portal) | [File Picker & Protocol](apps/files/picker.md) |
| **GDS Agents** *(unmigrated source)* | Native session workspace for an external BAS daemon | [Agents README](../bin/agents/README.md) · [简体中文](../bin/agents/docs/i18n/zh-CN/README.md) | [Agents Overview](apps/agents/README.md) |
| **GDS Nixes** *(unmigrated source)* | Flake-based Home Manager configuration browser | [Nixes README](../bin/nixes/README.md) | [Nixes Overview](apps/nixes/README.md) |

### Distribution & Packaging Guides

- [Nix, NixOS, Home Manager & NUR](nixos.md) — flake inputs, NixOS/HM modules, and NUR release setup
- [Files Desktop Integration on NixOS](apps/files/nixos.md) — MIME defaults, `FileManager1`, Portal routing, and Niri rules
- [Fedora RPM Packages](apps/files/fedora.md) — building and installing native Fedora RPMs (`just rpm`)
- [Debian & Ubuntu DEB Packages](apps/files/debian.md) — building and installing native DEBs (`just deb`)
- [Arch Linux & AUR Recipe](apps/files/arch.md) — container builds (`just arch`) and `PKGBUILD` workflow

### Feature Deep Dives

- [Embedded Terminals](apps/terminal.md) — shared local PTY service, appearance sync, and Files/Agents docks
- [Network Locations](apps/files/network.md) — native SFTP and SMB browsing and transfers
- [Volumes & Sidebar](apps/files/devices-and-sidebar.md) — UDisks2 discovery, mount actions, and sidebar layout
- [Tags & URI Schemes](apps/files/tags.md) — `user.xdg.tags`, Baloo/KIO integration, and `location:` / `tags:` URIs
- [Desktop VM Workflows](development/vm-e2e.md) — pytest workflows for Files/Images across QEMU desktops, evidence and replay
- [Files Testing](apps/files/testing.md) — VM coverage, supplementary Weston runner and unit test boundaries

## Architecture & Development

- **[Developer Guide](development.md)** — repository layout, `just` commands, FSD slices, crate features, `view!` / Stylo / i18n conventions, and CI release pipeline
  - [Task Ownership & Shutdown](development/task-manager.md) — `Supervisor`, `Scope` cancellation vs completion, and `Gds` / `zgui` lifecycle
  - [Storage & Persistence](development/storage.md) — atomic config files (`persistence`) and Toasty SQLite (`service::storage`)
  - [Agents & BAS Protocol](development/agents-protocol.md) — dual ACP/management streams, frontend multiplexing, and workspace provider forms
  - [Desktop Test Virtual Machines](development/virtual-machines.md) — local libvirt guests (`just vm`) and `clean-base` package verification

## Visual Language & Settings

- **[Visual Language Overview](visual-language/README.md)** — 3-layer contract, layout tokens, responsive sizing, state rules, and icon vocabulary
  - [Themes & Materials](visual-language/theme.md) — System (Noctalia, DMS, KDE, GNOME), Built-in, and Custom TOML palettes, opacity, and hot-reload
  - [Shortcuts & Focus Scopes](visual-language/shortcuts.md) — suite bindings, `ShortcutBadge`, modifier keytips, and scope routing
  - [Component Composition](visual-language/composition.md) — UIM anatomy (`ContentPage`, `SettingRow`, `DragCard`, `DialogActions`, `About`)
  - [Sidebar Anatomy](visual-language/sidebar.md) — `SideBar` structure, 160 ms width transition, and spatial stability
  - [Focus Presentation](visual-language/focus.md) — 1 px keyboard contour and viewport clipping
  - [Motion](visual-language/motion.md) — reduced-motion policy and transition timings
- **[Settings Center](settings/README.md)** — per-app preference pages, immediate apply semantics, and [Derived Config Editors](settings/config-editor.md)

## Decisions & Roadmap

- **[Technical Decisions & Compromises](decisions/README.md)** — why active upstream forks, runtime boundaries, PTY workers, and external CLI integrations work the way they do, and when to retire them
- **[Roadmap](roadmap/README.md)** — planned milestones across Files, distribution channels, and companion app migrations
- **[Changelog](../CHANGELOG.md)** — release history by version
