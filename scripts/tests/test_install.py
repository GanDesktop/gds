"""Exercise the piped installer with local release fixtures and mocked system tools."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


INSTALLER = Path(__file__).resolve().parents[1] / 'install.sh'
PACKAGES = [
    'gds-files-git-1.2.3.r1.gabcd-1-x86_64.pkg.tar.zst',
    'xdg-desktop-portal-gds-git-1.2.3.r1.gabcd-1-x86_64.pkg.tar.zst',
    'gds-files_1.2.3-1_amd64.deb',
    'xdg-desktop-portal-gds_1.2.3-1_amd64.deb',
    'gds-files-1.2.3-1.fc43.x86_64.rpm',
    'xdg-desktop-portal-gds-1.2.3-1.fc43.x86_64.rpm',
]
STUB = r'''#!/usr/bin/env python3
import json, os, pathlib, sys
name = pathlib.Path(sys.argv[0]).name
args = sys.argv[1:]
with open(os.environ['TEST_LOG'], 'a') as log:
    log.write(json.dumps([name, *args]) + '\n')
if name == 'uname':
    print('Linux' if args == ['-s'] else os.environ.get('TEST_ARCH', 'x86_64'))
elif name == 'sudo':
    os.execvp(args[0], args)
elif name == 'curl':
    if '--write-out' in args:
        print('https://github.com/GanDesktop/gds/releases/tag/v1.2.3', end='')
    else:
        url = next(arg for arg in args if arg.startswith('https://'))
        source = pathlib.Path(os.environ['TEST_ASSETS']) / url.rsplit('/', 1)[-1]
        if not source.is_file():
            sys.exit(22)
        pathlib.Path(args[args.index('--output') + 1]).write_bytes(source.read_bytes())
elif name in ('pacman', 'apt-get', 'dnf'):
    for arg in args:
        if arg.startswith('/'):
            assert pathlib.Path(arg).is_file(), arg
'''


class InstallerTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.assets = self.root / 'assets'
        self.assets.mkdir()
        self.log = self.root / 'commands.jsonl'
        checksums = []
        for name in PACKAGES:
            content = name.encode()
            (self.assets / name).write_bytes(content)
            checksums.append(f'{hashlib.sha256(content).hexdigest()}  {name}\n')
        (self.assets / 'SHA256SUMS').write_text(''.join(checksums))
        bindir = self.root / 'bin'
        bindir.mkdir()
        for command in ('curl', 'uname', 'sudo', 'pacman', 'apt-get', 'dnf', 'nix'):
            stub = bindir / command
            stub.write_text(STUB)
            stub.chmod(0o755)
        self.env = dict(os.environ, PATH=f'{bindir}:{os.environ["PATH"]}',
                        TEST_ASSETS=str(self.assets), TEST_LOG=str(self.log),
                        GDS_TEST_OS_RELEASE=str(self.root / 'os-release'))
        self.script = INSTALLER.read_text().replace('. /etc/os-release', '. "$GDS_TEST_OS_RELEASE"')

    def run_installer(self, distro='debian', version='13', args=(), architecture='x86_64'):
        (self.root / 'os-release').write_text(f'ID={distro}\nVERSION_ID={version}\n')
        self.env['TEST_ARCH'] = architecture
        return subprocess.run(['bash', '-s', '--', *args], input=self.script,
                              text=True, capture_output=True, env=self.env)

    def commands(self, name):
        if not self.log.exists():
            return []
        return [entry for line in self.log.read_text().splitlines()
                if (entry := json.loads(line))[0] == name]

    def test_native_systems_install_only_files_and_cleanup(self):
        for distro, version, manager, index in [('arch', '', 'pacman', 0),
                                                ('debian', '13', 'apt-get', 2),
                                                ('fedora', '43', 'dnf', 4)]:
            with self.subTest(distro=distro):
                self.log.unlink(missing_ok=True)
                result = self.run_installer(distro, version)
                self.assertEqual(result.returncode, 0, result.stderr)
                paths = [arg for arg in self.commands(manager)[-1] if arg.startswith('/')]
                self.assertEqual([Path(arg).name for arg in paths], [PACKAGES[index]])
                self.assertTrue(all(not Path(arg).exists() for arg in paths))

    def test_portal_and_explicit_release(self):
        result = self.run_installer(args=['--with-portal', '--version', 'v1.2.3-rc1'])
        self.assertEqual(result.returncode, 0, result.stderr)
        paths = [arg for arg in self.commands('apt-get')[-1] if arg.startswith('/')]
        self.assertEqual([Path(arg).name for arg in paths], PACKAGES[2:4])
        self.assertTrue(all('/download/v1.2.3-rc1/' in ' '.join(cmd) for cmd in self.commands('curl')))

    def test_corrupt_download_never_installs(self):
        (self.assets / PACKAGES[2]).write_bytes(b'corrupt')
        result = self.run_installer()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('Checksum verification failed', result.stderr)
        self.assertEqual(self.commands('apt-get'), [])

    def test_unfinished_release_never_installs(self):
        (self.assets / 'SHA256SUMS').unlink()
        result = self.run_installer()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('may still be building', result.stderr)
        self.assertEqual(self.commands('apt-get'), [])

    def test_missing_package_never_installs(self):
        (self.assets / 'SHA256SUMS').write_text('')
        result = self.run_installer()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('Expected one package', result.stderr)
        self.assertEqual(self.commands('apt-get'), [])

    def test_unsupported_systems_stop_before_download(self):
        for distro, version, arch in [('ubuntu', '24.04', 'x86_64'),
                                      ('debian', '12', 'x86_64'),
                                      ('fedora', '44', 'x86_64'),
                                      ('arch', '', 'aarch64')]:
            with self.subTest(distro=distro, version=version, arch=arch):
                result = self.run_installer(distro, version, architecture=arch)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(self.commands('curl'), [])

    def test_nixos_and_explicit_nix(self):
        for distro, args in [('nixos', ['--with-portal', '--version', 'v1.2.3']),
                              ('debian', ['--nix', '--with-portal', '--version', 'v1.2.3'])]:
            with self.subTest(distro=distro):
                result = self.run_installer(distro, '13', args, architecture='aarch64')
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(self.commands('curl'), [])
                self.assertEqual(self.commands('nix')[-1][3:5], ['profile', 'add'])
                self.assertEqual(self.commands('nix')[-1][-2:], [
                    'github:GanDesktop/gds/v1.2.3#gds-files',
                    'github:GanDesktop/gds/v1.2.3#xdg-desktop-portal-gds'])

    def test_invalid_arguments(self):
        for args in [['--version'], ['--version', '../bad'], ['--unknown']]:
            with self.subTest(args=args):
                result = self.run_installer(args=args)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(self.commands('curl'), [])


if __name__ == '__main__':
    unittest.main()
