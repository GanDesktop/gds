# Desktop test virtual machines

The local libvirt connection is `qemu:///system`. Open **GDS Test** entries
from the host application launcher, or run these commands from the repository:

```sh
just vm fedora
just vm ubuntu
just vm cachyos
```

These commands start a stopped guest and open its SPICE console in Virt Viewer.
The console provides keyboard and mouse access to the actual desktop.
Choose GNOME or Plasma at the Fedora login screen to test both environments.

| Guest | Desktop | Resources | Address |
| --- | --- | --- | --- |
| `fedora-dev` | Fedora 44, GNOME + KDE | Existing: 8 vCPUs, 16 GiB RAM | DHCP; use `virsh domifaddr` |
| `ubuntu-dev` | Ubuntu 26.04.1 LTS, GNOME | 8 vCPUs, 8 GiB RAM, 80 GiB disk | `192.168.122.120` |
| `cachyos-dev` | CachyOS rolling, KDE Plasma | 8 vCPUs, 8 GiB RAM, 80 GiB disk | `192.168.122.121` |

The new guests use UEFI, virtio storage/network, and virtio graphics with SPICE.
SPICE OpenGL is disabled because the host's NVIDIA display path failed to import
its display buffers. Mesa software Vulkan supports application rendering; this
setup is intended for functional testing rather than GPU performance measurement.
Their desktops log in automatically as `atlas`. SSH accepts the host's
existing Ed25519 key; password login over SSH is disabled. Local desktop/sudo
credentials are stored only on this host in
`~/.local/share/gds-vms/credentials` (mode `0600`). Fedora retains its existing
login configuration; its SSH service must be enabled separately inside Fedora
before using the SSH/copy commands there.

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

## Terminal access and packages

```sh
just vm ubuntu ssh
just vm cachyos ssh
just vm ubuntu ssh 'cat /etc/os-release'
just vm ubuntu copy target/deb/ubuntu-noble/gds-files_*.deb
just vm cachyos copy target/arch/arch-latest/gds-files-*.pkg.tar.zst
just vm ubuntu stop
just vm cachyos start
```

`copy` uploads files into the desktop user's `Downloads` directory. If SSH is
requested immediately after starting a guest, wait for boot to finish and retry.

Build native packages on the host, then install them inside the corresponding
guest. Build Ubuntu packages with `just deb ubuntu resolute`; the existing
`ubuntu-noble` packages can also be used for a compatibility smoke test on 26.04.
Build CachyOS packages with `just arch`.

Both new guests have the existing GDS Files 0.0.1 package installed for an initial
startup check. These packages were built before VM provisioning; rebuild and
transfer packages when testing newer working-tree changes. CachyOS uses its
official x86-64-v3 repositories and `linux-cachyos` kernel.

```sh
# Ubuntu guest
sudo apt install ./Downloads/gds-files_*.deb

# CachyOS guest
sudo pacman -U ./Downloads/gds-files-*.pkg.tar.zst

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
