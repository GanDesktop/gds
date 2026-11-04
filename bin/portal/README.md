# GDS Portal

An independent desktop portal backend for GDS. It currently provides the native file chooser interface.

## Installation

```sh
nix profile add github:GanDesktop/gds#xdg-desktop-portal-gds
```

On NixOS, import `gds.nixosModules.gds-portal` and enable:

```nix
services.gds-portal.enable = true;
```

The package does not depend on Files. File chooser requests use `gds-files` from PATH, so install Files separately when using this interface. A different executable implementing the native chooser protocol can be selected with `--file-chooser`.
