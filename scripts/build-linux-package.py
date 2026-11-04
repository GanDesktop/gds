#!/usr/bin/env python3
"""Build Arch packages or Debian/Ubuntu DEBs in a native cached container."""

import argparse
from datetime import datetime, timezone
from email.utils import format_datetime
import os
import re
from pathlib import Path
import shutil
import subprocess
import tomllib


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('target', choices=['arch', 'debian', 'ubuntu'])
    parser.add_argument('--release', help='Debian/Ubuntu container release (default: trixie/noble)')
    parser.add_argument('--profile', choices=['dev', 'release'], default='dev')
    parser.add_argument('--package-release', type=int, help='Package revision for releases (default: timestamp)')
    parser.add_argument('--engine', choices=['docker', 'podman'],
                        default='docker' if shutil.which('docker') else 'podman')
    args = parser.parse_args()
    if args.package_release is not None and args.package_release < 1:
        parser.error('--package-release must be positive')
    if not shutil.which(args.engine):
        parser.error(f'{args.engine} is required')
    if args.target == 'arch' and args.release:
        parser.error('Arch is rolling; omit --release')
    release = args.release or {'arch': 'latest', 'debian': 'trixie', 'ubuntu': 'noble'}[args.target]
    if not release.replace('.', '').replace('-', '').isalnum():
        parser.error('release must be a container tag such as trixie or noble')
    project = Path(__file__).resolve().parent.parent
    packaging = 'arch' if args.target == 'arch' else 'debian'
    label = f'{args.target}-{release}'
    cache = project / 'target' / label
    output = project / 'target' / ('arch' if packaging == 'arch' else 'deb') / label
    work = cache / 'package'
    work.mkdir(parents=True, exist_ok=True)
    output.mkdir(parents=True, exist_ok=True)
    now = datetime.now(timezone.utc)
    stamp = now.strftime('%Y%m%d%H%M%S')
    version = tomllib.loads((project / 'Cargo.toml').read_text())['package']['version']
    revision = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=project, text=True).strip()
    image = f'gds-{packaging}-builder:{label}'
    build = [args.engine, 'build', '-t', image, '-f',
             str(project / 'packaging' / packaging / 'Containerfile')]
    if packaging == 'debian':
        build += ['--build-arg', f'BASE_IMAGE={args.target}:{release}']
    build += ['--build-arg', f'BUILD_UID={os.getuid()}', str(project / 'packaging' / packaging)]
    subprocess.run(build, check=True)

    if packaging == 'arch':
        shutil.copy2(project / 'packaging/arch/PKGBUILD', work / 'PKGBUILD')
        count = subprocess.check_output(['git', 'rev-list', '--count', 'HEAD'], cwd=project, text=True).strip()
        short = subprocess.check_output(['git', 'rev-parse', '--short', 'HEAD'], cwd=project, text=True).strip()
        recipe = (work / 'PKGBUILD').read_text()
        recipe = re.sub(r'^pkgver=.*$', f'pkgver={version}.r{count}.g{short}', recipe, flags=re.M)
        recipe = recipe.replace('pkgrel=1\n', f'pkgrel={args.package_release or stamp}\n')
        recipe = recipe.replace('#branch=main', f'#commit={revision}')
        (work / 'PKGBUILD').write_text(recipe)
        (work / 'src').mkdir(exist_ok=True)
        source = work / 'src/gds'
        if not source.is_symlink():
            source.symlink_to('/source', target_is_directory=True)
        entry = ['bash', '-c', 'makepkg --noextract --nodeps --force && makepkg --printsrcinfo > /output/.SRCINFO']
    else:
        shutil.copytree(project / 'packaging/debian', work / 'debian', dirs_exist_ok=True,
                        ignore=shutil.ignore_patterns('Containerfile'))
        package_release = str(args.package_release) if args.package_release else f'0.{stamp}'
        (work / 'debian/changelog').write_text(
            f'gds ({version}-{package_release}) unstable; urgency=medium\n\n'
            f'  * Local {args.target} {release} build of {revision}.\n\n'
            f' -- 4t145 <u4t145@163.com>  {format_datetime(now)}\n')
        entry = ['bash', '-c', 'dpkg-buildpackage -b -us -uc -d && cp /cache/*.deb /cache/*.buildinfo /cache/*.changes /output/']

    command = [args.engine, 'run', '--rm', '--user', f'{os.getuid()}:{os.getgid()}']
    if args.engine == 'podman':
        command.append('--userns=keep-id')
    for source, destination, readonly in [(project, '/source', True), (cache, '/cache', False),
                                           (output, '/output', False)]:
        mount = f'type=bind,source={source},target={destination}' + (',readonly' if readonly else '')
        command += ['--mount', mount]
    environment = {
        'HOME': '/tmp', 'CARGO_HOME': '/cache/cargo', 'CARGO_TARGET_DIR': '/cache/target',
        'GDS_SOURCE': '/source', 'GDS_PROFILE': args.profile, 'GDS_BUILD_TAG': revision,
        'PKGDEST': '/output', 'CARGO_NET_GIT_FETCH_WITH_CLI': 'true',
        'GIT_CONFIG_COUNT': '1', 'GIT_CONFIG_KEY_0': 'safe.directory', 'GIT_CONFIG_VALUE_0': '/source',
    }
    for name in ['CARGO_BUILD_JOBS']:
        if name in os.environ:
            environment[name] = os.environ[name]
    for name, value in environment.items():
        command += ['--env', f'{name}={value}']
    command += [image, *entry]
    subprocess.run(command, check=True)
    print(f'Packages: {output}')


if __name__ == '__main__':
    main()
