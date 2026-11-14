# Debian and Ubuntu packages

Build two native DEBs: `gds-files` and the optional
`xdg-desktop-portal-gds`. The target machine installs them with APT and does not
need Rust or Nix. Build separately for each target distribution and release;
a Debian DEB is not assumed to be compatible with Ubuntu or vice versa.

## Build

The host needs Python 3.11+ and Docker or Podman. From the repository root:

```sh
just deb                              # Debian 13 (trixie), dev profile
just deb debian trixie release         # optimized Debian build
just deb ubuntu noble                 # Ubuntu 24.04, dev profile
just deb ubuntu noble release         # optimized Ubuntu 24.04 build
just deb ubuntu resolute release      # optimized Ubuntu 26.04 build
# Without just, distro-specific defaults are selected automatically:
python3 scripts/build-linux-package.py ubuntu --engine podman
```

Output is under `target/deb/debian-trixie/`, `target/deb/ubuntu-noble/`, or `target/deb/ubuntu-resolute/`.
Cargo and packaging caches are under `target/<distribution>-<release>/`.
`CARGO_BUILD_JOBS` limits compilation parallelism. The current working tree,
including uncommitted changes, is compiled against the target distribution's
libraries inside a container. Nix-linked host binaries are not reused.

The builder image installs a current stable Rust toolchain with rustup because
the distribution compiler may be too old for the project's dependencies. It is
cached in the image; rebuild the image when a newer compiler is required.
`packaging/debian/Containerfile` accepts `RUST_TOOLCHAIN` to pin it when building
the image directly. Runtime users only need the dependencies declared in the DEB.

Debhelper generates shared-library dependencies and user-service lifecycle
scripts. Files and Portal remain independently installable. Local package
versions include a timestamp (`0.<timestamp>`) so a newer build upgrades the
previous version; release builds passed `--package-release` append
`~<distro><version>` (for example `1~ubuntu24.04` or `1~ubuntu26.04`) so packages
across targets remain distinct and sort cleanly.
This is a local binary-package workflow, not a Debian source-package submission:
Cargo downloads its locked dependencies during the build, and the container
provides the Rust toolchain instead of using the distribution's Rust packages.

## Install, update and remove

Copy the DEB matching the target machine's distribution, release and architecture,
then run there:

```sh
sudo apt install ./gds-files_*.deb
# Optional native FileChooser portal:
sudo apt install ./xdg-desktop-portal-gds_*.deb
gds-files --help
```

Use a directory containing only the intended version when using wildcards.
Run `gds-files` from the application launcher or a terminal inside a graphical
session. Updating uses the same `apt install ./...deb` command. To remove:

```sh
sudo apt remove gds-files
sudo apt remove xdg-desktop-portal-gds
```

Settings and application data in the user's home directory are preserved.
Installing the packages does not change default applications or Portal routing.
To select Files as the current user's default folder handler explicitly:

```sh
xdg-mime default io.github.GanDesktop.GdsFiles.desktop inode/directory
```

For the optional portal, add `org.freedesktop.impl.portal.FileChooser=gds` to
`[preferred]` in the active desktop's user `portals.conf`, preserving existing
entries, then log out and back in. Remove that preference before uninstalling
the backend. The backend is D-Bus activated, not an always-enabled user service.

Test the actual window, icons, embedded terminal, file operations and Portal
open/save dialogs on the target desktop. CLI package checks alone cannot verify
rendering and desktop integration.

## Distribution

APT can install the local DEBs and resolve their dependencies from the system's
configured repositories. No public GDS APT repository is configured or published
by these build commands, so automatic GDS upgrades through `apt upgrade` require
a separately hosted and signed repository. Publishing such a repository needs
the project's hosting location and signing key; do not advertise an `apt install
gds-files` command without that repository.
