#!/usr/bin/env python3
"""Record real Files windows on Niri/Wayland. See capture.md before running.

Uses a temporary English configuration and private D-Bus session. All scenes,
including the fastfetch terminal screenshot, are recorded by default.
"""
import argparse
import contextlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import tomllib

from evdev import UInput, ecodes

ASSETS = Path(__file__).resolve().parent
sys.dont_write_bytecode = True
SCENES = ('overview', 'browsing', 'terminal', 'copy-path', 'keyboard', 'theme', 'appearance',
          'opacity', 'corners', 'font', 'animations', 'picker')
spec = importlib.util.spec_from_file_location('gif_export', ASSETS / 'edit-file-browsing.py')
gif_export = importlib.util.module_from_spec(spec)
spec.loader.exec_module(gif_export)


def run(*args, **kwargs):
    return subprocess.run([str(a) for a in args], check=True, **kwargs)


def read(*args):
    return run(*args, capture_output=True, text=True).stdout.strip()


def ipc(query):
    return json.loads(read('niri', 'msg', '-j', query))


def action(*args):
    run('niri', 'msg', 'action', *args, stdout=subprocess.DEVNULL)


class Capture:
    def __init__(self, args):
        self.args = args
        self.work = Path(tempfile.mkdtemp(prefix='gds-capture-'))
        print(f'Raw captures and logs: {self.work}', flush=True)
        self.output = args.output.resolve()
        self.output.mkdir(parents=True, exist_ok=True)
        self.original_focus = ipc('focused-window')
        self.original_workspace = next(w for w in ipc('workspaces') if w['is_focused'])
        self.monitor = self.original_workspace['output']
        self.display = ipc('outputs')[self.monitor]['logical']
        if self.display['width'] < 1280 or self.display['height'] < 840:
            raise RuntimeError('Recording needs at least 1280 × 840 logical pixels')
        self.original_wallpaper = read('dms', 'ipc', 'call', 'wallpaper', 'getFor', self.monitor)
        session = json.loads(read('dms', 'ipc', 'call', 'settings', 'dumpSession'))
        self.per_monitor = session.get('perMonitorWallpaper', False)
        self.wallpapers = sorted(args.wallpapers.glob('*'))
        self.wallpapers = [p for p in self.wallpapers if p.suffix.lower() in ('.jpg', '.jpeg', '.png')]
        if not self.wallpapers:
            raise RuntimeError('No wallpapers found; run fetch-wallpapers.py first')
        config = Path(os.environ.get('XDG_CONFIG_HOME', Path.home() / '.config'))
        self.preferences = tomllib.loads((config / 'gds/files/settings.toml').read_text())
        self.dbus = Path(shutil.which('dbus-run-session')).resolve()
        self.dbus_config = self.dbus.parent.parent / 'share/dbus-1/session.conf'
        self.proc = self.window = self.recorder = None
        self.keyboard = UInput({ecodes.EV_KEY: list(range(ecodes.KEY_ESC, ecodes.KEY_MICMUTE + 1))},
                               name='gds-readme-recording')
        time.sleep(0.6)
        self.prepare_files()
        (self.work / 'capture.json').write_text(json.dumps({
            'version': read('gds-files', '--version'),
            'language': 'english',
            'original_wallpaper': self.original_wallpaper,
            'wallpapers': [str(p) for p in self.wallpapers],
            'theme_mode': read('dms', 'ipc', 'call', 'theme', 'getMode'),
            'appearance': self.preferences.get('appearance', {}),
            'font': self.preferences.get('font', {}), 'monitor': self.display,
        }, ensure_ascii=False, indent=2))

    def prepare_files(self):
        self.workspace = self.work / 'Workspace'
        for folder in ('Documents', 'Images', 'Projects'):
            (self.workspace / folder).mkdir(parents=True)
        for filename, text in {
            'README.md': '# Workspace\n\nFiles, ideas, and everything in between.\n',
            'notes.md': '# Notes\n\nBrowse with the keyboard. Preview images. Keep the terminal nearby.\n',
            'todo.txt': 'Review the gallery\nOrganize project files\nTake a break\n',
            'Documents/Project notes.md': '# Project notes\n\nA workspace for the Files demo.\n',
            'Projects/hello.py': 'print("Hello from GDS Files!")\n',
        }.items():
            (self.workspace / filename).write_text(text)
        for pattern, filename in [('15-*', 'Big-Sur.jpg'), ('06-*', 'Bliss.jpg'), ('03-*', 'Mojave.jpg')]:
            source = next(self.args.wallpapers.glob(pattern))
            run('ffmpeg', '-hide_banner', '-loglevel', 'error', '-y', '-i', source,
                '-vf', 'scale=2560:1440:force_original_aspect_ratio=decrease',
                '-frames:v', '1', '-q:v', '2', self.workspace / 'Images' / filename)

    def settings(self, **overrides):
        groups = {
            'appearance': {**self.preferences.get('appearance', {}), **overrides.get('appearance', {})},
            'font': {**self.preferences.get('font', {}), **overrides.get('font', {})},
            'files': {'show_hidden': False, 'terminal_content_height': 300.0},
            'files.sidebar': {'show_volumes': False, 'locations_collapsed': False},
            'terminal': {'shell': '/run/current-system/sw/bin/bash', 'shell_args': ['--noprofile', '--norc'],
                         'login_shell': False, 'font_family': 'monospace', 'font_size': 15.0},
        }
        lines = ['language = "english"', 'time_format = "relative"',
                 f'animations_enabled = {str(overrides.get("animations", True)).lower()}']
        for group, fields in groups.items():
            lines.append(f'\n[{group}]')
            lines.extend(f'{k} = {json.dumps(v, ensure_ascii=False)}' for k, v in fields.items())
        return '\n'.join(lines) + '\n'

    def close_window(self):
        if self.window and any(w['id'] == self.window for w in ipc('windows')):
            action('close-window', '--id', self.window)
        if self.proc:
            try:
                self.proc.wait(timeout=4)
            except subprocess.TimeoutExpired:
                os.killpg(self.proc.pid, signal.SIGTERM)
                self.proc.wait(timeout=4)
        self.proc = self.window = None

    def launch(self, name, *, directory=None, picker=False, **overrides):
        self.close_window()
        empty = next(w for w in ipc('workspaces')
                     if w['output'] == self.monitor and w['active_window_id'] is None)
        action('focus-workspace', empty['idx'])
        root = self.work / name
        config = root / 'config/gds/files/settings.toml'
        config.parent.mkdir(parents=True)
        config.write_text(self.settings(**overrides))
        if name == 'terminal':
            fetch_config = root / 'config/fastfetch/config.jsonc'
            fetch_config.parent.mkdir(parents=True)
            fetch_config.write_text(json.dumps({
                'logo': {'source': 'nixos_small'},
                'modules': ['os', 'host', 'kernel', 'wm', 'shell', 'cpu', 'gpu',
                            'memory', 'break', 'colors'],
            }, indent=2))
        before = {w['id'] for w in ipc('windows')}
        env = {**os.environ, 'XDG_CONFIG_HOME': str(root / 'config'),
               'XDG_STATE_HOME': str(root / 'state'), 'PS1': 'Workspace $ ',
               'LANG': 'en_US.UTF-8', 'LC_ALL': 'en_US.UTF-8', 'LANGUAGE': 'en_US',
               'SHELL': '/run/current-system/sw/bin/bash'}
        env.pop('NO_COLOR', None)
        command = [str(self.dbus), f'--config-file={self.dbus_config}', '--', 'gds-files']
        if picker:
            command += ['--pick-file', '--multiple', '--filter', 'Images:*.jpg,*.png']
        command += [str(directory or self.workspace)]
        with (root / 'app.log').open('w') as log:
            self.proc = subprocess.Popen(command, env=env, stdout=log, stderr=log, start_new_session=True)
        for _ in range(100):
            windows = [w for w in ipc('windows') if w['id'] not in before
                       and (w['app_id'] or '').startswith('io.github.GanDesktop.GdsFiles')]
            if windows:
                self.window = windows[0]['id']
                break
            if self.proc.poll() is not None:
                raise RuntimeError(f'Files exited; inspect {root / "app.log"}')
            time.sleep(0.1)
        else:
            raise RuntimeError('Files window did not appear')
        action('focus-window', '--id', self.window)
        action('move-window-to-floating', '--id', self.window)
        action('set-window-width', '--id', self.window, '1200')
        action('set-window-height', '--id', self.window, '760')
        time.sleep(0.4)
        action('move-floating-window', '--id', self.window,
               '-x', str((self.display['width'] - 1200) // 2),
               '-y', str((self.display['height'] - 760) // 2))
        action('center-window', '--id', self.window)
        time.sleep(0.8)
        self.update_geometry()

    def update_geometry(self):
        windows = ipc('windows')
        w = next(w for w in windows if w['id'] == self.window)
        if sum(v['workspace_id'] == w['workspace_id'] for v in windows) != 1:
            raise RuntimeError('The recording workspace must contain only the demo window')
        layout = w['layout']
        x, y = layout['tile_pos_in_workspace_view']
        dx, dy = layout['window_offset_in_tile']
        width, height = layout['window_size']
        x, y = round(x + dx) - 24, round(y + dy) - 24
        if not (x >= 0 and y >= 0 and x + width + 48 <= self.display['width']
                and y + height + 48 <= self.display['height']):
            raise RuntimeError('Demo window and screenshot margin must fit on screen')
        self.geometry = f'{self.display["x"] + x},{self.display["y"] + y} {width + 48}x{height + 48}'

    def focused(self):
        current = ipc('focused-window')
        if not current or current['id'] != self.window:
            raise RuntimeError('Demo lost focus; stopped to avoid typing into another application')

    def key(self, key, *modifiers, pause=0.3):
        self.focused()
        aliases = {'ctrl': 'LEFTCTRL', 'shift': 'LEFTSHIFT', 'alt': 'LEFTALT'}
        codes = [getattr(ecodes, 'KEY_' + aliases.get(k, k.upper())) for k in (*modifiers, key)]
        for code in codes:
            self.keyboard.write(ecodes.EV_KEY, code, 1)
            self.keyboard.syn()
            time.sleep(0.04)
        for code in reversed(codes):
            self.keyboard.write(ecodes.EV_KEY, code, 0)
            self.keyboard.syn()
            time.sleep(0.04)
        time.sleep(pause)

    def type(self, text):
        self.focused()
        run('wtype', '-d', '35', text)
        time.sleep(0.2)

    def navigate(self, path):
        self.key('l', 'ctrl')
        self.type(str(path))
        self.key('ENTER', pause=0.8)

    def snapshot(self, name, destination=None, size=None):
        self.focused()
        self.update_geometry()
        geometry = self.geometry if size is None else f'{self.geometry.split()[0]} {size}'
        run(self.args.grim, '-s', '1', '-g', geometry, destination or self.output / f'{name}.png')
        print(f'Captured {name}', flush=True)

    @contextlib.contextmanager
    def video(self, name):
        self.update_geometry()
        path = self.work / f'{name}.mp4'
        with (self.work / f'{name}-recorder.log').open('w') as log:
            self.recorder = subprocess.Popen([
                self.args.recorder, '-D', '--no-dmabuf', '-o', self.monitor,
                '-g', self.geometry, '-r', '30', '-f', str(path),
            ], stdout=log, stderr=log)
        time.sleep(0.6)
        try:
            if self.recorder.poll() is not None:
                raise RuntimeError('wf-recorder failed; inspect recorder log')
            yield
            time.sleep(0.6)
        finally:
            self.recorder.send_signal(signal.SIGINT)
            self.recorder.wait(timeout=15)
            self.recorder = None
        gif_export.export(path, self.output / f'{name}.gif')
        print(f'Recorded {name}.gif', flush=True)

    def gallery(self, details=False):
        self.navigate(self.workspace / 'Images')
        self.key('g', 'ctrl')
        self.key('SPACE', 'alt')
        self.key('HOME')
        if details:
            self.key('p', 'ctrl', pause=0.8)

    def appearance_page(self):
        self.key('COMMA', 'ctrl')
        self.key('TAB', 'ctrl', pause=0.6)

    def scene(self, name):
        print(f'Scene: {name}', flush=True)
        self.launch(name)
        if name == 'overview':
            self.gallery(details=True)
            self.snapshot('overview')
        elif name == 'browsing':
            self.gallery()
            self.navigate(self.workspace)
            with self.video('file-browsing'):
                self.key('SPACE', 'alt')
                self.key('HOME')
                self.key('DOWN')
                self.key('ENTER', pause=1.2)
                self.key('g', 'ctrl', pause=0.8)
                self.key('g', 'ctrl', pause=0.8)
                self.key('f', 'ctrl')
                self.type('Bliss')
                time.sleep(0.8)
                self.key('a', 'ctrl')
                self.key('BACKSPACE')
                self.key('ESC')
                self.key('SPACE', 'alt')
                self.key('HOME')
                self.key('p', 'ctrl', pause=1)
                self.key('RIGHT', pause=1)
                self.key('p', 'ctrl')
                self.key('LEFT', 'alt', pause=1)
        elif name == 'terminal':
            self.key('GRAVE', 'ctrl', pause=0.8)
            self.type('fastfetch')
            self.key('ENTER', pause=2)
            self.snapshot('terminal')
        elif name == 'copy-path':
            self.gallery(details=True)
            self.key('F10', 'shift')
            for _ in range(4):
                self.key('DOWN', pause=0.1)
            self.snapshot('copy-path')
            self.key('ESC')
            with self.video('copy-path'):
                self.key('c', 'ctrl', 'shift', pause=0.8)
                clipboard = read('wl-paste', '--no-newline')
                expected = str(self.workspace / 'Images/Big-Sur.jpg')
                if clipboard != expected:
                    raise RuntimeError(f'Copy path did not produce {expected!r}')
                self.key('l', 'ctrl')
                self.key('v', 'ctrl', pause=1.5)
                self.key('ESC', pause=0.8)
        elif name == 'keyboard':
            with self.video('keyboard-navigation'):
                self.key('SPACE', 'alt')
                self.key('HOME')
                self.key('DOWN')
                self.key('ENTER', 'ctrl', pause=1)
                self.key('g', 'ctrl', pause=0.7)
                self.key('SPACE', 'alt')
                self.key('HOME')
                self.key('RIGHT')
                self.key('p', 'ctrl', pause=1)
                self.key('TAB', 'ctrl', pause=0.8)
                self.key('TAB', 'ctrl', pause=0.8)
                self.key('w', 'ctrl', pause=1)
        elif name == 'theme':
            self.gallery(details=True)
            frames = self.work / 'theme-frames'
            frames.mkdir()
            for i, wallpaper in enumerate(self.wallpapers):
                self.wallpaper(wallpaper)
                time.sleep(3)
                self.snapshot(wallpaper.stem, frames / f'{i:02d}.png')
            run('ffmpeg', '-hide_banner', '-loglevel', 'error', '-y', '-framerate', '1',
                '-i', frames / '%02d.png', '-filter_complex',
                'scale=960:-2:flags=lanczos,split[a][b];[a]palettegen[p];[b][p]paletteuse=dither=bayer',
                '-loop', '0', self.output / 'theme-sync.gif')
            self.set_backdrop()
        elif name == 'appearance':
            self.appearance_page()
            self.snapshot('appearance-settings')
        elif name == 'opacity':
            for label, content, sidebar in [('opaque', 1.0, 1.0), ('translucent', 0.65, 0.35)]:
                self.launch(f'opacity-{label}', appearance={'content_opacity': content, 'sidebar_opacity': sidebar})
                self.gallery(details=True)
                self.snapshot(f'opacity-{label}')
        elif name == 'corners':
            for label, rounded in [('square', False), ('rounded', True)]:
                self.launch(f'corners-{label}', appearance={'rounded_corners': rounded})
                self.gallery(details=True)
                self.snapshot(f'corners-{label}', size='400x260')
        elif name == 'font':
            frames = self.work / 'font-frames'
            frames.mkdir()
            for i, size in enumerate((14.0, 20.0)):
                self.launch(f'font-{int(size)}', font={'size': size})
                self.gallery(details=True)
                self.snapshot(f'font-{int(size)}', frames / f'{i:02d}.png')
            run('ffmpeg', '-hide_banner', '-loglevel', 'error', '-y', '-framerate', '1/2',
                '-i', frames / '%02d.png', '-filter_complex',
                'scale=960:-2:flags=lanczos,split[a][b];[a]palettegen[p];[b][p]paletteuse=dither=bayer',
                '-loop', '0', self.output / 'font-size.gif')
        elif name == 'animations':
            self.gallery(details=True)
            with self.video('interface-animations'):
                for _ in range(2):
                    self.key('b', 'ctrl', pause=0.9)
                    self.key('p', 'ctrl', pause=0.9)
        elif name == 'picker':
            self.launch('picker-dialog', directory=self.workspace / 'Images', picker=True)
            self.key('g', 'ctrl')
            self.key('SPACE', 'alt')
            self.key('HOME')
            self.key('RIGHT', 'shift', pause=0.6)
            self.snapshot('file-picker')

    def set_backdrop(self):
        wallpaper = next(self.args.wallpapers.glob('15-*'))
        self.wallpaper(wallpaper)
        time.sleep(3)

    def wallpaper(self, path):
        arguments = ('setFor', self.monitor, path) if self.per_monitor else ('set', path)
        result = read('dms', 'ipc', 'call', 'wallpaper', *arguments)
        if not result.startswith('SUCCESS:'):
            raise RuntimeError(result)

    def cleanup(self):
        if self.recorder:
            self.recorder.send_signal(signal.SIGINT)
            self.recorder.wait(timeout=15)
        self.close_window()
        self.keyboard.close()
        if read('dms', 'ipc', 'call', 'wallpaper', 'getFor', self.monitor) != self.original_wallpaper:
            self.wallpaper(self.original_wallpaper)
        if self.original_focus and any(w['id'] == self.original_focus['id'] for w in ipc('windows')):
            action('focus-window', '--id', self.original_focus['id'])
        else:
            action('focus-workspace', self.original_workspace['idx'])


def main():
    # Development shells may inject libraries from a different nixpkgs revision.
    # Packaged tools and the installed Files wrapper supply their own runtime.
    os.environ.pop('LD_LIBRARY_PATH', None)
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--scene', action='append', choices=SCENES)
    parser.add_argument('--wallpapers', type=Path, required=True)
    parser.add_argument('--output', type=Path, default=ASSETS)
    parser.add_argument('--grim', default=os.environ.get('GRIM', 'grim'))
    parser.add_argument('--recorder', default=os.environ.get('WF_RECORDER', 'wf-recorder'))
    args = parser.parse_args()
    if 'terminal' in (args.scene or SCENES) and not shutil.which('fastfetch'):
        parser.error('The terminal scene requires fastfetch')
    for tool in ('gds-files', 'niri', 'dms', 'wtype', 'wl-paste', 'dbus-run-session',
                 'ffmpeg', args.grim, args.recorder):
        if not shutil.which(tool):
            parser.error(f'Required executable not found: {tool}')
    capture = Capture(args)
    try:
        capture.set_backdrop()
        for scene in args.scene or SCENES:
            capture.scene(scene)
    finally:
        capture.cleanup()


if __name__ == '__main__':
    main()
