"""libvirt input, SSH fixtures, AT-SPI observation, and bounded expectations."""

from contextlib import contextmanager
from datetime import datetime, timezone
import base64
import hashlib
import io
import json
from pathlib import Path
import shlex
import subprocess
import sys
import time

from PIL import Image, ImageChops

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'scripts'))
import vm  # noqa: E402


class Blocked(RuntimeError):
    """The required desktop environment could not be prepared."""


def command(argv, *, input=None, timeout=30):
    result = subprocess.run(argv, input=input, capture_output=True, text=True, timeout=timeout)
    if result.returncode:
        raise RuntimeError(f'{shlex.join(argv)} exited {result.returncode}: {result.stderr.strip()}')
    return result.stdout


def eventually(observe, accept, description, timeout=15):
    """Poll observations only. Never retry input or a failed workflow."""
    deadline = time.monotonic() + timeout
    last = None
    while True:
        last = observe()
        if accept(last):
            return last
        if time.monotonic() >= deadline:
            raise AssertionError(f'Timed out: {description}; last observation: {str(last)[:500]}')
        time.sleep(0.1)


class Desktop:
    def __init__(self, name, artifacts, bundle, guest_bin_dir=None):
        if name == 'freebsd' and not guest_bin_dir:
            raise Blocked('FreeBSD needs a native Files build and --guest-bin-dir')
        self.name = name
        self.domain = vm.MACHINES[name]
        self.artifacts = artifacts
        self.remote = None
        self.original_a11y = None
        try:
            vm.start_machine(self.domain)
            address = eventually(lambda: vm.ip_address(self.domain), bool, 'DHCP address', 60)
            self.remote = f'atlas@{address}'
            self.ssh = ['ssh', '-o', 'BatchMode=yes', '-o', 'ConnectTimeout=8',
                        '-o', 'StrictHostKeyChecking=accept-new', self.remote]
            self.base = f'/tmp/gds-vm-e2e/{artifacts.parent.name}/{name}'
            command([*self.ssh, shlex.join(['mkdir', '-p', self.base])])
            self.script = self.base + '/guest.py'
            command(['scp', '-q', '-o', 'BatchMode=yes', str(Path(__file__).with_name('guest.py')),
                     f'{self.remote}:{self.script}'])
            self.environment = self.call('environment')
            self.environment['screenshot_backend'] = 'grim' if name == 'noctalia' else 'libvirt'
            if self.environment['uid'] == 0:
                raise Blocked('Workflows must run as an unprivileged desktop user')
            if self.environment['files_already_running']:
                raise Blocked('A Files instance already owns the desktop session; close it before testing')
            if not self.environment['desktop']['WAYLAND_DISPLAY']:
                raise Blocked('No Wayland desktop session; X11 needs its own capability profile')
            self.original_a11y = self.call('session')
            # Unlocking does not wake a DPMS-suspended output on Plasma.
            self.key('shift')
            self.screenshot(artifacts / 'desktop.png')
            if guest_bin_dir:
                self.bin = guest_bin_dir
                self.build = {'kind': 'installed', 'directory': self.bin}
                apps = ('files',) if name == 'freebsd' else ('files', 'images')
                suffix = ''
            else:
                if not bundle.is_file():
                    raise Blocked(f'Missing {bundle}; run just e2e-build')
                with bundle.open('rb') as source:
                    digest = hashlib.file_digest(source, 'sha256').hexdigest()
                self.bin = f'/var/tmp/gds-vm-e2e/bundles/{digest}/bin'
                parent = str(Path(self.bin).parent)
                present = command([*self.ssh, f'test -f {shlex.quote(parent + "/ready")} && echo yes || true'])
                if present.strip() != 'yes':
                    command([*self.ssh, shlex.join(['mkdir', '-p', parent])])
                    command(['scp', '-q', '-o', 'BatchMode=yes', str(bundle),
                             f'{self.remote}:{parent}/bundle.tar.gz'], timeout=180)
                    command([*self.ssh,
                             f'tar -xzf {shlex.quote(parent + "/bundle.tar.gz")} -C {shlex.quote(parent)} && '
                             f'if test -d {shlex.quote(parent + "/nix/store")}; then '
                             'sudo -n mkdir -p /nix/store && '
                             f'sudo -n cp -an {shlex.quote(parent + "/nix/store/.")} /nix/store/; fi && '
                             f'touch {shlex.quote(parent + "/ready")}'], timeout=180)
                self.build = json.loads(self.read(parent + '/manifest.json'))
                apps = self.build['binaries']
                suffix = '.elf'
            self.environment['binaries'] = {
                app: self.call('sha256', path=f'{self.bin}/gds-{app}{suffix}')
                for app in apps
            }
            if not guest_bin_dir and self.environment['binaries'] != self.build['binaries']:
                raise Blocked('Deployed binary hashes differ from build manifest')
            (artifacts / 'environment.json').write_text(json.dumps(self.environment, indent=2))
            (artifacts / 'build.json').write_text(json.dumps(self.build, indent=2))
        except Exception as error:
            self.close()
            raise Blocked(f'{name}: {error}') from error

    def call(self, op, **args):
        proc = subprocess.run([*self.ssh, shlex.join(['python3', self.script])],
                              input=json.dumps({'op': op, 'args': args}), text=True,
                              capture_output=True, timeout=25)
        if not proc.stdout.strip():
            raise RuntimeError(f'SSH {op}: {proc.stderr.strip()}')
        response = json.loads(proc.stdout)
        if not response['ok']:
            raise RuntimeError(response['error'])
        return response['result']

    def read(self, path):
        return base64.b64decode(self.call('read', path=path))

    def screenshot(self, destination):
        if self.environment['screenshot_backend'] == 'grim':
            destination.write_bytes(base64.b64decode(self.call('screenshot')))
        else:
            command([*vm.VIRSH, 'screenshot', self.domain, str(destination)])

    def qmp(self, events):
        result = json.loads(command([*vm.VIRSH, 'qemu-monitor-command', self.domain,
                                     json.dumps({'execute': 'input-send-event', 'arguments': {'events': events}})]))
        if 'error' in result:
            raise RuntimeError(result['error'])

    def key(self, stroke):
        aliases = {'enter': 'ret', 'escape': 'esc', 'delete': 'delete', 'space': 'spc',
                   'backspace': 'backspace', 'pageup': 'pgup', 'pagedown': 'pgdn'}
        codes = [aliases.get(part, part) for part in stroke.split('+')]
        self.qmp([{'type': 'key', 'data': {'down': down, 'key': {'type': 'qcode', 'data': code}}}
                  for down, order in ((True, codes), (False, list(reversed(codes)))) for code in order])

    def type_text(self, text):
        normal = {' ': 'spc', '/': 'slash', '.': 'dot', '-': 'minus', '=': 'equal',
                  ',': 'comma', ';': 'semicolon', "'": 'apostrophe', '[': 'bracket_left',
                  ']': 'bracket_right', '\\': 'backslash', '`': 'grave', '\n': 'ret'}
        shifted = {'_': 'minus', ':': 'semicolon', '?': 'slash', '+': 'equal',
                   '!': '1', '@': '2', '#': '3', '$': '4', '%': '5', '^': '6',
                   '&': '7', '*': '8', '(': '9', ')': '0', '"': 'apostrophe',
                   '>': 'dot', '<': 'comma', '|': 'backslash', '{': 'bracket_left', '}': 'bracket_right'}
        for character in text:
            if character in shifted:
                self.key('shift+' + shifted[character])
            elif character.isascii() and character.isupper():
                self.key('shift+' + character.lower())
            elif character in normal:
                self.key(normal[character])
            elif character.isascii() and character.isalnum():
                self.key(character)
            else:
                raise ValueError('QEMU text input uses the guest US keymap; use Unicode fixture paths via argv')

    def pointer(self, x, y, screen_size, button=None):
        width, height = screen_size
        events = [
            {'type': 'abs', 'data': {'axis': 'x', 'value': round(x * 32767 / (width - 1))}},
            {'type': 'abs', 'data': {'axis': 'y', 'value': round(y * 32767 / (height - 1))}},
        ]
        if button is not None:
            events.append({'type': 'btn', 'data': {'down': button, 'button': 'left'}})
        self.qmp(events)

    def close(self):
        if self.original_a11y is not None:
            self.call('session', enabled=self.original_a11y)


class Workflow:
    def __init__(self, desktop, case_id, artifacts):
        self.desktop = desktop
        self.id = case_id
        self.artifacts = artifacts
        self.root = f'{desktop.base}/{case_id}'
        self.work = self.root + '/work'
        self.processes = []
        self.pid = None
        self.label = None
        self.last_step = 'fixture initialization'
        self.desktop.call('init', root=self.root)
        self.events = artifacts / 'steps.jsonl'

    @contextmanager
    def step(self, description):
        self.last_step = description
        started = time.monotonic()
        self.log({'step': description, 'event': 'start'})
        try:
            yield
        except Exception as error:
            self.log({'step': description, 'event': 'failed', 'error': str(error),
                      'seconds': time.monotonic() - started})
            raise
        else:
            self.log({'step': description, 'event': 'passed', 'seconds': time.monotonic() - started})

    def log(self, event):
        with self.events.open('a') as out:
            out.write(json.dumps({'time': datetime.now(timezone.utc).isoformat(), **event}) + '\n')

    def path(self, relative):
        return self.work + '/' + relative

    def write(self, relative, content):
        data = content.encode() if isinstance(content, str) else content
        self.desktop.call('write', path=self.path(relative), data=base64.b64encode(data).decode())

    def mkdir(self, relative):
        self.desktop.call('mkdir', path=self.path(relative))

    def sample_image(self, relative, size=(80, 48)):
        image = Image.new('RGBA', size, '#e05a28')
        image.paste('#285ae0', (0, 0, size[0] // 3, size[1] // 2))
        output = io.BytesIO()
        image.save(output, format='PNG')
        self.write(relative, output.getvalue())
        return image

    def coordinate_image(self, relative):
        image = Image.new('RGBA', (320, 200))
        image.putdata([(x % 256, y, 123 + x // 256, 255) for y in range(200) for x in range(320)])
        output = io.BytesIO()
        image.save(output, format='PNG')
        self.write(relative, output.getvalue())
        return image

    def image_dimensions(self):
        import re
        facts = self.expect(contains='×', role='label')
        sizes = {tuple(map(int, match.groups())) for node in facts
                 if (match := re.search(r'(\d+) × (\d+)', node['name']))}
        assert len(sizes) == 1, facts
        return sizes.pop()

    def launch(self, app, *args, stdin=None):
        with self.step(f'Launch {app} {args}'):
            label = f'{app}-{len(self.processes)}'
            process = self.desktop.call('launch', root=self.root, label=label,
                                        argv=[self.desktop.bin + f'/gds-{app}', *args], stdin=stdin)
            self.processes.append(process)
            self.pid, self.label = process['pid'], label
            eventually(self.tree, lambda tree: any(n['role'] == 'frame' and 'active' in n['states'] for n in tree),
                       f'{app} active native window', timeout=30)
        return self

    def tree(self):
        if not self.desktop.call('alive', pid=self.pid):
            raise AssertionError(f'Application exited: {self.label}; see logs')
        return self.desktop.call('snapshot', pid=self.pid)

    def expect(self, name=None, role=None, *, contains=None, present=True):
        with self.step(f'Expect UI {name or contains or role!r}, present={present}'):
            def matches(tree):
                return [n for n in tree if 'showing' in n['states'] and (name is None or n['name'] == name)
                        and (role is None or n['role'] == role)
                        and (contains is None or contains in n['name'])]
            tree = eventually(self.tree, lambda t: bool(matches(t)) == present,
                              f'UI name={name} role={role} contains={contains} present={present}')
            return matches(tree)

    def activate(self, name, role=None, *, description=None, focus=False):
        with self.step(f'{"Focus" if focus else "Activate"} {name}'):
            self.expect(name, role)
            query = {'name': name}
            if role:
                query['role_name'] = role
            if description:
                query['description'] = description
            assert self.desktop.call('action', pid=self.pid, query=query, focus=focus), query

    def expect_state(self, name, state, enabled=True):
        with self.step(f'Expect {name}: {state}={enabled}'):
            eventually(self.tree, lambda tree: any(n['name'] == name and (state in n['states']) == enabled for n in tree),
                       f'{name}: {state}={enabled}')

    def select(self, filename):
        self.activate(Path(filename).name, description=self.path(filename))
        self.expect_state(Path(filename).name, 'selected')

    def key(self, stroke):
        with self.step(f'Key {stroke}'):
            self.desktop.key(stroke)

    def type(self, text):
        with self.step(f'Type {text!r}'):
            self.desktop.type_text(text)

    def navigate(self, relative):
        self.key('ctrl+l')
        self.expect_entry()
        self.key('ctrl+a')
        self.type(self.path(relative))
        self.key('enter')

    def expect_entry(self):
        with self.step('Wait for focused text entry'):
            eventually(self.tree, lambda tree: any(n['role'] == 'entry' and 'focused' in n['states'] for n in tree),
                       'focused text entry')

    def expect_file(self, relative, content=None, *, exists=True):
        with self.step(f'Expect file {relative}, exists={exists}'):
            path = self.path(relative)
            eventually(lambda: self.desktop.call('exists', path=path), lambda value: value == exists,
                       f'{path} exists={exists}')
            if exists and content is not None:
                expected = content.encode() if isinstance(content, str) else content
                eventually(lambda: self.desktop.read(path), lambda value: value == expected,
                           f'{path} content equals fixture')

    def expect_image(self, relative, expected):
        with self.step(f'Expect decoded pixels of {relative}'):
            digest = hashlib.sha256(expected.convert('RGBA').tobytes()).hexdigest()
            self.expect_file(relative)
            eventually(lambda: self.desktop.call('image', path=self.path(relative)),
                       lambda info: info['size'] == list(expected.size) and info['pixels'] == digest,
                       f'{relative}: dimensions {expected.size}, RGBA sha256 {digest}')

    def expect_directory(self, relative):
        with self.step(f'Expect directory {relative}'):
            eventually(lambda: self.desktop.call('is_dir', path=self.path(relative)), bool,
                       f'{relative} is a directory')

    def expect_empty_trash(self):
        with self.step('Expect no remaining trash payload or metadata'):
            manifest = self.desktop.call('files', root=self.root + '/data/Trash')
            assert not [path for path, entry in manifest.items() if 'sha256' in entry or 'link' in entry], manifest

    def image_target(self, expected):
        """Locate a deterministic 1:1 image fixture, independent of Wayland window origin.

        This matches image content, not UI colors, layout coordinates or a golden window.
        A thumbnail cannot match the complete full-size fixture.
        """
        with self.step('Locate the original image pixels on the VM display'):
            self.key('ctrl+1')
            reference = expected.convert('RGB')
            corner = reference.getpixel((0, 0))
            path = self.artifacts / 'pointer-target.png'

            def observe():
                self.desktop.screenshot(path)
                screen = Image.open(path).convert('RGB')
                pixels = screen.load()
                matches = []
                for y in range(screen.height - reference.height + 1):
                    for x in range(screen.width - reference.width + 1):
                        if pixels[x, y] != corner or (x and pixels[x - 1, y] == corner) or (y and pixels[x, y - 1] == corner):
                            continue
                        region = screen.crop((x, y, x + reference.width, y + reference.height))
                        if ImageChops.difference(region, reference).getbbox() is None:
                            matches.append({'x': x, 'y': y, 'screen': screen.size})
                return matches

            matches = eventually(observe, lambda result: len(result) == 1, 'unique full-size image fixture')
            target = matches[0]
            target['name'] = self.expect(role='image')[0]['name']
            (self.artifacts / 'pointer-target.json').write_text(json.dumps(target))
            return target

    def drag_image(self, target, start, end):
        with self.step(f'Drag on image from {start} to {end}'):
            # Wait for the pointer press to focus the canvas before sending motion.
            # Otherwise the compositor may coalesce motion before zgui begins the drag.
            self.activate('Copy Image', focus=True)
            self.expect_state('Copy Image', 'focused')
            self.desktop.pointer(target['x'] + start[0], target['y'] + start[1], target['screen'])
            self.desktop.pointer(target['x'] + start[0], target['y'] + start[1], target['screen'], True)
            self.expect_state(target['name'], 'focused')
            for fraction in range(1, 11):
                x = target['x'] + start[0] + (end[0] - start[0]) * fraction / 10
                y = target['y'] + start[1] + (end[1] - start[1]) * fraction / 10
                self.desktop.pointer(x, y, target['screen'])
            self.desktop.pointer(target['x'] + end[0], target['y'] + end[1], target['screen'], False)

    def checkpoint(self, name):
        self.artifacts.mkdir(parents=True, exist_ok=True)
        self.desktop.screenshot(self.artifacts / f'{name}.png')
        if self.pid and self.desktop.call('alive', pid=self.pid):
            (self.artifacts / f'{name}.ui.json').write_text(json.dumps(self.tree(), indent=2))
        (self.artifacts / f'{name}.files.json').write_text(json.dumps(
            self.desktop.call('files', root=self.root), indent=2))

    def quit(self):
        self.activate('Close window', 'button')
        with self.step('Wait for normal process exit'):
            eventually(lambda: self.desktop.call('alive', pid=self.pid), lambda alive: not alive,
                       'application shutdown')
            self.expect_exit(0)

    def expect_exit(self, code):
        path = self.root + f'/logs/{self.label}.exit.json'
        eventually(lambda: self.desktop.call('exists', path=path), bool, 'process exit status')
        assert json.loads(self.desktop.read(path))['code'] == code

    def collect(self):
        command(['scp', '-q', '-r', '-o', 'BatchMode=yes', f'{self.desktop.remote}:{self.root}/logs',
                 str(self.artifacts)])
        command([*self.desktop.ssh, shlex.join(['tar', '-czf', self.root + '.tar.gz', '-C', self.root, '.'])])
        command(['scp', '-q', '-o', 'BatchMode=yes', f'{self.desktop.remote}:{self.root}.tar.gz',
                 str(self.artifacts / 'guest-state.tar.gz')])

    def close(self):
        for process in self.processes:
            self.desktop.call('stop', **process)
            eventually(lambda: self.desktop.call('alive', pid=process['pid']), lambda alive: not alive,
                       'owned process cleanup', timeout=10)
