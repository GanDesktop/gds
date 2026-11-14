#!/usr/bin/env python3
"""Manage local desktop test VMs, snapshots, consoles, and SSH/SCP access."""

import argparse
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import time

MACHINES = {
    'fedora': 'fedora-dev',
    'ubuntu': 'ubuntu-dev',
    'cachyos': 'cachyos-dev',
    'noctalia': 'arch-noctalia',
}
ACTIONS = ['open', 'start', 'stop', 'status', 'reset', 'ssh', 'copy']
SNAPSHOT = 'clean-base'
VIRSH = ['virsh', '--connect', 'qemu:///system']


def virsh_output(*args: str, check: bool = True) -> subprocess.CompletedProcess[str]:
    return subprocess.run([*VIRSH, *args], text=True, capture_output=True, check=check)


def domstate(domain: str) -> str:
    return virsh_output('domstate', domain).stdout.strip()


def current_snapshot(domain: str) -> str:
    proc = virsh_output('snapshot-current', domain, '--name', check=False)
    return (proc.stdout.strip() or '-') if proc.returncode == 0 else '-'


def ip_address(domain: str) -> str | None:
    for source in ('lease', 'agent'):
        proc = virsh_output('domifaddr', domain, '--source', source, check=False)
        if proc.returncode != 0:
            continue
        for match in re.finditer(r'ipv4\s+([\d.]+)/', proc.stdout):
            if not match[1].startswith('127.'):
                return match[1]
    return None


def start_machine(domain: str) -> None:
    if domstate(domain) == 'shut off':
        subprocess.run([*VIRSH, 'start', domain], check=True)


def stop_machine(domain: str, timeout: float = 60.0) -> None:
    if domstate(domain) == 'shut off':
        return
    proc = virsh_output('shutdown', domain, '--mode', 'agent', check=False)
    if proc.returncode != 0:
        subprocess.run([*VIRSH, 'shutdown', domain], check=True)
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if domstate(domain) == 'shut off':
            return
        time.sleep(1.0)
    raise SystemExit(f"Timed out waiting for '{domain}' to shut off.")


def reset_machine(domain: str) -> None:
    info = virsh_output('snapshot-info', domain, SNAPSHOT, check=False)
    if info.returncode != 0:
        detail = info.stderr.strip() or info.stdout.strip()
        suffix = f': {detail}' if detail else ''
        raise SystemExit(f"Snapshot '{SNAPSHOT}' not found for '{domain}'{suffix}")
    subprocess.run([*VIRSH, 'snapshot-revert', domain, SNAPSHOT], check=True)


def open_machine(domain: str, parser: argparse.ArgumentParser) -> None:
    start_machine(domain)
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
    if environment.get('IN_NIX_SHELL'):
        environment.pop('LD_LIBRARY_PATH', None)
    subprocess.run([viewer, '--connect', 'qemu:///system', domain], env=environment, check=True)


def print_status(targets: list[tuple[str, str]]) -> None:
    rows: list[tuple[str, str, str, str, str]] = [
        ('MACHINE', 'DOMAIN', 'STATE', 'ADDRESS', 'SNAPSHOT'),
    ]
    for machine, domain in targets:
        state = domstate(domain)
        address = (ip_address(domain) or '-') if state != 'shut off' else '-'
        rows.append((machine, domain, state, address, current_snapshot(domain)))
    widths = [max(len(row[index]) for row in rows) for index in range(len(rows[0]))]
    for row in rows:
        line = '  '.join(value.ljust(widths[index]) for index, value in enumerate(row))
        print(line.rstrip())


def parse_args(argv: list[str] | None = None) -> tuple[argparse.ArgumentParser, argparse.Namespace]:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('machine', choices=[*MACHINES, 'all'])
    parser.add_argument('action', choices=ACTIONS, nargs='?')
    parser.add_argument('args', nargs=argparse.REMAINDER, help='SSH command or local files to copy to Downloads')
    parsed = parser.parse_args(argv)
    if parsed.action is None:
        parsed.action = 'status' if parsed.machine == 'all' else 'open'
    if parsed.machine == 'all' and parsed.action in ('open', 'ssh', 'copy'):
        parser.error(f"'{parsed.action}' requires a specific machine, not 'all'")
    if parsed.action == 'copy' and not parsed.args:
        parser.error('copy requires at least one local file')
    if parsed.action not in ('ssh', 'copy') and parsed.args:
        parser.error(f"unrecognized arguments for '{parsed.action}': {' '.join(parsed.args)}")
    return parser, parsed


def main(argv: list[str] | None = None) -> None:
    parser, args = parse_args(argv)
    targets = list(MACHINES.items()) if args.machine == 'all' else [(args.machine, MACHINES[args.machine])]

    if args.action == 'status':
        print_status(targets)
        return
    if args.action == 'start':
        for _, domain in targets:
            start_machine(domain)
        return
    if args.action == 'stop':
        for _, domain in targets:
            stop_machine(domain)
        return
    if args.action == 'reset':
        for _, domain in targets:
            reset_machine(domain)
        return

    domain = targets[0][1]
    if args.action == 'open':
        open_machine(domain, parser)
        return

    start_machine(domain)
    address = ip_address(domain)
    if not address:
        parser.exit(1, 'No DHCP address yet; wait for the guest to boot and retry.\n')
    destination = f'atlas@{address}'
    ssh_options = ['-o', 'StrictHostKeyChecking=accept-new']
    if args.action == 'ssh':
        subprocess.run(['ssh', *ssh_options, destination, *args.args], check=True)
    else:
        subprocess.run(['scp', *ssh_options, '--', *args.args, f'{destination}:Downloads/'], check=True)


if __name__ == '__main__':
    main(sys.argv[1:])
