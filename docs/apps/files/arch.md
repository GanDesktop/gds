# Arch Linux packages and AUR recipe

The recipe builds two independent native packages:

- `gds-files`: Files, its desktop entry and icon.
- `xdg-desktop-portal-gds`: the optional FileChooser portal backend.

The recipe is in `packaging/arch/PKGBUILD`. It builds a versioned release
archive, verifies its SHA-256 checksum, and uses `Cargo.lock`. Having a recipe in this repository
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
sudo pacman -U ./gds-files-*.pkg.tar.zst
# Optional, install separately:
sudo pacman -U ./xdg-desktop-portal-gds-*.pkg.tar.zst
```

Keep only the intended version in the directory when using wildcards. Install
newer builds with the same command; remove with `pacman -R gds-files` or
`pacman -R xdg-desktop-portal-gds`. User settings and data are retained.

## Build from the upstream recipe / prepare AUR publication

On Arch, install `base-devel` and Git, copy `packaging/arch/PKGBUILD` into an
empty directory, and run as a normal user:

```sh
makepkg --syncdeps --install
makepkg --printsrcinfo > .SRCINFO
```

This fetches the release archive declared in the recipe. Unlike the container
command, it does not build uncommitted changes. The default recipe uses the
release profile. Files and Portal remain separately installable.

## Release automation

The `Release Linux packages` workflow builds the published tag (`v<version>`,
matching `Cargo.toml`). Its Arch job downloads that tag's source archive, updates
`pkgver` and its SHA-256 checksum, resets `pkgrel` to `1`, and runs `makepkg` in
an Arch container, including source extraction, dependency checks and CLI smoke
tests. It also runs `namcap` for packaging diagnostics.

The workflow saves the exact built `PKGBUILD` and generated `.SRCINFO` as the
`aur-gds-files` artifact. After all native packages have been uploaded to GitHub
Releases, a separate job pushes those two files to the AUR repository for
`gds-files`. This single repository produces both split packages. Prereleases
are built but are not pushed to AUR. Re-running an unchanged publication is a
no-op. AUR users compile from source; prebuilt packages remain on GitHub Releases.

One-time maintainer setup:

1. Create an [AUR account](https://aur.archlinux.org/register) and confirm that
   `gds-files` and `xdg-desktop-portal-gds` are available, or arrange access with
   their existing maintainer. An existing package requires maintainer or
   co-maintainer access.
2. Generate a dedicated SSH key for publishing and add its public key to the
   AUR account. Store the private key as the GitHub repository Actions secret
   `AUR_SSH_PRIVATE_KEY`; never commit it.
3. Store the verified `aur.archlinux.org` SSH known-hosts entry as the Actions
   secret `AUR_KNOWN_HOSTS`. Compare its fingerprint with the values linked by
   the [AUR submission guidelines](https://wiki.archlinux.org/title/AUR_submission_guidelines#Authentication).
   Do not trust an unverified `ssh-keyscan` result.
4. Set the repository Actions variable `AUR_ENABLED=true` once the account and
   credentials are ready. Leave it unset while AUR registration is unavailable;
   the AUR job is skipped while native package and NUR publication continue.
5. Publish a new GitHub Release from a tag containing this workflow. The first
   successful AUR push creates the package repository when the name is available.

When AUR publication is enabled, missing credentials fail only the AUR job,
after release assets are uploaded.
After configuring the secrets, re-run the failed job. To repair packaging for
an already published upstream version, increment `pkgrel`, rebuild and regenerate
`.SRCINFO`, then push the tested recipe to the same AUR repository. Do not submit
local timestamped working-tree recipes or binary packages to AUR.

For a local reproduction of the release build, check out its tag in a separate
worktree and run:

```sh
python3 scripts/build-linux-package.py arch --profile release --release-tag v0.0.2
```

Replace the example tag with the intended version. The publication recipe is
written to `target/arch/arch-latest/aur/`. After AUR publication, users can install
with an AUR helper, for example `yay -S gds-files`, and optionally
`yay -S xdg-desktop-portal-gds`.

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
