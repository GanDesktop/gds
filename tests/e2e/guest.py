"""Small SSH endpoint for OS fixtures and AT-SPI. Never calls GDS internals."""

import base64
import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import time


def desktop_environment():
    env = os.environ.copy()
    for line in subprocess.check_output(['systemctl', '--user', 'show-environment'], text=True).splitlines():
        key, _, value = line.partition('=')
        env[key] = value
    env['XDG_RUNTIME_DIR'] = f'/run/user/{os.getuid()}'
    env['DBUS_SESSION_BUS_ADDRESS'] = f'unix:path={env["XDG_RUNTIME_DIR"]}/bus'
    return env


def atspi():
    import gi
    gi.require_version('Atspi', '2.0')
    from gi.repository import Atspi
    Atspi.set_timeout(1500, 3000)
    return Atspi


def application(pid):
    spi = atspi()
    desktop = spi.get_desktop(0)
    for index in range(desktop.get_child_count()):
        app = desktop.get_child_at_index(index)
        if app.get_process_id() == pid:
            return app
    return None


def walk(node):
    if node is None:
        return
    yield node
    for index in range(node.get_child_count()):
        yield from walk(node.get_child_at_index(index))


def snapshot(pid):
    spi = atspi()
    nodes = []
    for node in walk(application(pid)):
        component = node.get_component_iface()
        bounds = component.get_extents(spi.CoordType.WINDOW) if component else None
        text = node.get_text_iface()
        nodes.append({
            'role': node.get_role_name(), 'name': node.get_name(),
            'description': node.get_description(),
            'text': spi.Text.get_text(node, 0, -1) if text else '',
            'states': [state.value_nick for state in node.get_state_set().get_states()],
            'bounds': [bounds.x, bounds.y, bounds.width, bounds.height] if bounds else None,
        })
    return nodes


def find(pid, query):
    matches = [node for node in walk(application(pid))
               if all(getattr(node, f'get_{key}')() == value for key, value in query.items())]
    if len(matches) != 1:
        raise RuntimeError(f'Expected one accessible {query}, found {len(matches)}')
    return matches[0]


def action(args):
    node = find(args['pid'], args['query'])
    if args.get('focus'):
        return node.get_component_iface().grab_focus()
    # A file label is inside an actionable table row. Resolve by semantics, not row index.
    while node:
        interface = node.get_action_iface()
        if interface and interface.get_n_actions():
            return interface.do_action(0)
        node = node.get_parent()
    raise RuntimeError(f'No accessible action for {args["query"]}')


def initialize(root):
    for name in ('home', 'config', 'data', 'cache', 'state', 'work', 'logs'):
        (root / name).mkdir(parents=True, exist_ok=True)


def supervise(args):
    root = Path(args['root'])
    initialize(root)
    env = desktop_environment()
    env.update(HOME=str(root / 'home'), XDG_CONFIG_HOME=str(root / 'config'),
               XDG_DATA_HOME=str(root / 'data'), XDG_CACHE_HOME=str(root / 'cache'),
               XDG_STATE_HOME=str(root / 'state'), LANG='en_US.UTF-8', LANGUAGE='en',
               LC_ALL='en_US.UTF-8', LIBGL_ALWAYS_SOFTWARE='1')
    # Prevent inheriting an unrelated shell's DISPLAY or Nix driver override.
    env.pop('VK_DRIVER_FILES', None)
    env.pop('VK_ICD_FILENAMES', None)
    stem = root / 'logs' / args['label']
    stdin = open(args['stdin'], 'rb') if args.get('stdin') else subprocess.DEVNULL
    with stem.with_suffix('.stdout').open('wb') as out, stem.with_suffix('.stderr').open('wb') as err:
        process = subprocess.Popen(args['argv'], env=env, cwd=root / 'work', stdin=stdin,
                                   stdout=out, stderr=err, start_new_session=True)
    identity = {'pid': process.pid, 'start': Path(f'/proc/{process.pid}/stat').read_text().split()[21]}
    temporary = stem.with_suffix('.pid.tmp')
    temporary.write_text(json.dumps(identity))
    temporary.replace(stem.with_suffix('.pid.json'))
    code = process.wait()
    stem.with_suffix('.exit.json').write_text(json.dumps({'code': code}))


def launch(args):
    root = Path(args['root'])
    initialize(root)
    stem = root / 'logs' / args['label']
    spec = stem.with_suffix('.launch.json')
    spec.write_text(json.dumps(args))
    with stem.with_suffix('.supervisor.log').open('wb') as log:
        monitor = subprocess.Popen([sys.executable, __file__, '--supervise', str(spec)],
                                   stdin=subprocess.DEVNULL, stdout=log, stderr=log, start_new_session=True)
    deadline = time.monotonic() + 10
    identity = stem.with_suffix('.pid.json')
    while not identity.exists():
        if monitor.poll() is not None or time.monotonic() > deadline:
            raise RuntimeError(stem.with_suffix('.supervisor.log').read_text())
        time.sleep(0.02)
    return json.loads(identity.read_text())


def stop(args):
    pid = args['pid']
    stat = Path(f'/proc/{pid}/stat')
    if stat.exists() and stat.read_text().split()[21] == args['start']:
        os.killpg(pid, signal.SIGTERM)
    return True


def files(root):
    result = {}
    for path in sorted(root.rglob('*')):
        item = {'mode': oct(path.lstat().st_mode)}
        if path.is_symlink():
            item['link'] = os.readlink(path)
        elif path.is_file():
            with path.open('rb') as source:
                item.update(size=path.stat().st_size, sha256=hashlib.file_digest(source, 'sha256').hexdigest())
        result[str(path.relative_to(root))] = item
    return result


def image_info(path):
    from PIL import Image
    with Image.open(path) as source:
        rgba = source.convert('RGBA')
        return {'size': list(source.size), 'format': source.format,
                'pixels': hashlib.sha256(rgba.tobytes()).hexdigest()}


def execute(op, args):
    if op == 'environment':
        from PIL import __version__ as pillow_version
        spi = atspi()
        env = desktop_environment()
        import gi
        gi.require_version('Gio', '2.0')
        from gi.repository import Gio, GLib
        bus = Gio.bus_get_sync(Gio.BusType.SESSION, None)
        files_running = bus.call_sync('org.freedesktop.DBus', '/org/freedesktop/DBus',
                                      'org.freedesktop.DBus', 'NameHasOwner',
                                      GLib.Variant('(s)', ('io.github.GanDesktop.GdsFiles',)),
                                      None, Gio.DBusCallFlags.NONE, 5000, None).unpack()[0]
        return {'os': Path('/etc/os-release').read_text(), 'kernel': os.uname().release,
                'files_already_running': files_running,
                'uid': os.getuid(), 'desktop': {k: env.get(k) for k in (
                    'WAYLAND_DISPLAY', 'DISPLAY', 'XDG_CURRENT_DESKTOP', 'XDG_SESSION_TYPE')},
                'atspi': spi.get_version(), 'pillow': pillow_version}
    if op == 'session':
        env = desktop_environment()
        os.environ.update(env)
        import gi
        gi.require_version('Gio', '2.0')
        from gi.repository import Gio, GLib
        bus = Gio.bus_get_sync(Gio.BusType.SESSION, None)
        old = bus.call_sync('org.a11y.Bus', '/org/a11y/bus', 'org.freedesktop.DBus.Properties',
                            'Get', GLib.Variant('(ss)', ('org.a11y.Status', 'IsEnabled')),
                            None, Gio.DBusCallFlags.NONE, 5000, None).unpack()[0]
        enabled = args.get('enabled', True)
        bus.call_sync('org.a11y.Bus', '/org/a11y/bus', 'org.freedesktop.DBus.Properties',
                      'Set', GLib.Variant('(ssv)', ('org.a11y.Status', 'IsEnabled', GLib.Variant('b', enabled))),
                      None, Gio.DBusCallFlags.NONE, 5000, None)
        if enabled:
            subprocess.run(['sudo', '-n', 'loginctl', 'unlock-sessions'], check=True, capture_output=True)
            if shutil.which('umbriel'):
                subprocess.run(['umbriel', 'msg', 'overview-close'], env=env, capture_output=True)
        return old
    if op == 'init':
        initialize(Path(args['root']))
        return True
    if op == 'launch':
        return launch(args)
    if op == 'stop':
        return stop(args)
    if op == 'alive':
        stat = Path(f'/proc/{args["pid"]}/stat')
        return stat.exists() and stat.read_text().split()[2] != 'Z'
    if op == 'snapshot':
        return snapshot(args['pid'])
    if op == 'action':
        return action(args)
    if op == 'write':
        path = Path(args['path'])
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(base64.b64decode(args['data']))
        return True
    if op == 'mkdir':
        Path(args['path']).mkdir(parents=True, exist_ok=True)
        return True
    if op == 'read':
        return base64.b64encode(Path(args['path']).read_bytes()).decode()
    if op == 'exists':
        return Path(args['path']).exists()
    if op == 'is_dir':
        return Path(args['path']).is_dir()
    if op == 'unlink':
        Path(args['path']).unlink()
        return True
    if op == 'chmod':
        Path(args['path']).chmod(args['mode'])
        return True
    if op == 'files':
        return files(Path(args['root']))
    if op == 'image':
        return image_info(args['path'])
    if op == 'clipboard':
        data = subprocess.check_output(['wl-paste', '--no-newline', '--type', args['mime']],
                                       env=desktop_environment(), timeout=10)
        return base64.b64encode(data).decode()
    if op == 'screenshot':
        data = subprocess.check_output(['grim', '-'], env=desktop_environment(), timeout=15)
        return base64.b64encode(data).decode()
    raise ValueError(f'Unknown operation: {op}')


if __name__ == '__main__':
    if len(sys.argv) > 1 and sys.argv[1] == '--supervise':
        supervise(json.loads(Path(sys.argv[2]).read_text()))
        sys.exit(0)
    try:
        request = json.load(sys.stdin)
        print(json.dumps({'ok': True, 'result': execute(request['op'], request.get('args', {}))}))
    except Exception as error:
        print(json.dumps({'ok': False, 'error': f'{type(error).__name__}: {error}'}))
        sys.exit(1)
