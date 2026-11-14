# Desktop test virtual machines

The local libvirt connection is `qemu:///system`. Open **GDS Test** entries
from the host application launcher, or run these commands from the repository:

```sh
just vm            # Show status and active snapshots of all test VMs
just vm fedora
just vm ubuntu
just vm cachyos
just vm noctalia
```

These commands start a stopped guest and open its SPICE console in Virt Viewer.
The console provides keyboard and mouse access to the actual desktop.
Choose GNOME or Plasma at the Fedora login screen to test both environments.

| Guest | Desktop | Resources | Address |
| --- | --- | --- | --- |
| `fedora-dev` | Fedora 44, GNOME + KDE | Existing: 8 vCPUs, 16 GiB RAM | DHCP; use `just vm` / `virsh domifaddr` |
| `ubuntu-dev` | Ubuntu 26.04.1 LTS, GNOME | 8 vCPUs, 8 GiB RAM, 80 GiB disk | `192.168.122.120` |
| `cachyos-dev` | CachyOS rolling, KDE Plasma | 8 vCPUs, 8 GiB RAM, 80 GiB disk | `192.168.122.121` |
| `arch-noctalia` | Arch Linux, Umbriel + Noctalia | 8 vCPUs, 8 GiB RAM | DHCP (`192.168.122.63`) |

The new guests use UEFI, virtio storage/network, and virtio graphics with SPICE.
SPICE OpenGL is disabled because the host's NVIDIA display path failed to import
its display buffers. Mesa software Vulkan supports application rendering; this
setup is intended for functional testing rather than GPU performance measurement.
All four guests log in automatically as `atlas` on the desktop (Fedora preserves its
existing desktop login), accept the host's Ed25519 key over SSH with password
authentication disabled, and allow passwordless `sudo` (`NOPASSWD: ALL`) for automated
package testing. Local desktop/sudo credentials are stored only on this host in
`~/.local/share/gds-vms/credentials` (mode `0600`).

> [!IMPORTANT]
> `arch-noctalia` now runs on a libvirt external disk snapshot (`clean-base`). Start
> and manage it with `just vm noctalia` or `virsh` rather than the legacy standalone
> `/home/atlas/VMs/arch-noctalia/start.sh` script (which points directly at the base `.qcow2` file).

## Shared clipboard

Use the **GDS Test** launcher or `just vm` console for bidirectional text copying.
On a Wayland host, the launcher explicitly uses native GTK Wayland support.
The existing Virtual Machine Manager console on this workstation did not reliably
transfer clipboard contents through the host's X11/Wayland bridge. Both guest
`spice-vdagentd` and desktop-session `spice-vdagent` must be running.

Copy text with **Ctrl+C**. In a guest terminal, paste with **Ctrl+Shift+V**.
When recovering an interrupted SPICE clipboard connection, restart the guest's
session agent with `systemctl --user restart spice-vdagent` and reconnect the
console. Copy the text again after reconnecting.

Ubuntu still uses GNOME's default idle lock. Unlock the desktop before testing
clipboard sharing; do not rely on the shared clipboard to enter the lock-screen
password. If clipboard transfer remains stuck after unlocking, close Virt Viewer
and reopen it with `just vm ubuntu`, then copy the text again.

The host needs `virt-viewer`. On this NixOS workstation it is retained at
`~/.local/share/gds-vms/virt-viewer` with
`nix build nixpkgs#virt-viewer --out-link ~/.local/share/gds-vms/virt-viewer`.

## Clean test baselines (`clean-base`)

All four guests have an offline libvirt external disk snapshot named `clean-base`
containing a clean desktop with `sshd`, `qemu-guest-agent`, and `~/Downloads`, and
**no** pre-installed `gds*` / `xdg-desktop-portal-gds` packages or GDS user configuration,
cache, or desktop overrides.

Before running a packaging or installer regression test, revert a guest (or all guests)
to the clean baseline:

```sh
just vm                 # List state, IP, and active snapshot for all 4 VMs
just vm ubuntu reset    # Revert ubuntu-dev back to clean-base
just vm all reset       # Revert all 4 VMs back to clean-base
```

## Terminal access and packages

```sh
just vm ubuntu ssh
just vm cachyos ssh
just vm fedora ssh
just vm noctalia ssh
just vm ubuntu ssh 'cat /etc/os-release'
just vm ubuntu copy target/deb/ubuntu-resolute/gds-files_*.deb
just vm cachyos copy target/arch/arch-latest/gds-files-*.pkg.tar.zst
just vm noctalia copy target/arch/arch-latest/gds-files-*.pkg.tar.zst
just vm fedora copy target/rpm/fedora-44/x86_64/gds-files-*.rpm
just vm ubuntu stop
just vm cachyos start
```

`copy` uploads files into the desktop user's `Downloads` directory (`fedora-dev`
links `~/Downloads` to `~/下载`). If SSH is requested immediately after starting a guest,
wait for boot to finish and retry.

Build native packages on the host, then install them inside the corresponding
guest (using minimal dependency flags when verifying that optional helpers are not pulled in).
Build Ubuntu packages with `just deb ubuntu resolute`; the existing
`ubuntu-noble` packages can also be used for a compatibility smoke test on 26.04.
Build Arch packages with `just arch` and Fedora packages with `just rpm 44`.

```sh
# Ubuntu guest (minimal dependencies without Recommends)
sudo apt install --no-install-recommends ./Downloads/gds-files_*.deb

# CachyOS / Arch guest
sudo pacman -U ./Downloads/gds-files-*.pkg.tar.zst

# Fedora guest (without weak dependencies)
sudo dnf install --setopt=install_weak_deps=False ./Downloads/gds-files-*.rpm

# Run from a terminal in the guest desktop session
gds-files --enable-logs
```

Test clipboard, portals, file operations, and the embedded terminal inside the
guest's own Wayland session. Create test files on its own filesystem so that
permissions and trash semantics match the distribution. Installing GDS Portal
and changing the desktop's portal routing are separate, opt-in test steps.

Disks and installation media are in the libvirt `default` and `iso` pools.
Download/setup files and private credentials are under
`~/.local/share/gds-vms/`; they are outside the repository. The VMs do not
start automatically with the host.

Image sources: [Ubuntu LTS](https://releases.ubuntu.com/26.04/),
[Ubuntu cloud image](https://cloud-images.ubuntu.com/releases/resolute/release/),
and [CachyOS desktop media](https://build.cachyos.org/ISO/desktop/260809/).
Ubuntu was provisioned from the official cloud image with Ubuntu's GNOME desktop
packages; CachyOS was installed from its signed repositories using the official
live environment. Both downloaded images were checked against their published
SHA-256 checksums.
