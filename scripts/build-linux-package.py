#!/usr/bin/env python3
"""Build Arch packages or Debian/Ubuntu DEBs in a native cached container."""

import argparse
import hashlib
from datetime import datetime, timezone
from email.utils import format_datetime
import os
import re
from pathlib import Path
import shutil
import subprocess
import tomllib

DEB_RELEASES = {
    ('debian', 'bookworm'): '12',
    ('debian', 'trixie'): '13',
    ('debian', 'forky'): '14',
    ('ubuntu', 'jammy'): '22.04',
    ('ubuntu', 'noble'): '24.04',
    ('ubuntu', 'oracular'): '24.10',
    ('ubuntu', 'plucky'): '25.04',
    ('ubuntu', 'questing'): '25.10',
    ('ubuntu', 'resolute'): '26.04',
}


def resolve_deb_version_suffix(target: str, release: str) -> str | None:
    if (target, release) in DEB_RELEASES:
        return DEB_RELEASES[(target, release)]
    if re.fullmatch(r'\d+(?:\.\d+)?', release):
        return release
    return None


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('target', choices=['arch', 'debian', 'ubuntu'])
    parser.add_argument('--release', help='Debian/Ubuntu container release (default: trixie/noble)')
    parser.add_argument('--profile', choices=['dev', 'release'], default='dev')
    parser.add_argument('--package-release', type=int, help='Package revision for releases (default: timestamp)')
    parser.add_argument('--release-tag', help='Arch: build the published tag from its downloaded source archive')
    parser.add_argument('--engine', choices=['docker', 'podman'],
                        default='docker' if shutil.which('docker') else 'podman')
    args = parser.parse_args()
    if args.package_release is not None and args.package_release < 1:
        parser.error('--package-release must be positive')
    if not shutil.which(args.engine):
        parser.error(f'{args.engine} is required')
    if args.target == 'arch' and args.release:
        parser.error('Arch is rolling; omit --release')
    if args.release_tag and (args.target != 'arch' or args.profile != 'release'):
        parser.error('--release-tag requires arch --profile release')
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
    if args.release_tag:
        if args.release_tag != f'v{version}':
            parser.error('--release-tag must be v<Cargo.toml version>')
        tag_revision = subprocess.check_output(
            ['git', 'rev-parse', '--verify', f'refs/tags/{args.release_tag}^{{commit}}'],
            cwd=project, text=True).strip()
        if tag_revision != revision:
            parser.error('HEAD must match --release-tag')
    image = f'gds-{packaging}-builder:{label}'
    build = [args.engine, 'build', '-t', image, '-f',
             str(project / 'packaging' / packaging / 'Containerfile')]
    if packaging == 'debian':
        build += ['--build-arg', f'BASE_IMAGE={args.target}:{release}']
    build += ['--build-arg', f'BUILD_UID={os.getuid()}', str(project / 'packaging' / packaging)]
    subprocess.run(build, check=True)

    if packaging == 'arch':
        for stale in output.glob('*.pkg.tar.zst'):
            stale.unlink()
        shutil.copy2(project / 'packaging/arch/PKGBUILD', work / 'PKGBUILD')
        recipe = (work / 'PKGBUILD').read_text()
        recipe = re.sub(r'^pkgver=.*$', f'pkgver={version}', recipe, flags=re.M)
        package_release = args.package_release or (1 if args.release_tag else stamp)
        recipe = re.sub(r'^pkgrel=.*$', f'pkgrel={package_release}', recipe, flags=re.M)
        if args.release_tag:
            archive = work / f'gds-{version}.tar.gz'
            subprocess.run(['curl', '--fail', '--location', '--retry', '3',
                            f'https://github.com/GanDesktop/gds/archive/refs/tags/{args.release_tag}.tar.gz',
                            '--output', str(archive)], check=True)
            with archive.open('rb') as source:
                checksum = hashlib.file_digest(source, 'sha256').hexdigest()
            recipe = re.sub(r'^sha256sums=.*$', f"sha256sums=('{checksum}')", recipe, flags=re.M)
        (work / 'PKGBUILD').write_text(recipe)
        if args.release_tag:
            entry = ['bash', '-ec', 'makepkg --cleanbuild --force; '
                     'namcap /output/*.pkg.tar.zst; '
                     'mkdir -p /output/aur; cp PKGBUILD /output/aur/; '
                     'makepkg --printsrcinfo > /output/aur/.SRCINFO']
        else:
            (work / 'src').mkdir(exist_ok=True)
            source = work / f'src/gds-{version}'
            if source.exists() and not source.is_symlink():
                shutil.rmtree(source)
            if not source.is_symlink():
                source.symlink_to('/source', target_is_directory=True)
            entry = ['bash', '-ec', 'makepkg --noextract --nodeps --force; '
                     'makepkg --printsrcinfo > /output/.SRCINFO']
    else:
        for directory in (output, cache):
            for pattern in ('*.deb', '*.buildinfo', '*.changes'):
                for stale in directory.glob(pattern):
                    stale.unlink()
        shutil.copytree(project / 'packaging/debian', work / 'debian', dirs_exist_ok=True,
                        ignore=shutil.ignore_patterns('Containerfile'))
        if args.package_release is not None:
            distro_version = resolve_deb_version_suffix(args.target, release)
            if distro_version is None:
                parser.error(f'Unknown numeric version for {args.target}:{release}')
            package_release = f'{args.package_release}~{args.target}{distro_version}'
        else:
            package_release = f'0.{stamp}'
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
