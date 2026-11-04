# Install GDS on NixOS

GDS exports packages for `x86_64-linux` and `aarch64-linux`. The graphical suite contains Files and Agents. GDS Portal is packaged and enabled separately.

| Component | Package | NixOS module | Enable option |
| --- | --- | --- | --- |
| GDS Files | `gds-files` | `nixosModules.gds-files` | `programs.gds-files.enable` |
| GDS Agents | `gds-agents` | `nixosModules.gds-agents` | `programs.gds-agents.enable` |
| GDS Portal | `xdg-desktop-portal-gds` | `nixosModules.gds-portal` | `services.gds-portal.enable` |

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

Graphical packages and modules live in `packaging/nix/apps/<application>/`, sharing `packaging/nix/package.nix`. The flake discovers those applications and reads target features from Cargo.toml. Portal definitions live in `packaging/nix/portal/` and do not depend on a graphical application package.
