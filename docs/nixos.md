# Install GDS with Nix

GDS exports packages for `x86_64-linux` and `aarch64-linux`. The graphical suite contains Files and Agents. GDS Portal is packaged and enabled separately.

| Component | Package | NixOS module | Home Manager module | Enable option |
| --- | --- | --- | --- | --- |
| GDS Files | `gds-files` | `nixosModules.gds-files` | `homeManagerModules.gds-files` | `programs.gds-files.enable` |
| GDS Agents | `gds-agents` | `nixosModules.gds-agents` | `homeManagerModules.gds-agents` | `programs.gds-agents.enable` |
| GDS Portal | `xdg-desktop-portal-gds` | `nixosModules.gds-portal` | `homeManagerModules.gds-portal` | `services.gds-portal.enable` |

## Build, install and run

From the repository:

```sh
nix build .#gds
nix build .#gds-files
nix build .#gds-agents
nix build .#xdg-desktop-portal-gds

nix profile add .#gds
nix run .#gds-files -- "$HOME"
nix run .#gds-agents -- --compact
```

`nix build` uses the graphical suite by default. Install either the suite or individual graphical packages in a profile. `just install`, `just install files` and `just install agents` provide equivalent profile installation commands. Packaged executables include their runtime wrappers and do not require a development shell.

## Home Manager

With `inputs.gds.url = "github:GanDesktop/gds"` and `gds` in scope, add to your
Home Manager configuration:

```nix
imports = [ gds.homeManagerModules.default ];
programs.gds-files = {
  enable = true;
  fileManager1.enable = true;
};
programs.gds-agents.enable = true;
services.gds-portal.enable = true;
```

Enable only the components you need. The individual modules are listed above;
`homeManagerModules.default` imports all three without enabling them.

- Files becomes the default folder application. Set
  `programs.gds-files.defaultFileManager = false` to leave directory associations
  unchanged. `fileManager1.enable` is off by default; enable it to handle
  requests such as "Show in Folder" when Files is not running.
- Portal installs the GDS backend and routes FileChooser to it. Other backends
  and interface routing belong in your desktop configuration. Files is installed
  separately. `services.gds-portal.desktops` defaults to `[ "niri" ]` alongside
  the common configuration; set it for your desktop when needed.
- Override packages with `programs.gds-files.package`,
  `programs.gds-agents.package` or `services.gds-portal.package`.
- Removable-drive support requires UDisks2 and a polkit agent. The Home Manager
  module does not enable system services.

When using Portal with NixOS-integrated Home Manager and
`home-manager.useUserPackages = true`, add this to the NixOS configuration:

```nix
environment.pathsToLink = [ "/share/applications" "/share/xdg-desktop-portal" ];
```

## System flake

```nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    gds.url = "github:GanDesktop/gds";
  };

  outputs = { nixpkgs, gds, ... }: {
    nixosConfigurations.myhost = nixpkgs.lib.nixosSystem {
      system = "x86_64-linux";
      modules = [
        ./configuration.nix
        gds.nixosModules.default
        {
          programs.gds-files.enable = true;
          programs.gds-agents.enable = true;
          services.gds-portal.enable = true;
        }
      ];
    };
  };
}
```

The default module registers all options without enabling components. Import individual modules when only some components are needed. Packages use the project's locked nixpkgs; setting `gds.inputs.nixpkgs.follows` is unnecessary. Override a component with its `programs.<application>.package` or `services.gds-portal.package` option.

Run in the system configuration repository, replacing `myhost`:

```sh
nix flake update gds
sudo nixos-rebuild switch --flake .#myhost
```

For a local checkout, use `git+file:///absolute/path/to/gds`. Git flakes include tracked files, so required new source and packaging files must be added to Git before building through a flake.

## Integration

- [Files desktop integration](apps/files/nixos.md) covers directory associations, FileManager1, volumes and FileChooser routing.
- [Agents](apps/agents/README.md) is a frontend; its module does not start the independent Bas service.
- Portal uses `gds-files` from PATH for FileChooser requests. Install Files separately or supply another handler with `--file-chooser`.

Graphical packages, NixOS modules (`module.nix`) and Home Manager modules (`home-manager.nix`) live in `packaging/nix/apps/<application>/`, sharing `packaging/nix/package.nix`. The flake discovers those applications and reads target features from Cargo.toml. Portal definitions live in `packaging/nix/portal/`; its module uses the shared `xdg.portal` interface for both NixOS and Home Manager and does not depend on a graphical application package.
