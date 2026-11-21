"""Run the driven zgui E2E suite and native headless Weston smoke check; retain artifacts."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import struct
import subprocess
import sys
import tempfile
import time
import zlib

CHECKPOINTS = [
    "01-open",
    "02-rename-undo-redo",
    "03-new-folder",
    "04-trash-restore",
    "05-external-watch",
    "06-grid-switch",
    "07-tab-drag",
    "08-search-ime",
    "09-details-terminal",
    "10-location-mouse-history",
]


def wait_for(predicate, timeout_secs: float, message: str) -> None:
    deadline = time.monotonic() + timeout_secs
    while time.monotonic() < deadline:
        if predicate():
            return
        time.sleep(0.05)
    raise SystemExit(message)


def png_center_rows(path: Path) -> list[bytes]:
    raw = path.read_bytes()
    pos = 8
    width = 0
    height = 0
    color_type = 6
    idat = bytearray()
    while pos + 8 <= len(raw):
        length = struct.unpack(">I", raw[pos : pos + 4])[0]
        kind = raw[pos + 4 : pos + 8]
        chunk = raw[pos + 8 : pos + 8 + length]
        pos += 12 + length
        if kind == b"IHDR":
            width, height, _bit_depth, color_type = struct.unpack(">IIBB", chunk[:10])
        elif kind == b"IDAT":
            idat.extend(chunk)
        elif kind == b"IEND":
            break
    bpp = {2: 3, 6: 4}.get(color_type)
    if width == 0 or height == 0 or not idat or bpp is None:
        raise SystemExit(f"Unsupported Weston PNG format in {path}: {width}x{height} color_type={color_type}")
    decompressed = zlib.decompress(bytes(idat))
    row_bytes = width * bpp
    stride = row_bytes + 1
    if len(decompressed) < height * stride:
        raise SystemExit(f"Truncated PNG scanlines in {path}")
    y0 = height // 4
    y1 = (height * 3) // 4
    x0 = (width // 4) * bpp
    x1 = ((width * 3) // 4) * bpp
    prev = bytearray(row_bytes)
    rows: list[bytes] = []
    for y in range(y1):
        base = y * stride
        ftype = decompressed[base]
        scan = decompressed[base + 1 : base + 1 + row_bytes]
        cur = bytearray(row_bytes)
        if ftype == 0:
            cur[:] = scan
        elif ftype == 1:
            for i in range(row_bytes):
                left = cur[i - bpp] if i >= bpp else 0
                cur[i] = (scan[i] + left) & 0xFF
        elif ftype == 2:
            for i in range(row_bytes):
                cur[i] = (scan[i] + prev[i]) & 0xFF
        elif ftype == 3:
            for i in range(row_bytes):
                left = cur[i - bpp] if i >= bpp else 0
                cur[i] = (scan[i] + ((left + prev[i]) >> 1)) & 0xFF
        elif ftype == 4:
            for i in range(row_bytes):
                a = cur[i - bpp] if i >= bpp else 0
                b = prev[i]
                c = prev[i - bpp] if i >= bpp else 0
                p = a + b - c
                pa = abs(p - a)
                pb = abs(p - b)
                pc = abs(p - c)
                pr = a if pa <= pb and pa <= pc else (b if pb <= pc else c)
                cur[i] = (scan[i] + pr) & 0xFF
        else:
            cur[:] = scan
        prev = cur
        if y >= y0:
            rows.append(bytes(cur[x0:x1]))
    return rows


def capture_weston_png(screenshooter: Path, artifacts: Path, env: dict[str, str]) -> Path:
    before = set(artifacts.glob("wayland-screenshot*.png"))
    shot_res = subprocess.run(
        [str(screenshooter)],
        cwd=artifacts,
        env=env,
        capture_output=True,
        text=True,
        timeout=15,
    )
    if shot_res.returncode != 0:
        raise SystemExit(f"weston-screenshooter failed ({shot_res.returncode}): {shot_res.stderr}")
    after = set(artifacts.glob("wayland-screenshot*.png")) - before
    if not after:
        after = set(artifacts.glob("*.png")) - {artifacts / f"{name}.png" for name in CHECKPOINTS}
    if not after:
        raise SystemExit(f"weston-screenshooter produced no PNG in {artifacts}")
    newest = max(after, key=lambda p: p.stat().st_mtime)
    if newest.stat().st_size == 0:
        raise SystemExit(f"Captured Wayland PNG is empty: {newest}")
    return newest


def build_base_env(
    home: Path,
    runtime: Path,
    artifacts: Path,
    weston: Path,
) -> dict[str, str]:
    env = os.environ.copy()
    for key in (
        "DISPLAY",
        "WAYLAND_DISPLAY",
        "DBUS_SESSION_BUS_ADDRESS",
        "XDG_CURRENT_DESKTOP",
        "NIRI_SOCKET",
        "SWAYSOCK",
        "HYPRLAND_INSTANCE_SIGNATURE",
    ):
        env.pop(key, None)
    env.update(
        HOME=str(home),
        XDG_CONFIG_HOME=str(home / ".config"),
        XDG_DATA_HOME=str(home / ".local/share"),
        XDG_STATE_HOME=str(home / ".local/state"),
        XDG_CACHE_HOME=str(home / ".cache"),
        XDG_RUNTIME_DIR=str(runtime),
        DBUS_SYSTEM_BUS_ADDRESS=f"unix:path={runtime}/unavailable-system-bus",
        GDS_E2E_ARTIFACTS=str(artifacts),
        GDS_E2E_SCREENSHOT=str(weston.with_name("weston-screenshooter")),
    )
    return env


def run_driven(
    binary: Path,
    work: Path,
    artifacts: Path,
    base_env: dict[str, str],
) -> Path:
    env = base_env.copy()
    env["GDS_E2E_MODE"] = "driven"
    command = [
        "timeout",
        "--kill-after=5",
        "90",
        "dbus-run-session",
        f"--config-file={artifacts / 'dbus.conf'}",
        "--",
        str(binary),
        str(work),
    ]
    log_path = artifacts / "driven.log"
    with log_path.open("w", encoding="utf-8") as log:
        result = subprocess.run(command, env=env, stdout=log, stderr=subprocess.STDOUT)
    (artifacts / "driven-exit-code.txt").write_text(f"{result.returncode}\n", encoding="utf-8")
    if result.returncode != 0:
        raise SystemExit(f"Driven E2E failed (exit {result.returncode}); inspect {log_path}")

    passed_path = artifacts / "driven-passed.txt"
    if not passed_path.is_file():
        raise SystemExit(f"Driven E2E did not write {passed_path}; inspect {log_path}")

    for name in CHECKPOINTS:
        png = artifacts / f"{name}.png"
        a11y = artifacts / f"{name}.accessibility.txt"
        if not png.is_file() or png.stat().st_size == 0:
            raise SystemExit(f"Missing or empty PNG artifact {png}; inspect {log_path}")
        if not a11y.is_file() or a11y.stat().st_size == 0:
            raise SystemExit(f"Missing or empty accessibility dump {a11y}; inspect {log_path}")

    return passed_path


def run_wayland_smoke(
    binary: Path,
    weston: Path,
    work: Path,
    runtime: Path,
    artifacts: Path,
    base_env: dict[str, str],
) -> Path:
    socket_name = "files-e2e-wayland"
    socket_path = runtime / socket_name
    weston_log_path = artifacts / "weston.log"
    app_log_path = artifacts / "wayland-app.log"
    screenshooter = weston.with_name("weston-screenshooter")

    dbus_proc = subprocess.Popen(
        [
            "dbus-daemon",
            "--nofork",
            "--print-address=1",
            f"--config-file={artifacts / 'dbus.conf'}",
        ],
        env=base_env.copy(),
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    try:
        assert dbus_proc.stdout is not None
        dbus_addr = dbus_proc.stdout.readline().strip()
        dbus_proc.stdout.close()
        if not dbus_addr:
            err = dbus_proc.stderr.read() if dbus_proc.stderr else ""
            raise SystemExit(
                f"Failed to start shared dbus-daemon for Wayland smoke: {err}"
            )
        if dbus_proc.stderr:
            dbus_proc.stderr.close()

        bus_env = base_env.copy()
        bus_env["DBUS_SESSION_BUS_ADDRESS"] = dbus_addr

        weston_ini = artifacts / "weston.ini"
        weston_ini.write_text(
            "[core]\nidle-time=0\n\n[shell]\nlocking=false\n\n[output]\nname=headless\nmode=1600x1000\n",
            encoding="utf-8",
        )

        weston_cmd = [
            str(weston),
            "--backend=headless",
            "--renderer=pixman",
            f"--socket={socket_name}",
            "--width=1600",
            "--height=1000",
            "--fake-seat",
            f"--config={weston_ini}",
            "--idle-time=0",
            "--debug",
        ]

        with weston_log_path.open("w", encoding="utf-8") as weston_log:
            weston_proc = subprocess.Popen(
                weston_cmd,
                env=bus_env,
                stdout=weston_log,
                stderr=subprocess.STDOUT,
                start_new_session=True,
            )
            try:
                wait_for(
                    lambda: socket_path.exists() or weston_proc.poll() is not None,
                    15.0,
                    f"Timed out waiting for Weston Wayland socket {socket_path}; inspect {weston_log_path}",
                )
                if weston_proc.poll() is not None:
                    raise SystemExit(
                        f"Weston exited early ({weston_proc.returncode}); inspect {weston_log_path}"
                    )

                app_env = bus_env.copy()
                app_env["WAYLAND_DISPLAY"] = socket_name
                app_env["GDS_E2E_MODE"] = "wayland"
                app_env["RUST_LOG"] = "gds_files=info,zgui_render_wgpu::renderer::builder=info,warn"

                baseline_rows: list[bytes] = []
                prev_rows: list[bytes] = []
                desktop_deadline = time.monotonic() + 8.0
                while time.monotonic() < desktop_deadline:
                    shot = capture_weston_png(screenshooter, artifacts, app_env)
                    rows = png_center_rows(shot)
                    shot.unlink(missing_ok=True)
                    non_empty = any(any(row) for row in rows)
                    if non_empty and rows == prev_rows:
                        baseline_rows = rows
                        break
                    prev_rows = rows
                    time.sleep(0.2)
                if not baseline_rows:
                    baseline_rows = prev_rows

                with app_log_path.open("w", encoding="utf-8") as app_log:
                    app_proc = subprocess.Popen(
                        [str(binary), str(work)],
                        env=app_env,
                        stdout=app_log,
                        stderr=subprocess.STDOUT,
                        start_new_session=True,
                    )
                    try:
                        def wayland_ready() -> bool:
                            if app_proc.poll() is not None:
                                return True
                            log_text = app_log_path.read_text(encoding="utf-8", errors="replace")
                            return "Files ready" in log_text and (
                                "graphics device opened" in log_text
                                or "no graphics card on this machine" in log_text
                            )

                        wait_for(
                            wayland_ready,
                            25.0,
                            f"Native Wayland Files session did not become ready; inspect {app_log_path}",
                        )
                        if app_proc.poll() is not None:
                            raise SystemExit(
                                f"Native Wayland app exited early ({app_proc.returncode}); inspect {app_log_path}"
                            )

                        wayland_png = artifacts / "wayland-window.png"
                        window_painted = False
                        deadline = time.monotonic() + 20.0
                        while time.monotonic() < deadline:
                            if app_proc.poll() is not None:
                                raise SystemExit(
                                    f"Native Wayland app exited early ({app_proc.returncode}); inspect {app_log_path}"
                                )
                            shot = capture_weston_png(screenshooter, artifacts, app_env)
                            current_rows = png_center_rows(shot)
                            diff_rows = sum(a != b for a, b in zip(baseline_rows, current_rows))
                            if diff_rows >= 100:
                                shutil.move(str(shot), str(wayland_png))
                                window_painted = True
                                break
                            shot.unlink(missing_ok=True)
                            time.sleep(0.4)
                        if not window_painted:
                            raise SystemExit(
                                f"Native Wayland window did not paint onto Weston within 20s; inspect {app_log_path}"
                            )
                        shot_hdr = wayland_png.read_bytes()[:24]
                        shot_w, shot_h = struct.unpack(">II", shot_hdr[16:24])
                        if (shot_w, shot_h) != (1600, 1000):
                            raise SystemExit(
                                f"Expected 1600x1000 Weston screenshot, got {shot_w}x{shot_h} in {wayland_png}"
                            )
                    finally:
                        if app_proc.poll() is None:
                            app_proc.send_signal(signal.SIGTERM)
                            try:
                                app_proc.wait(timeout=10)
                            except subprocess.TimeoutExpired:
                                app_proc.kill()
                                app_proc.wait(timeout=5)
            finally:
                if weston_proc.poll() is None:
                    try:
                        os.killpg(weston_proc.pid, signal.SIGTERM)
                    except ProcessLookupError:
                        pass
                    try:
                        weston_proc.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        try:
                            os.killpg(weston_proc.pid, signal.SIGKILL)
                        except ProcessLookupError:
                            pass
                        weston_proc.wait(timeout=5)
    finally:
        if dbus_proc.poll() is None:
            dbus_proc.terminate()
            try:
                dbus_proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                dbus_proc.kill()
                dbus_proc.wait(timeout=5)

    app_log_text = app_log_path.read_text(encoding="utf-8", errors="replace")
    if "timed out waiting on app_will_quit" in app_log_text:
        raise SystemExit(f"Shutdown did not drain cleanly; inspect {app_log_path}")
    return wayland_png


def resolve_default_binary(repository: Path) -> Path:
    candidate = repository / "target/debug/gds-files"
    subprocess.run(
        [
            "cargo",
            "build",
            "--bin",
            "gds-files",
            "--no-default-features",
            "--features",
            "files-e2e",
        ],
        cwd=repository,
        check=True,
    )
    return candidate.resolve()


def resolve_default_weston(repository: Path) -> Path:
    found = shutil.which("weston")
    if found:
        return Path(found).resolve()
    store_path = subprocess.check_output(
        [
            "nix",
            "build",
            "--impure",
            "--no-link",
            "--print-out-paths",
            "--expr",
            "let lock = builtins.fromJSON (builtins.readFile ./flake.lock); "
            "source = builtins.fetchTree lock.nodes.nixpkgs.locked; "
            "pkgs = import source.outPath {}; in pkgs.weston",
        ],
        cwd=repository,
        text=True,
    ).strip()
    return (Path(store_path) / "bin/weston").resolve()


def main() -> None:
    repository = Path(__file__).resolve().parents[3]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", nargs="?", help="Path to built gds-files binary")
    parser.add_argument("weston", nargs="?", help="Path to weston binary")
    parser.add_argument("--artifacts", help="Explicit output artifacts directory")
    args = parser.parse_args()

    binary = Path(args.binary).resolve() if args.binary else resolve_default_binary(repository)
    weston = Path(args.weston).resolve() if args.weston else resolve_default_weston(repository)
    if args.artifacts:
        artifacts = Path(args.artifacts).resolve()
        if artifacts.exists():
            shutil.rmtree(artifacts)
        artifacts.mkdir(parents=True, exist_ok=True)
    else:
        output = repository / "target/files-e2e"
        output.mkdir(parents=True, exist_ok=True)
        artifacts = Path(tempfile.mkdtemp(prefix="run-", dir=output))

    home = artifacts / "home"
    work = home / "work"
    work.mkdir(parents=True)
    (home / ".config/gds/app/files").mkdir(parents=True)
    (home / ".local/share").mkdir(parents=True)
    (home / ".local/state").mkdir(parents=True)
    (home / ".cache").mkdir()
    (work / "original.txt").write_bytes(b"original payload\n")
    (home / ".config/gds/app/files/settings.toml").write_text(
        'language = "en-US"\nanimations_enabled = false\n[appearance]\nsource = "builtin"\nmode = "dark"\n',
        encoding="utf-8",
    )

    with binary.open("rb") as binary_file:
        binary_sha256 = hashlib.file_digest(binary_file, "sha256").hexdigest()
    (artifacts / "manifest.json").write_text(
        json.dumps(
            {
                "binary": str(binary),
                "weston": str(weston),
                "binary_sha256": binary_sha256,
                "revision": subprocess.check_output(
                    ["git", "rev-parse", "HEAD"], cwd=repository, text=True
                ).strip(),
                "replay": [sys.executable, str(Path(__file__).resolve()), str(binary), str(weston)],
            },
            indent=2,
        )
        + "\n",
        encoding="utf-8",
    )
    (artifacts / "source.diff").write_bytes(
        subprocess.check_output(
            ["git", "diff", "--", "Cargo.toml", "bin/files", "src"],
            cwd=repository,
        )
    )
    subprocess.run(
        [
            "tar",
            "czf",
            str(artifacts / "source.tar.gz"),
            "Cargo.toml",
            "Cargo.lock",
            "build.rs",
            "src",
            "bin/files/src",
            "bin/files/tests",
            "assets",
            "macros",
            "packaging/nix",
            "shell.nix",
            "flake.lock",
        ],
        cwd=repository,
        check=True,
    )

    print(f"Files E2E artifacts: {artifacts}", flush=True)
    # Unix domain sockets used by D-Bus and Weston are limited by UNIX_PATH_MAX (108 bytes),
    # which long worktree artifact paths exceed, so host XDG_RUNTIME_DIR in a short temp dir.
    with tempfile.TemporaryDirectory(prefix="gds-e2e-") as runtime_dir:
        runtime = Path(runtime_dir)
        runtime.chmod(0o700)
        (artifacts / "dbus.conf").write_text(
            f'''<busconfig>
  <type>session</type><listen>unix:tmpdir={runtime}</listen>
  <policy context="default"><allow send_destination="*"/><allow receive_sender="*"/><allow own="*"/></policy>
</busconfig>
''',
            encoding="utf-8",
        )
        base_env = build_base_env(home, runtime, artifacts, weston)

        driven_passed = run_driven(binary, work, artifacts, base_env)
        wayland_png = run_wayland_smoke(binary, weston, work, runtime, artifacts, base_env)
        wayland_subdir = artifacts / "wayland"
        wayland_subdir.mkdir(parents=True, exist_ok=True)
        shutil.copy2(wayland_png, wayland_subdir / "wayland-window.png")

    summary = (
        driven_passed.read_text(encoding="utf-8", errors="replace").rstrip()
        + f"\nPASS wayland screenshot={wayland_png}\n"
    )
    passed_path = artifacts / "passed.txt"
    passed_path.write_text(summary, encoding="utf-8")
    print(summary, end="")


if __name__ == "__main__":
    main()
