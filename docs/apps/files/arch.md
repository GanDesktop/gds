# Arch Linux packages and AUR recipe

The recipe builds two independent native packages:

- `gds-files-git`: Files, its desktop entry and icon.
- `xdg-desktop-portal-gds-git`: the optional FileChooser portal backend.

The recipe is in `packaging/arch/PKGBUILD`. It is a VCS package tracking the
project's `main` branch and uses `Cargo.lock`. Having a recipe in this repository
does not mean it has been published to the AUR. Download prebuilt packages from
[GitHub Releases](https://github.com/GanDesktop/gds/releases), or build locally
using one of the methods below; no AUR helper or Nix is needed.

## Build the working tree in a container

On a host with Python 3.11+ and Docker or Podman:

```sh
just arch                  # incremental dev build of the current working tree
just arch release          # optimized release build
python3 scripts/build-linux-package.py arch --profile dev --engine podman
```

This builds against Arch libraries, not the host's Nix libraries. It includes
uncommitted source changes. Output is under `target/arch/arch-latest/`; Cargo
and package caches are under `target/arch-latest/`. `CARGO_BUILD_JOBS` limits
compilation parallelism. Each local build has a timestamped package release.

On the target Arch machine, install the selected package files:

```sh
sudo pacman -U ./gds-files-git-*.pkg.tar.zst
# Optional, install separately:
sudo pacman -U ./xdg-desktop-portal-gds-git-*.pkg.tar.zst
```

Keep only the intended version in the directory when using wildcards. Install
newer builds with the same command; remove with `pacman -R gds-files-git` or
`pacman -R xdg-desktop-portal-gds-git`. User settings and data are retained.

## Build from the upstream recipe / prepare AUR publication

On Arch, install `base-devel` and Git, copy `packaging/arch/PKGBUILD` into an
empty directory, and run as a normal user:

```sh
makepkg --syncdeps --install
makepkg --printsrcinfo > .SRCINFO
```

This fetches the published `main` branch, which must include the shared installer
in `packaging/linux/install.py`. Unlike the container command, it does not build
uncommitted changes. The default recipe uses the release profile. The split
packages provide `gds-files` and `xdg-desktop-portal-gds` and conflict with other
packages providing the same application files.

For publication, submit `PKGBUILD` and the generated `.SRCINFO` to the AUR Git
repository for `gds-files-git` using the maintainer's AUR account. Regenerate
`.SRCINFO` whenever the recipe changes. Do not submit the local container's
timestamped recipe or compiled packages to AUR. AUR hosts build recipes, not a
binary repository.

## Desktop integration

Run `gds-files` from a terminal in the target machine's graphical session. The
packages preserve the default folder handler and existing portal routing.

To explicitly choose Files as the current user's folder handler:

```sh
xdg-mime default io.github.GanDesktop.GdsFiles.desktop inode/directory
```

For the optional portal, add `org.freedesktop.impl.portal.FileChooser=gds` to
`[preferred]` in the active desktop's user `portals.conf`, preserving all other
entries. Log out and back in. The backend is activated on demand and does not
need to be enabled as a persistent user service. Remove that preference before
uninstalling the backend.

Validate actual window rendering, icons, embedded terminal, file operations and
portal dialogs on the target desktop; package CLI smoke checks do not cover GUI
behavior.
