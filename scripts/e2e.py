#!/usr/bin/env python3
"""Prepare desktop guests or bundle the current normal Files/Images build."""

import argparse
import hashlib
import json
from pathlib import Path
import re
import shlex
import shutil
import subprocess
import tarfile

import vm

ROOT = Path(__file__).resolve().parents[1]
OUTPUT = ROOT / 'target/vm-e2e'


def sha256(path):
    with path.open('rb') as source:
        return hashlib.file_digest(source, 'sha256').hexdigest()


def bundle():
    """Keep Nix ELF interpreters/RPATHs intact; use the guest's Mesa and desktop."""
    output = OUTPUT / 'build'
    output.mkdir(parents=True, exist_ok=True)
    binaries = output / 'bin'
    binaries.mkdir(exist_ok=True)
    roots = set()
    for app in ('files', 'images'):
        source = ROOT / f'target/debug/gds-{app}'
        dependencies = subprocess.check_output(['ldd', str(source)], text=True)
        if 'not found' in dependencies:
            raise RuntimeError(dependencies)
        roots.update(re.findall(r'/nix/store/[^/\s]+', dependencies))
        shutil.copy2(source, binaries / f'gds-{app}.elf')
        subprocess.run(['strip', '--strip-debug', str(binaries / f'gds-{app}.elf')], check=True)
    closure = subprocess.check_output(
        ['nix-store', '--query', '--requisites', *sorted(roots)], text=True).splitlines() if roots else []
    libraries = ':'.join(str(Path(path) / 'lib') for path in closure)
    libraries += ':/usr/lib/x86_64-linux-gnu:/usr/lib64:/usr/lib'
    for app in ('files', 'images'):
        wrapper = binaries / f'gds-{app}'
        loader = subprocess.check_output(['patchelf', '--print-interpreter', str(binaries / f'gds-{app}.elf')], text=True).strip()
        wrapper.write_text(
            '#!/bin/sh\n'
            'export FONTCONFIG_FILE=/etc/fonts/fonts.conf\n'
            f'exec {shlex.quote(loader)} --library-path {shlex.quote(libraries)} "${{0%/*}}/gds-{app}.elf" "$@"\n')
        wrapper.chmod(0o755)
    source_paths = subprocess.check_output(
        ['git', 'ls-files', '-z', '--cached', '--others', '--exclude-standard'], cwd=ROOT
    ).decode().split('\0')
    with tarfile.open(output / 'source.tar.gz', 'w:gz') as archive:
        for name in sorted(set(filter(None, source_paths))):
            path = ROOT / name
            if path.is_file():
                archive.add(path, arcname=name, recursive=False)
    (output / 'source.diff').write_bytes(subprocess.check_output(['git', 'diff', 'HEAD'], cwd=ROOT))
    manifest = {
        'revision': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
        'features': ['files', 'images'],
        'profile': 'dev',
        'source_sha256': sha256(output / 'source.tar.gz'),
        'binaries': {app: sha256(binaries / f'gds-{app}.elf') for app in ('files', 'images')},
        'nix_closure': closure,
    }
    (output / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
    destination = OUTPUT / 'build.tar.gz'
    with tarfile.open(destination, 'w:gz') as archive:
        for app in ('files', 'images'):
            for suffix in ('', '.elf'):
                name = f'bin/gds-{app}{suffix}'
                archive.add(output / name, arcname=name)
        for name in ('manifest.json', 'source.tar.gz', 'source.diff'):
            archive.add(output / name, arcname=name)
        for path in closure:
            archive.add(path, arcname=path.lstrip('/'))
    print(destination)


def prepare(machine):
    targets = vm.MACHINES if machine == 'all' else {machine: vm.MACHINES[machine]}
    for name, domain in targets.items():
        vm.start_machine(domain)
        address = vm.ip_address(domain)
        if not address:
            raise RuntimeError(f'{name}: no DHCP address; wait for guest boot')
        # These are environment dependencies, not substitutes for application behavior.
        command = '''set -eu
if command -v apt-get >/dev/null; then
  sudo apt-get update
  sudo apt-get install -y python3-gi gir1.2-atspi-2.0 python3-pil wl-clipboard
elif command -v dnf >/dev/null; then
  sudo dnf install -y python3-gobject at-spi2-core python3-pillow wl-clipboard
elif command -v pacman >/dev/null; then
  sudo pacman -S --needed --noconfirm python-gobject at-spi2-core python-pillow wl-clipboard grim vulkan-swrast
elif [ "$(uname -s)" = FreeBSD ]; then
  sudo pkg install -y plasma6-kactivitymanagerd python3 py312-pygobject at-spi2-core py312-pillow wl-clipboard
else
  echo 'Unsupported guest package manager' >&2
  exit 1
fi
'''
        print(f'Preparing {name}', flush=True)
        subprocess.run(['ssh', '-o', 'BatchMode=yes', '-o', 'StrictHostKeyChecking=accept-new',
                        f'atlas@{address}', 'sh -s'], input=command, text=True, check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest='command', required=True)
    sub.add_parser('bundle')
    prep = sub.add_parser('prepare')
    prep.add_argument('machine', choices=[*vm.MACHINES, 'all'], default='all', nargs='?')
    args = parser.parse_args()
    if args.command == 'bundle':
        bundle()
    else:
        prepare(args.machine)


if __name__ == '__main__':
    main()
