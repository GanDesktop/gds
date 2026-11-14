"""Exercise release manifest generation and scripts/install.sh with hermetic fixtures."""

import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SCRIPTS_DIR = Path(__file__).resolve().parents[1]
INSTALLER = SCRIPTS_DIR / 'install.sh'
MANIFEST_SCRIPT = SCRIPTS_DIR / 'generate-release-manifest.py'

PACKAGES = [
    'gds-files-1.2.3-1-x86_64.pkg.tar.zst',
    'xdg-desktop-portal-gds-1.2.3-1-x86_64.pkg.tar.zst',
    'gds-files_1.2.3-1~debian13_amd64.deb',
    'xdg-desktop-portal-gds_1.2.3-1~debian13_amd64.deb',
    'gds-files_1.2.3-1~ubuntu24.04_amd64.deb',
    'xdg-desktop-portal-gds_1.2.3-1~ubuntu24.04_amd64.deb',
    'gds-files_1.2.3-1~ubuntu26.04_amd64.deb',
    'xdg-desktop-portal-gds_1.2.3-1~ubuntu26.04_amd64.deb',
    'gds-files-1.2.3-1.fc43.x86_64.rpm',
    'xdg-desktop-portal-gds-1.2.3-1.fc43.x86_64.rpm',
    'gds-files-1.2.3-1.fc44.x86_64.rpm',
    'xdg-desktop-portal-gds-1.2.3-1.fc44.x86_64.rpm',
]

STUB = r'''#!/usr/bin/env python3
import itertools
import json
import os
import pathlib
import re
import sys

name = pathlib.Path(sys.argv[0]).name
args = sys.argv[1:]

with open(os.environ['TEST_LOG'], 'a', encoding='utf-8') as log:
    log.write(json.dumps([name, *args]) + '\n')


def parse_chunks(part: str):
    i = 0
    while i < len(part):
        j = i
        while j < len(part) and not part[j].isdigit():
            j += 1
        yield (0, part[i:j])
        i = j
        j = i
        while j < len(part) and part[j].isdigit():
            j += 1
        yield (1, int(part[i:j] or '0'))
        i = j


def deb_char_order(ch: str) -> int:
    if ch == '~':
        return -1
    if ch.isalpha():
        return ord(ch)
    return ord(ch) + 256


def deb_cmp_part(a: str, b: str) -> int:
    for (ka, va), (kb, vb) in itertools.zip_longest(
        parse_chunks(a), parse_chunks(b), fillvalue=(None, None)
    ):
        if ka is None:
            return -1 if not (kb == 0 and vb == '~') else 1
        if kb is None:
            return 1 if not (ka == 0 and va == '~') else -1
        if ka == 1 and kb == 1:
            if va != vb:
                return -1 if va < vb else 1
        else:
            sa = str(va)
            sb = str(vb)
            for ca, cb in itertools.zip_longest(sa, sb, fillvalue=None):
                if ca is None:
                    return 1 if cb == '~' else -1
                if cb is None:
                    return -1 if ca == '~' else 1
                oa, ob = deb_char_order(ca), deb_char_order(cb)
                if oa != ob:
                    return -1 if oa < ob else 1
    return 0


def deb_cmp(a: str, b: str) -> int:
    ua, ra = (a.rsplit('-', 1) + ['0'])[:2] if '-' in a else (a, '0')
    ub, rb = (b.rsplit('-', 1) + ['0'])[:2] if '-' in b else (b, '0')
    c = deb_cmp_part(ua, ub)
    return c if c != 0 else deb_cmp_part(ra, rb)


installed = json.loads(os.environ.get('TEST_INSTALLED_JSON', '{}'))

if name == 'uname':
    print('Linux' if args == ['-s'] else os.environ.get('TEST_ARCH', 'x86_64'))
elif name == 'getconf':
    print(os.environ.get('TEST_LONG_BIT', '64'))
elif name == 'sudo':
    os.execvp(args[0], args)
elif name == 'curl':
    url = next(arg for arg in args if arg.startswith(('https://', 'file://')))
    out = pathlib.Path(args[args.index('--output') + 1])
    if url.startswith('https://pkgs.gandesktop.dev/'):
        rel = url.removeprefix('https://pkgs.gandesktop.dev/')
    elif url.startswith('file://'):
        rel = url.removeprefix('file://')
        source = pathlib.Path(rel)
        if not source.is_file():
            sys.exit(22)
        out.write_bytes(source.read_bytes())
        sys.exit(0)
    else:
        sys.exit(22)
    source = pathlib.Path(os.environ['TEST_REPO']) / rel
    if not source.is_file():
        sys.exit(22)
    out.write_bytes(source.read_bytes())
elif name == 'dpkg':
    if args == ['--print-architecture']:
        print(os.environ.get('TEST_DPKG_ARCH', 'amd64'))
    elif args[0] == '--compare-versions':
        _, v1, op, v2 = args
        c = deb_cmp(v1, v2)
        ok = {'eq': c == 0, 'lt': c < 0, 'le': c <= 0, 'gt': c > 0, 'ge': c >= 0}[op]
        sys.exit(0 if ok else 1)
elif name == 'dpkg-query':
    pkg = args[-1]
    entry = installed.get(pkg)
    if not entry:
        sys.exit(1)
    status = entry.get('status', 'ii ')
    print(f'{status}\t{entry["version"]}')
elif name == 'apt-get':
    marker = os.environ.get('TEST_APT_UPDATED_MARKER')
    if args and args[0] == 'update':
        if marker:
            pathlib.Path(marker).write_text('updated')
        sys.exit(0)
    if args and args[0] == 'install':
        for arg in args:
            if arg.startswith('/'):
                assert pathlib.Path(arg).is_file(), arg
        if os.environ.get('TEST_APT_FAIL_DIRECT') == '1' and marker and not pathlib.Path(marker).exists():
            sys.exit(100)
elif name == 'rpm':
    if args == ['--eval', '%{_arch}']:
        print(os.environ.get('TEST_RPM_ARCH', os.environ.get('TEST_ARCH', 'x86_64')))
    elif args == ['--eval', '%{fedora}']:
        print(os.environ.get('TEST_RPM_FEDORA', '43'))
    elif args[0] == '--eval':
        m = re.search(r"vercmp\('([^']*)',\s*'([^']*)'\)", args[1])
        v1, v2 = m.groups()
        print(deb_cmp(v1, v2))
    elif args[0] == '-q':
        pkg = args[-1]
        entry = installed.get(pkg)
        if not entry:
            print(f'package {pkg} is not installed')
            sys.exit(1)
        print(entry['version'])
elif name == 'dnf':
    if args and args[0] in ('install', 'reinstall', 'downgrade'):
        for arg in args:
            if arg.startswith('/'):
                assert pathlib.Path(arg).is_file(), arg
elif name == 'pacman-conf':
    print(os.environ.get('TEST_PACMAN_ARCH', 'auto'))
elif name == 'vercmp':
    print(deb_cmp(args[0], args[1]))
elif name == 'pacman':
    if args and args[0] == '-Q':
        pkg = args[-1]
        entry = installed.get(pkg)
        if not entry:
            sys.exit(1)
        print(f'{pkg} {entry["version"]}')
    elif args and args[0] == '-U':
        for arg in args:
            if arg.startswith('/'):
                assert pathlib.Path(arg).is_file(), arg
elif name == 'nix':
    if 'profile' in args and 'list' in args:
        print(os.environ.get('TEST_NIX_PROFILE_JSON', '{"elements":{}}'))
elif name == 'gds-files':
    sys.exit(0)
'''


def load_manifest_module():
    spec = importlib.util.spec_from_file_location('gen_manifest', MANIFEST_SCRIPT)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class InstallerTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.repo = self.root / 'repo'
        self.release_dir = self.repo / 'releases' / 'v1.2.3'
        self.channels_dir = self.repo / 'channels'
        self.release_dir.mkdir(parents=True)
        self.channels_dir.mkdir(parents=True)
        (self.channels_dir / 'stable').write_text('v1.2.3\n', encoding='utf-8')
        for name in PACKAGES:
            (self.release_dir / name).write_bytes(name.encode())

        manifest_mod = load_manifest_module()
        manifest_mod.build_manifest_and_sums('v1.2.3', self.release_dir)

        self.apt_lists = self.root / 'apt-lists'
        self.apt_lists.mkdir()
        (self.apt_lists / 'deb.debian.org_Packages').write_text('')
        self.pac_sync = self.root / 'pac-sync'
        self.pac_sync.mkdir()
        (self.pac_sync / 'core.db').write_text('')

        self.os_release = self.root / 'os-release'
        self.debian_version = self.root / 'debian_version'
        self.ostree_marker = self.root / 'ostree-booted'
        self.log = self.root / 'commands.jsonl'

        bindir = self.root / 'bin'
        bindir.mkdir()
        for command in (
            'curl', 'uname', 'getconf', 'sudo', 'pacman', 'pacman-conf', 'vercmp',
            'apt-get', 'dpkg', 'dpkg-query', 'dnf', 'rpm', 'nix', 'gds-files',
        ):
            stub = bindir / command
            stub_body = STUB.removeprefix('#!/usr/bin/env python3\n')
            stub.write_text(f'#!{sys.executable}\n{stub_body}', encoding='utf-8')
            stub.chmod(0o755)

        self.script = INSTALLER.read_text(encoding='utf-8')
        self.env = {
            **os.environ,
            'PATH': f'{bindir}:{os.environ["PATH"]}',
            'TEST_REPO': str(self.repo),
            'TEST_LOG': str(self.log),
            'GDS_OS_RELEASE': str(self.os_release),
            'GDS_DEBIAN_VERSION': str(self.debian_version),
            'GDS_OSTREE_BOOTED': str(self.ostree_marker),
            'GDS_APT_LISTS_DIR': str(self.apt_lists),
            'GDS_PACMAN_SYNC_DIR': str(self.pac_sync),
        }
        self.env.pop('GDS_TTY', None)
        self.env.pop('GDS_NONINTERACTIVE', None)
        self.env.pop('GDS_ALLOW_FILE_URL', None)

    def write_os_release(self, **fields):
        lines = []
        for k, v in fields.items():
            lines.append(f'{k}="{v}"' if ' ' in v else f'{k}={v}')
        self.os_release.write_text('\n'.join(lines) + '\n', encoding='utf-8')

    def run_installer(self, args=(), extra_env=None):
        self.log.unlink(missing_ok=True)
        run_env = dict(self.env)
        if extra_env:
            run_env.update(extra_env)
        return subprocess.run(
            ['bash', '-s', '--', *args],
            input=self.script,
            text=True,
            capture_output=True,
            env=run_env,
        )

    def commands(self, name):
        if not self.log.exists():
            return []
        return [
            entry
            for line in self.log.read_text(encoding='utf-8').splitlines()
            if (entry := json.loads(line))[0] == name
        ]

    def test_manifest_generator_validates_filenames_and_duplicates(self):
        manifest_text = (self.release_dir / 'MANIFEST').read_text(encoding='utf-8')
        self.assertTrue(manifest_text.startswith('# gds-manifest-v1 release=v1.2.3\n'))
        self.assertEqual(len([l for l in manifest_text.splitlines() if not l.startswith('#')]), len(PACKAGES))

        manifest_mod = load_manifest_module()
        with tempfile.TemporaryDirectory() as bad_dir:
            bad_path = Path(bad_dir)
            (bad_path / 'gds-files_1.2.3-1_amd64.deb').write_bytes(b'x')
            with self.assertRaises(ValueError):
                manifest_mod.build_manifest_and_sums('v1.2.3', bad_path)

            manifest_mod.build_manifest_and_sums('v1.2.3', bad_path, ('debian', '13'))
            (bad_path / 'gds-files_1.2.3-1~debian13_amd64.deb').write_bytes(b'y')
            with self.assertRaises(ValueError):
                manifest_mod.build_manifest_and_sums('v1.2.3', bad_path, ('debian', '13'))

    def test_tier1_fresh_install_apt_resilience_and_cleanup(self):
        cases = [
            ('arch', '', 'pacman', PACKAGES[0]),
            ('debian', '13', 'apt-get', PACKAGES[2]),
            ('ubuntu', '24.04', 'apt-get', PACKAGES[4]),
            ('ubuntu', '26.04', 'apt-get', PACKAGES[6]),
            ('fedora', '43', 'dnf', PACKAGES[8]),
            ('fedora', '44', 'dnf', PACKAGES[10]),
        ]
        for distro, ver, manager, expected_pkg in cases:
            with self.subTest(distro=distro, ver=ver):
                self.write_os_release(ID=distro, VERSION_ID=ver)
                result = self.run_installer(['-y'])
                self.assertEqual(result.returncode, 0, result.stderr)
                mgr_cmds = self.commands(manager)
                if manager == 'apt-get':
                    # Cached APT lists exist, so direct install succeeds without running apt-get update.
                    self.assertEqual(mgr_cmds[0][1], 'install')
                paths = [arg for arg in mgr_cmds[-1] if arg.startswith('/')]
                self.assertEqual([Path(p).name for p in paths], [expected_pkg])
                self.assertTrue(all(not Path(p).exists() for p in paths))
                self.assertTrue(any(c == ['gds-files', '--help'] for c in self.commands('gds-files')))

        # When direct apt-get install fails due to stale index, install.sh retries after apt-get update.
        marker = self.root / 'apt-updated'
        self.write_os_release(ID='ubuntu', VERSION_ID='24.04')
        result = self.run_installer(
            ['-y'],
            extra_env={'TEST_APT_FAIL_DIRECT': '1', 'TEST_APT_UPDATED_MARKER': str(marker)},
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual([cmd[1] for cmd in self.commands('apt-get')], ['install', 'update', 'install'])

    def test_derivatives_non_lts_and_manual_override(self):
        cases = [
            ({'ID': 'ubuntu', 'VERSION_ID': '25.10', 'PRETTY_NAME': 'Ubuntu 25.10'}, 'apt-get', PACKAGES[4], {}),
            ({'ID': 'ubuntu', 'VERSION_ID': '26.10', 'PRETTY_NAME': 'Ubuntu 26.10'}, 'apt-get', PACKAGES[6], {}),
            (
                {'ID': 'linuxmint', 'VERSION_ID': '22.1', 'ID_LIKE': 'ubuntu debian', 'UBUNTU_CODENAME': 'noble', 'PRETTY_NAME': 'Linux Mint 22.1'},
                'apt-get',
                PACKAGES[4],
                {},
            ),
            (
                {'ID': 'linuxmint', 'VERSION_ID': '7', 'ID_LIKE': 'debian', 'DEBIAN_CODENAME': 'trixie', 'PRETTY_NAME': 'LMDE 7'},
                'apt-get',
                PACKAGES[2],
                {},
            ),
            (
                {'ID': 'nobara', 'VERSION_ID': '44', 'ID_LIKE': 'fedora', 'PRETTY_NAME': 'Nobara Linux 44'},
                'dnf',
                PACKAGES[10],
                {'TEST_RPM_FEDORA': '44'},
            ),
            (
                {'ID': 'cachyos', 'ID_LIKE': 'arch', 'PRETTY_NAME': 'CachyOS Linux'},
                'pacman',
                PACKAGES[0],
                {},
            ),
        ]
        for os_fields, manager, expected_pkg, extra in cases:
            with self.subTest(os=os_fields['PRETTY_NAME']):
                self.write_os_release(**os_fields)
                result = self.run_installer(['-y'], extra_env=extra)
                self.assertEqual(result.returncode, 0, result.stderr)
                paths = [arg for arg in self.commands(manager)[-1] if arg.startswith('/')]
                self.assertEqual([Path(p).name for p in paths], [expected_pkg])
                self.assertIn('[Tier-2]', result.stdout)

        # Unknown derivative succeeds when --distro escape hatch is passed.
        self.write_os_release(ID='exoticlinux', PRETTY_NAME='Exotic Linux')
        result = self.run_installer(['--distro', 'ubuntu:24.04', '-y'])
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(
            [Path(a).name for a in self.commands('apt-get')[-1] if a.startswith('/')],
            [PACKAGES[4]],
        )

    def test_upgrade_smart_portal_rc_ignored_and_idempotent_skip(self):
        self.write_os_release(ID='ubuntu', VERSION_ID='24.04')

        # rc (config-only) status must be treated as not installed -> fresh install (Files only).
        rc_state = json.dumps({'gds-files': {'status': 'rc ', 'version': '0.0.2-1~ubuntu24.04'}})
        result = self.run_installer(['-y'], extra_env={'TEST_INSTALLED_JSON': rc_state})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('Action : Fresh install', result.stdout)
        self.assertEqual(
            [Path(a).name for a in self.commands('apt-get')[-1] if a.startswith('/')],
            [PACKAGES[4]],
        )

        # Older Portal installed -> auto-included in upgrade unless --without-portal is specified.
        both_old = json.dumps({
            'gds-files': {'status': 'ii ', 'version': '0.0.2-1~ubuntu24.04'},
            'xdg-desktop-portal-gds': {'status': 'ii ', 'version': '0.0.2-1~ubuntu24.04'},
        })
        result = self.run_installer(['-y'], extra_env={'TEST_INSTALLED_JSON': both_old})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('Action : Upgrade', result.stdout)
        self.assertEqual(
            [Path(a).name for a in self.commands('apt-get')[-1] if a.startswith('/')],
            [PACKAGES[4], PACKAGES[5]],
        )

        result = self.run_installer(['-y', '--without-portal'], extra_env={'TEST_INSTALLED_JSON': both_old})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(
            [Path(a).name for a in self.commands('apt-get')[-1] if a.startswith('/')],
            [PACKAGES[4]],
        )

        # Already up to date -> no-op exit 0 unless --reinstall is passed.
        up_to_date = json.dumps({'gds-files': {'status': 'ii ', 'version': '1.2.3-1~ubuntu24.04'}})
        result = self.run_installer(['-y'], extra_env={'TEST_INSTALLED_JSON': up_to_date})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('already up to date', result.stdout)
        self.assertEqual(self.commands('apt-get'), [])

        result = self.run_installer(['-y', '--reinstall'], extra_env={'TEST_INSTALLED_JSON': up_to_date})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('--reinstall', self.commands('apt-get')[-1])

        # Arch -git migration (v0.0.2 -> v1.2.3 non-git) removes conflicting -git package first.
        self.write_os_release(ID='cachyos', ID_LIKE='arch')
        arch_git = json.dumps({'gds-files-git': {'version': '0.0.2.r150.ge9fe328-1'}})
        result = self.run_installer(['-y'], extra_env={'TEST_INSTALLED_JSON': arch_git})
        self.assertEqual(result.returncode, 0, result.stderr)
        pac_cmds = [cmd for cmd in self.commands('pacman') if cmd[1] != '-Q']
        self.assertEqual(pac_cmds[0], ['pacman', '-Rdd', '--noconfirm', 'gds-files-git'])
        self.assertEqual(pac_cmds[1][:3], ['pacman', '-U', '--noconfirm'])

    def test_interactive_tty_selection_and_non_tty_guard(self):
        self.write_os_release(ID='debian', VERSION_ID='13')

        # Without -y or an interactive TTY, abort before running package manager or sudo.
        result = self.run_installer([])
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('Pass -y', result.stderr)
        self.assertEqual(self.commands('apt-get'), [])

        # Simulate interactive user toggling Portal [2] then hitting Enter to confirm.
        tty_file = self.root / 'tty_input'
        tty_file.write_text('2\n\n', encoding='utf-8')
        result = self.run_installer([], extra_env={'GDS_TTY': str(tty_file)})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('Action : Fresh install', result.stdout)
        self.assertEqual(
            [Path(a).name for a in self.commands('apt-get')[-1] if a.startswith('/')],
            [PACKAGES[2], PACKAGES[3]],
        )

        # Pressing 'q' cancels cleanly without touching apt-get or sudo.
        tty_file.write_text('q\n', encoding='utf-8')
        result = self.run_installer([], extra_env={'GDS_TTY': str(tty_file)})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('cancelled', result.stdout.lower())
        self.assertEqual(self.commands('apt-get'), [])

    def test_unsupported_systems_stop_before_download(self):
        # Ubuntu 22.04 / Mint 21 and Debian 12 are below built baselines in MANIFEST.
        for fields in [
            {'ID': 'linuxmint', 'VERSION_ID': '21.3', 'UBUNTU_CODENAME': 'jammy', 'PRETTY_NAME': 'Linux Mint 21.3'},
            {'ID': 'debian', 'VERSION_ID': '12', 'PRETTY_NAME': 'Debian GNU/Linux 12'},
        ]:
            with self.subTest(os=fields['PRETTY_NAME']):
                self.write_os_release(**fields)
                result = self.run_installer(['-y'])
                self.assertNotEqual(result.returncode, 0)
                self.assertIn('older than the minimum supported release', result.stderr)
                self.assertEqual(self.commands('apt-get'), [])

        # OSTree immutable host, 32-bit userland, Manjaro non-interactive, and unknown distro.
        self.write_os_release(ID='fedora', VERSION_ID='43', VARIANT_ID='silverblue')
        self.ostree_marker.write_text('')
        result = self.run_installer(['-y'])
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('OSTree', result.stderr)
        self.assertEqual(self.commands('curl'), [])
        self.ostree_marker.unlink()

        self.write_os_release(ID='debian', VERSION_ID='13')
        result = self.run_installer(['-y'], extra_env={'TEST_LONG_BIT': '32'})
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('32-bit', result.stderr)
        self.assertEqual(self.commands('curl'), [])

        self.write_os_release(ID='manjaro', ID_LIKE='arch')
        result = self.run_installer(['-y'])
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('Manjaro', result.stderr)
        self.assertEqual(self.commands('curl'), [])

    def test_checksum_failure_dry_run_uninstall_and_nix_parity(self):
        self.write_os_release(ID='debian', VERSION_ID='13')

        # Tampered package aborts on SHA-256 mismatch before invoking apt-get.
        pkg_path = self.release_dir / PACKAGES[2]
        orig_bytes = pkg_path.read_bytes()
        pkg_path.write_bytes(b'corrupted')
        result = self.run_installer(['-y'])
        pkg_path.write_bytes(orig_bytes)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('Checksum verification failed', result.stderr)
        self.assertEqual(self.commands('apt-get'), [])

        # --dry-run prints plan and never invokes sudo or apt-get.
        result = self.run_installer(['--dry-run'])
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('Dry run complete', result.stdout)
        self.assertEqual(self.commands('apt-get'), [])

        # --uninstall removes installed packages without downloading anything.
        installed = json.dumps({
            'gds-files': {'status': 'ii ', 'version': '1.2.3-1~debian13'},
            'xdg-desktop-portal-gds': {'status': 'ii ', 'version': '1.2.3-1~debian13'},
        })
        result = self.run_installer(['--uninstall', '-y'], extra_env={'TEST_INSTALLED_JSON': installed})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.commands('curl'), [])
        self.assertEqual(self.commands('apt-get'), [['apt-get', 'remove', '-y', 'gds-files', 'xdg-desktop-portal-gds']])

        # NixOS & --nix resolve channels/stable (v1.2.3) and replace older profile entries cleanly.
        self.write_os_release(ID='nixos', VERSION_ID='26.05')
        old_nix = json.dumps({
            'elements': {
                'gds-files': {
                    'active': True,
                    'storePaths': ['/nix/store/00000000000000000000000000000000-gds-files-0.0.2'],
                }
            }
        })
        result = self.run_installer(
            ['-y', '--with-portal'],
            extra_env={'TEST_ARCH': 'aarch64', 'TEST_NIX_PROFILE_JSON': old_nix},
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        nix_cmds = [cmd[3:] for cmd in self.commands('nix') if cmd[3:5] != ['profile', 'list']]
        self.assertEqual(
            nix_cmds[0],
            ['profile', 'remove', '/nix/store/00000000000000000000000000000000-gds-files-0.0.2'],
        )
        self.assertEqual(
            nix_cmds[1],
            [
                'profile',
                'add',
                'github:GanDesktop/gds/v1.2.3#gds-files',
                'github:GanDesktop/gds/v1.2.3#xdg-desktop-portal-gds',
            ],
        )


if __name__ == '__main__':
    unittest.main()
