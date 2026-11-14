# Install GDS with Nix

GDS exports `gds-files` and `xdg-desktop-portal-gds` for `x86_64-linux` and `aarch64-linux`.

| Component | Package | NixOS module | Home Manager module | Enable option |
| --- | --- | --- | --- | --- |
| GDS Files | `gds-files` | `nixosModules.gds-files` | `homeManagerModules.gds-files` | `programs.gds-files.enable` |
| GDS Portal | `xdg-desktop-portal-gds` | `nixosModules.gds-portal` | `homeManagerModules.gds-portal` | `services.gds-portal.enable` |

## Build, install and run

From the repository:

```sh
nix build .#gds-files
nix build .#xdg-desktop-portal-gds

nix profile add .#gds-files
nix run .#gds-files -- "$HOME"
```

`nix build` and `nix run` default to `gds-files`. Install either individual package in a profile; `just install` installs `gds-files` and `just install portal` installs `xdg-desktop-portal-gds`. Packaged executables include their library wrappers; optional runtime helpers such as `xdg-terminal-exec`, `xdg-utils`, and `ffmpeg` are resolved from your environment `PATH` when needed.

## Home Manager

With `inputs.gds.url = "github:GanDesktop/gds"` and `gds` in scope, add to your
Home Manager configuration:

```nix
imports = [ gds.homeManagerModules.default ];
programs.gds-files = {
  enable = true;
  fileManager1.enable = true;
};
services.gds-portal.enable = true;
```

Enable only the components you need. The individual modules are listed above;
`homeManagerModules.default` imports both without enabling them.

- Files becomes the default folder application. Set
  `programs.gds-files.defaultFileManager = false` to leave directory associations
  unchanged. `fileManager1.enable` is off by default; enable it to handle
  requests such as "Show in Folder" when Files is not running.
- Portal installs the GDS backend and routes FileChooser to it. Other backends
  and interface routing belong in your desktop configuration. Files is installed
  separately. `services.gds-portal.desktops` defaults to `[ "niri" ]` alongside
  the common configuration; set it for your desktop when needed.
- Override packages with `programs.gds-files.package`
  or `services.gds-portal.package`.
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
          services.gds-portal.enable = true;
        }
      ];
    };
  };
}
```

The default module registers all options without enabling components. Import individual modules when only some components are needed. Packages use the project's locked nixpkgs; setting `gds.inputs.nixpkgs.follows` is unnecessary. Override a component with its `programs.gds-files.package` or `services.gds-portal.package` option.

Run in the system configuration repository, replacing `myhost`:

```sh
nix flake update gds
sudo nixos-rebuild switch --flake .#myhost
```

For a local checkout, use `git+file:///absolute/path/to/gds`. Git flakes include tracked files, so required new source and packaging files must be added to Git before building through a flake.

## Integration

- [Files desktop integration](apps/files/nixos.md) covers directory associations, FileManager1, volumes and FileChooser routing.
- Portal uses `gds-files` from PATH for FileChooser requests. Install Files separately or supply another handler with `--file-chooser`.

Application packages, NixOS modules (`module.nix`) and Home Manager modules (`home-manager.nix`) live in `packaging/nix/apps/<application>/`, sharing `packaging/nix/package.nix`. Portal definitions live in `packaging/nix/portal/`; its module uses the shared `xdg.portal` interface for both NixOS and Home Manager and does not depend on a graphical application package.

## NUR releases

`packaging/nix/nur.nix` exports `gds-files` and `xdg-desktop-portal-gds`, using
the caller's `pkgs`. The flake shares this package set, while retaining its
pinned nixpkgs and module exports. NUR does not provide a binary cache; users
may need to compile these packages.

NUR registration is a separate, one-time step. The presence of this entry point
does not mean that GDS has already been accepted into NUR. After registration
under `gandesktop`, users with the NUR overlay configured can install:

```nix
environment.systemPackages = [ pkgs.nur.repos.gandesktop.gds-files ];
```

Modules remain available through the GDS flake. To use a NUR-built package with
an imported module, set `programs.gds-files.package` (or the equivalent Portal
package option) to the corresponding NUR package.

### Publication and first registration

The release workflow checks NUR's restricted evaluation and builds `gds-files`
plus Portal on `x86_64-linux` against `nixpkgs-unstable`. After those checks
and native release uploads succeed, it advances the `nur` branch to the release
tag's commit. This push uses the workflow's built-in `GITHUB_TOKEN` with
`contents: write`; repository rules must permit that branch update. It never
force-pushes, and an older release cannot rewind the branch.

NUR must track `nur`, not `main`, so development commits and prereleases do not
become NUR releases. The existing flake also exports `aarch64-linux`; the release
workflow's NUR build currently covers x86_64 only.

For first registration:

1. Publish a stable GitHub Release containing the NUR entry point and workflow,
   and wait for the `Publish stable revision for NUR` job to create `nur`.
2. Merge the entry in `packaging/nix/nur-registration.json` into the upstream
   [NUR `repos.json`](https://github.com/nix-community/NUR/blob/main/repos.json).
   Confirm the `gandesktop` name and the `4t145` maintainer contact, run NUR's
   manifest checks, and submit the registration PR. Do not replace the upstream
   manifest or commit its generated lock file.
3. After acceptance, set the GitHub repository Actions variable
   `NUR_REPOSITORY=gandesktop`. The release job will then notify NUR's update
   service after advancing the branch. Before that variable is set, publication
   still updates the branch but skips notification.

NUR indexes updates asynchronously; a successful notification does not mean the
new version is already indexed. See the
[NUR contribution instructions](https://github.com/nix-community/NUR#how-to-add-your-own-repository).

To build the same package set locally with your selected nixpkgs:

```sh
nix-build packaging/nix/nur.nix -A gds-files -A xdg-desktop-portal-gds --no-out-link
```
