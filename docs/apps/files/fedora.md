# Install GDS Files on Fedora

GDS Files is available as a native RPM built for the target Fedora release and
the build host's architecture. The Fedora machine needs neither Rust nor Nix.

## Installation

Copy the `gds-files-*.rpm` produced by the build to Fedora, then install it:

```sh
sudo dnf install ./gds-files-*.rpm
gds-files --enable-logs
```

The package installs the `gds-files` command, the Files desktop entry, its icon
and its runtime dependencies. Run it from the Fedora desktop's terminal or
application launcher, inside a Wayland session. Installing it preserves existing
default applications and user settings.

Both dev and release packages embed their interface icons in the executable;
they do not need the build host's Cargo cache at runtime. Default UI and
monospace fonts follow Fontconfig's `sans-serif` and `monospace` aliases.

To make Files the default folder handler for the current user:

```sh
xdg-mime default io.github.GanDesktop.GdsFiles.desktop inode/directory
```

Keep test files on the guest's own filesystem when checking file permissions,
trash, renaming and volume operations. Check clipboard and drag-and-drop with
Nautilus within the same guest desktop. VM rendering performance depends on the
virtual GPU and its driver; it is not a measure of native hardware performance.

## Transferring a package to a virtual machine

For a libvirt VM, find its address on the host:

```sh
virsh -c qemu:///system domifaddr fedora-dev --source lease
```

Replace `fedora-dev` with the VM name. With SSH available in the guest, copy the
Files RPM from the host; replace `USER` and `GUEST_IP` with the guest login and
address:

```sh
scp target/rpm/fedora-43/x86_64/gds-files-*.rpm USER@GUEST_IP:~/Downloads/
```

Then, in a terminal inside the Fedora desktop:

```sh
cd ~/Downloads
sudo dnf install ./gds-files-*.rpm
gds-files --enable-logs
```

If the guest needs an SSH server, install `openssh-server` and start `sshd` there.
An existing QEMU Guest Agent also allows a host administrator to transfer the
package without enabling SSH. Guest policy may prevent the agent from running
package-management tools; install a transferred RPM with `sudo dnf install` in
the guest's desktop terminal. The normal application should run as the logged-in
desktop user.

Use the build directory matching the Fedora target and architecture. For a
Fedora 44 build, run `just rpm 44` and copy from `target/rpm/fedora-44/`. An
older-target RPM may also run on a newer Fedora release when DNF can satisfy
its runtime dependencies; verify `gds-files --help` and launch it in that desktop
session before using it for testing.

### Virtual GPU configuration

Keep the VM's working display configuration when testing a new package.
**Virtio 3D acceleration** with **SPICE OpenGL** can improve rendering when the
host renderer and guest drivers support it, but needs separate desktop
validation. SPICE OpenGL uses a local connection with the listen type set to
None and a host render device supported by the host's Mesa/EGL stack. Changing
the virtual GPU configuration requires shutting down the VM and starting it
again; restore the previous configuration if the desktop becomes unusable.

An LLVM failure ending in `fs_variant_partial` comes from the guest's Mesa
software shader compiler. Verify the virtual GPU and guest graphics drivers
when this happens. The RPM builds use Cargo's portable CPU target; changing
Rust's CPU flags does not repair a guest graphics-driver failure.

Older guest drivers may also need updating. For example, Mesa 26.0.7 includes
[LLVM 22 scatter/gather compatibility fixes](https://docs.mesa3d.org/relnotes/26.0.7.html)
relevant to `X86ISD::MGATHER` failures with older Mesa versions. Update the
installed graphics and desktop components from Fedora's repositories:

```sh
sudo dnf upgrade --refresh 'mesa-*' llvm-libs mutter gnome-shell
```

Log out and back in after updating so the desktop loads the new libraries.
A startup error containing `NoReply: Remote peer disconnected` can also result
from a Files process crashing during a D-Bus request. Check
`journalctl --user -b` for the underlying failure.

After installing or upgrading, open the actual window and check its toolbar
icons, file list, settings and embedded terminal. The package's command-line
checks alone do not exercise window rendering.

### Pasting commands into the guest

In the Fedora terminal, paste text copied from the host with **Ctrl+Shift+V**
or the terminal's right-click Paste action.

If a SPICE console has `spice-vdagent` installed but clipboard sharing stops
working, restart the current user's agent from a terminal in the guest:

```sh
systemctl --user restart spice-vdagent.service
```

Copy the text again on the host, then paste it into the guest terminal.

## Updating and removing

Install the newly built RPM with the same `dnf install` command. Each build has
a timestamped RPM release, so a newer build upgrades the previous one even when
the application version is unchanged. Keep only the intended RPM in the current
directory when using the wildcard.

```sh
sudo dnf remove gds-files
```

Removal keeps settings in `~/.config/gds/files/` and state in
`~/.local/state/gds/files/`. If you changed the default folder handler, choose a
replacement with `xdg-mime`.

## Optional GDS Portal

The build also produces `xdg-desktop-portal-gds-*.rpm`. Install it separately to
test the system file picker:

```sh
sudo dnf install ./xdg-desktop-portal-gds-*.rpm
```

Installing the backend does not change the desktop's Portal routing. In the
existing user configuration for the active desktop, for example
`~/.config/xdg-desktop-portal/gnome-portals.conf`, add this preference to its
`[preferred]` section, preserving the other entries:

```ini
[preferred]
org.freedesktop.impl.portal.FileChooser=gds
```

Create the parent directory and file if absent. Log out and back in to load the
configuration, then test a Portal-enabled application's open/save dialog. The
backend is activated on demand; it does not need `systemctl enable`.

To remove it, remove the FileChooser preference and run:

```sh
sudo dnf remove xdg-desktop-portal-gds
```

Log out and back in to restore the desktop's usual file picker.

## Building RPMs on the development host

The host needs Python 3.11+, Docker or Podman, and access to the container engine.
From the repository root:

```sh
just rpm                     # Fedora 43, incremental dev build
just rpm 43 release          # Optimized release build
# Without just, or to select the container engine explicitly:
python3 scripts/build-rpm.py --fedora 43 --profile dev --engine docker
```

Choose the Fedora release that matches the test machine. The command builds a
Fedora toolchain image on the host, compiles the current working tree with Cargo's
lockfile, and builds two independent binary RPMs. Uncommitted source changes are
included. The VM only installs the resulting packages.

The Containerfile, RPM specification and staging script are in
`packaging/fedora/`. Nix package definitions are maintained separately in
`packaging/nix/`.

The default `dev` profile reuses Cargo's incremental cache and uses fast RPM
compression for quick testing.
The `release` profile uses the project's full LTO configuration and takes longer.
Both are packaged with standard names and stripped executables; the RPM release
timestamp allows switching between profiles through a normal upgrade.

Artifacts are under `target/rpm/fedora-43/<architecture>/`; dependencies and
compilation caches are under `target/fedora-43/`. Subsequent builds reuse the
container image and caches. Set `CARGO_BUILD_JOBS` to limit compilation parallelism.

This is the project's local binary-package workflow, not a Fedora distribution
source-package submission. Binaries are built against Fedora's libraries inside
the host container. Existing Nix-linked binaries cannot be reused in these RPMs.
