#!/usr/bin/env python3
"""Generate MANIFEST (v1) and SHA256SUMS for built GDS Linux release packages."""

import argparse
from dataclasses import dataclass
import hashlib
from pathlib import Path
import re
import sys

TAG_RE = re.compile(r'^v?[0-9][a-zA-Z0-9._-]*$')
SAFE_TOKEN_RE = re.compile(r'^[a-zA-Z0-9][a-zA-Z0-9._~+-]*$')
DEFAULT_DISTRO_RE = re.compile(r'^([a-z0-9-]+):([0-9]+(?:\.[0-9]+)?)$')

DEB_RE = re.compile(r'^(gds-files|xdg-desktop-portal-gds)_([^_]+)_([^_.]+)\.deb$')
DEB_SUFFIX_RE = re.compile(r'~([a-z]+)([0-9]+(?:\.[0-9]+)?)$')
RPM_RE = re.compile(
    r'^(gds-files|xdg-desktop-portal-gds)-(.+)-([^-]+\.fc([0-9]+))\.([^.]+)\.rpm$'
)

COMPONENT_MAP = {
    'gds-files': 'files',
    'gds-files-git': 'files',
    'xdg-desktop-portal-gds': 'portal',
    'xdg-desktop-portal-gds-git': 'portal',
}
BACKEND_ORDER = {'pacman': 0, 'deb': 1, 'rpm': 2}
COMPONENT_ORDER = {'files': 0, 'portal': 1}
IGNORED_FILES = {'MANIFEST', 'SHA256SUMS', 'install.sh'}


@dataclass(frozen=True)
class ManifestRow:
    component: str
    backend: str
    distro: str
    distro_version: str
    package_arch: str
    package_version: str
    filename: str
    sha256: str


def validate_token(value: str, label: str, filename: str) -> None:
    if not SAFE_TOKEN_RE.fullmatch(value):
        raise ValueError(f'Unsafe {label} "{value}" parsed from "{filename}"')


def parse_package_filename(
    filename: str, default_deb_distro: tuple[str, str] | None
) -> tuple[str, str, str, str, str, str]:
    """Return (component, backend, distro, distro_version, package_arch, package_version)."""
    if filename.endswith('.pkg.tar.zst'):
        stem = filename.removesuffix('.pkg.tar.zst')
        parts = stem.rsplit('-', 3)
        if len(parts) != 4:
            raise ValueError(f'Unrecognized pacman package filename: {filename}')
        pkgname, pkgver, pkgrel, arch = parts
        component = COMPONENT_MAP.get(pkgname)
        if component is None:
            raise ValueError(f'Unsupported pacman package "{pkgname}" in "{filename}"')
        package_version = f'{pkgver}-{pkgrel}'
        validate_token(package_version, 'package_version', filename)
        validate_token(arch, 'package_arch', filename)
        return component, 'pacman', 'arch', 'rolling', arch, package_version

    if filename.endswith('.deb'):
        match = DEB_RE.fullmatch(filename)
        if not match:
            raise ValueError(f'Unrecognized DEB package filename: {filename}')
        pkgname, version_part, arch = match.groups()
        component = COMPONENT_MAP[pkgname]
        suffix_match = DEB_SUFFIX_RE.search(version_part)
        if suffix_match:
            distro, distro_version = suffix_match.groups()
        elif default_deb_distro is not None:
            distro, distro_version = default_deb_distro
        else:
            raise ValueError(
                f'DEB "{filename}" version "{version_part}" lacks ~<distro><version> suffix'
            )
        validate_token(version_part, 'package_version', filename)
        validate_token(arch, 'package_arch', filename)
        return component, 'deb', distro, distro_version, arch, version_part

    if filename.endswith('.rpm'):
        match = RPM_RE.fullmatch(filename)
        if not match:
            raise ValueError(f'Unrecognized RPM package filename: {filename}')
        pkgname, ver, rel, fc_num, arch = match.groups()
        component = COMPONENT_MAP[pkgname]
        package_version = f'{ver}-{rel}'
        validate_token(package_version, 'package_version', filename)
        validate_token(arch, 'package_arch', filename)
        return component, 'rpm', 'fedora', fc_num, arch, package_version

    raise ValueError(f'Unrecognized package or unexpected file in release dir: {filename}')


def version_sort_key(version: str) -> tuple[int, list[int], str]:
    if version == 'rolling':
        return (0, [], '')
    if re.fullmatch(r'[0-9]+(?:\.[0-9]+)*', version):
        return (1, [int(part) for part in version.split('.')], '')
    return (2, [], version)


def build_manifest_and_sums(
    tag: str, directory: Path, default_deb_distro: tuple[str, str] | None = None
) -> list[ManifestRow]:
    if not TAG_RE.fullmatch(tag):
        raise ValueError(f'Invalid release tag: {tag}')
    if not directory.is_dir():
        raise ValueError(f'Directory does not exist: {directory}')

    rows: list[ManifestRow] = []
    seen_keys: set[tuple[str, str, str, str, str]] = set()

    for path in sorted(directory.iterdir(), key=lambda p: p.name):
        if path.is_dir() or path.name in IGNORED_FILES:
            continue
        component, backend, distro, distro_version, package_arch, package_version = (
            parse_package_filename(path.name, default_deb_distro)
        )
        key = (component, backend, distro, distro_version, package_arch)
        if key in seen_keys:
            raise ValueError(f'Duplicate manifest entry for tuple {key}: {path.name}')
        seen_keys.add(key)
        with path.open('rb') as handle:
            sha256 = hashlib.file_digest(handle, 'sha256').hexdigest()
        rows.append(
            ManifestRow(
                component=component,
                backend=backend,
                distro=distro,
                distro_version=distro_version,
                package_arch=package_arch,
                package_version=package_version,
                filename=path.name,
                sha256=sha256,
            )
        )

    if not rows:
        raise ValueError(f'No release packages found in {directory}')

    rows.sort(
        key=lambda r: (
            BACKEND_ORDER[r.backend],
            r.distro,
            version_sort_key(r.distro_version),
            r.package_arch,
            COMPONENT_ORDER[r.component],
        )
    )

    sums_lines = [
        f'{row.sha256}  {row.filename}'
        for row in sorted(rows, key=lambda r: r.filename)
    ]
    (directory / 'SHA256SUMS').write_text('\n'.join(sums_lines) + '\n', encoding='utf-8')

    headers = (
        'component',
        'backend',
        'distro',
        'distro_version',
        'package_arch',
        'package_version',
        'filename',
        'sha256',
    )
    tuple_rows = [
        (
            r.component,
            r.backend,
            r.distro,
            r.distro_version,
            r.package_arch,
            r.package_version,
            r.filename,
            r.sha256,
        )
        for r in rows
    ]
    widths = [
        max(len(headers[i]), *(len(row[i]) for row in tuple_rows))
        for i in range(len(headers) - 1)
    ]

    def format_cols(cols: tuple[str, ...]) -> str:
        padded = [cols[i].ljust(widths[i]) for i in range(len(headers) - 1)]
        padded.append(cols[-1])
        return '  '.join(padded)

    manifest_lines = [
        f'# gds-manifest-v1 release={tag}',
        f'# {format_cols(headers)}',
        *(format_cols(row) for row in tuple_rows),
    ]
    (directory / 'MANIFEST').write_text('\n'.join(manifest_lines) + '\n', encoding='utf-8')
    return rows


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--tag', required=True, help='Release tag (e.g. v0.0.3)')
    parser.add_argument('--dir', required=True, type=Path, help='Release asset directory')
    parser.add_argument(
        '--default-deb-distro',
        help='Fallback distro:version for legacy unsuffixed DEBs (e.g. debian:13)',
    )
    args = parser.parse_args()

    default_deb: tuple[str, str] | None = None
    if args.default_deb_distro:
        match = DEFAULT_DISTRO_RE.fullmatch(args.default_deb_distro)
        if not match:
            parser.error('--default-deb-distro must look like <distro>:<version>')
        default_deb = (match.group(1), match.group(2))

    try:
        rows = build_manifest_and_sums(args.tag, args.dir, default_deb)
    except ValueError as err:
        print(f'Error: {err}', file=sys.stderr)
        return 1

    print(f'Generated MANIFEST ({len(rows)} entries) and SHA256SUMS in {args.dir}')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
