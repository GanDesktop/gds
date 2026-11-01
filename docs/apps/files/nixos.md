# Files desktop integration on NixOS

Install Files with `programs.gds-files.enable = true` after importing its module. See [suite installation](../../nixos.md) for the complete flake example.

Directory MIME association, FileManager1 activation and FileChooser Portal routing are separate settings:

```nix
{
  imports = [
    inputs.gds.nixosModules.gds-files
    inputs.gds.nixosModules.gds-portal
  ];

  programs.gds-files = {
    enable = true;
    defaultFileManager = true;
    fileManager1.enable = true;
  };
  services.gds-portal = {
    enable = true;
    desktops = [ "niri" ];
  };
}
```

The Files module enables UDisks2 by default. Interactive device authorization also needs the session's polkit agent. Installing only the package requires enabling UDisks2 separately. Missing device services do not prevent ordinary folder browsing. Inserting a device does not automatically open a window or mount it.

## Default folder application

The package installs `io.github.GanDesktop.GdsFiles.desktop`, with `gds-files -- %f` as its local-path entry. `defaultFileManager` defaults to true and associates only `inode/directory`.

User MIME settings take priority over system settings. With Home Manager:

```nix
xdg.mimeApps = {
  enable = true;
  defaultApplications."inode/directory" = [ "io.github.GanDesktop.GdsFiles.desktop" ];
  associations.added."inode/directory" = [ "io.github.GanDesktop.GdsFiles.desktop" ];
};
```

Alternatively, configure and check the association in a graphical session:

```sh
xdg-mime default io.github.GanDesktop.GdsFiles.desktop inode/directory
xdg-mime query default inode/directory
xdg-open "$HOME"
```

## FileManager1

`programs.gds-files.fileManager1.enable` defaults to false. Enabling it installs D-Bus activation for `org.freedesktop.FileManager1` using `gds-files --dbus-service`.

Files supports `ShowFolders`, `ShowItems` and `ShowItemProperties`. Ordinary launches reuse the file-manager instance and existing directory tabs; pickers run independently and do not own the bus name. Files reports an existing provider instead of taking its name.

Requests accept local `file://` URIs. Remote URIs return an explicit argument error. Revealing several items in one directory combines their selection; different directories open separate tabs. Property requests open a native properties window. Activation identifiers are passed to the platform's Wayland/X11 startup mechanism.

```sh
busctl --user status org.freedesktop.FileManager1
busctl --user introspect org.freedesktop.FileManager1 /org/freedesktop/FileManager1
```

## File chooser Portal

GDS Portal is the independent `xdg-desktop-portal-gds` executable. Its module registers D-Bus activation and selects `gds` for `org.freedesktop.impl.portal.FileChooser`. The package does not depend on Files: its default handler is `gds-files` from PATH. A handler implementing the native protocol can be selected with `--file-chooser /path/to/executable`.

The module also installs GTK for other desktop interfaces and selects it for AppChooser. This supports Files' “Open with…” flow without routing FileChooser through GTK.

| Request | Behavior |
| --- | --- |
| Single/multiple files or directories | Native Files picker |
| SaveFile | Filename input and overwrite confirmation |
| SaveFiles | Directory choice and unique names for conflicts, preserving request order |
| Filename/MIME filters and choices | Passed through the native request protocol |
| User cancellation or caller Close | Cancels and reaps the corresponding picker |
| Invalid request/result or launch failure | Error response with stderr diagnostics |

Requests carry titles, button labels/mnemonics, initial locations, filters, choices, external parent handles and modality through stdin JSON. Unix paths preserve raw bytes. Results use local `file://` URIs, including non-UTF-8 names. Remote files must be downloaded first. See [picker protocol](picker.md).

The GPUI platform patches support Wayland xdg-foreign-v2 parent relationships and xdg-dialog hints, plus X11 WM_TRANSIENT_FOR and modal hints. Modality defaults to true. If a parent is absent, invalid or unsupported, the picker remains usable independently and reports why the relationship could not be established. Desktop adoption of these hints depends on the session.

Only applications using the desktop Portal follow this routing; applications drawing their own chooser retain it. Existing system or user Portal preferences may override module defaults. `services.gds-portal.desktops` selects additional desktop-specific configurations; the common configuration is always included. Other interfaces such as screen capture keep their configured providers.

## Picker window rules

The picker uses `io.github.GanDesktop.GdsFiles.Picker`. For a floating picker in Niri:

```kdl
window-rule {
    match app-id=r#"^io\.github\.GanDesktop\.GdsFiles\.Picker$"#
    open-floating true
}
```

The package also provides this rule at `share/gds-files/niri.kdl`. Ordinary browser windows retain their own identity and layout.

## Troubleshooting

```sh
systemctl --user status xdg-desktop-portal.service
systemctl --user status xdg-desktop-portal-gds.service
journalctl --user -b -u xdg-desktop-portal -u xdg-desktop-portal-gds
```

The GDS service activates on demand, so it may be inactive before the first request. After changing session configuration, log in again or update the activation environment from a graphical-session terminal:

```sh
dbus-update-activation-environment --systemd WAYLAND_DISPLAY DISPLAY XDG_CURRENT_DESKTOP
systemctl --user restart xdg-desktop-portal-gds.service xdg-desktop-portal.service
```

Check `~/.config/xdg-desktop-portal/*portals.conf` for user overrides. The FileChooser backend entry is `org.freedesktop.impl.portal.FileChooser=gds`; writing a picker CLI command there does not register a Portal backend.

Manual picker check:

```sh
gds-files --pick-file "$HOME" --filter 'Text:*.txt,*.md'
```

Build and protocol checks:

```sh
nix build .#gds-files
nix build .#xdg-desktop-portal-gds
nix flake check
```

The package build skips a metadata test requiring setuid/setgid changes prohibited in the build sandbox. The independent Portal D-Bus tests remain enabled.
