#!/usr/bin/env python3
"""Open a local test desktop or connect to it over SSH."""

import argparse
import os
from pathlib import Path
import re
import shutil
import subprocess


MACHINES = {'fedora': 'fedora-dev', 'ubuntu': 'ubuntu-dev', 'cachyos': 'cachyos-dev'}
VIRSH = ['virsh', '--connect', 'qemu:///system']


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('machine', choices=MACHINES)
    parser.add_argument('action', choices=['open', 'start', 'stop', 'ssh', 'copy'], default='open', nargs='?')
    parser.add_argument('args', nargs=argparse.REMAINDER, help='SSH command or local files to copy to Downloads')
    args = parser.parse_args()
    name = MACHINES[args.machine]
    if args.action == 'stop':
        subprocess.run([*VIRSH, 'shutdown', name], check=True)
        return
    state = subprocess.check_output([*VIRSH, 'domstate', name], text=True).strip()
    if state == 'shut off':
        subprocess.run([*VIRSH, 'start', name], check=True)
    if args.action == 'start':
        return
    if args.action == 'open':
        viewer = shutil.which('virt-viewer')
        if not viewer:
            local_viewer = Path.home() / '.local/share/gds-vms/virt-viewer/bin/virt-viewer'
            if local_viewer.is_file():
                viewer = str(local_viewer)
        if not viewer:
            parser.exit(1, 'Install virt-viewer to open the SPICE desktop console.\n')
        environment = os.environ.copy()
        if environment.get('WAYLAND_DISPLAY'):
            # Use native Wayland clipboard access instead of the host X11 bridge.
            environment['GDK_BACKEND'] = 'wayland'
        subprocess.run([viewer, '--connect', 'qemu:///system', name], env=environment, check=True)
        return
    addresses = subprocess.check_output([*VIRSH, 'domifaddr', name, '--source', 'lease'], text=True)
    match = re.search(r'ipv4\s+([\d.]+)/', addresses)
    if not match:
        parser.exit(1, 'No DHCP address yet; wait for the guest to boot and retry.\n')
    destination = f'atlas@{match[1]}'
    if args.action == 'ssh':
        subprocess.run(['ssh', destination, *args.args], check=True)
    else:
        if not args.args:
            parser.error('copy requires at least one local file')
        subprocess.run(['scp', '--', *args.args, f'{destination}:Downloads/'], check=True)


if __name__ == '__main__':
    main()
