"""Run existing Files UI scenarios, then a separate isolated native Wayland smoke."""
import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time


def wayland_child(binary, artifacts):
    """Weston supplies its Wayland socket and private session bus to this child."""
    with (artifacts / "wayland-app.log").open("w", encoding="utf-8") as log:
        app = subprocess.Popen([str(binary), "--enable-logs", str(artifacts / "home/work")], stdout=log, stderr=subprocess.STDOUT)
        try:
            while app.poll() is None and not (artifacts / "stop-wayland").exists():
                time.sleep(0.05)
            if app.poll() is None:
                app.terminate()
            code = app.wait(timeout=20)
            (artifacts / "wayland-app-exit.txt").write_text(f"{code}\n", encoding="utf-8")
            return code
        finally:
            if app.poll() is None:
                app.kill()
                app.wait()


if len(sys.argv) > 1 and sys.argv[1] == "--wayland-child":
    raise SystemExit(wayland_child(Path(sys.argv[2]), Path(sys.argv[3])))

repository = Path(__file__).resolve().parents[3]
binary = Path(sys.argv[1]).resolve()
weston = Path(sys.argv[2]).resolve()
output = repository / "target/files-e2e"
output.mkdir(parents=True, exist_ok=True)
artifacts = Path(tempfile.mkdtemp(prefix="run-", dir=output))
home = artifacts / "home"
for directory in ("work", ".config/gds/app/files", ".local/share", ".local/state", ".cache"):
    (home / directory).mkdir(parents=True, exist_ok=True)
runtime = artifacts / "runtime"
runtime.mkdir(mode=0o700)
capture_directory = artifacts / "wayland"
capture_directory.mkdir()
frame_path = capture_directory / "app-frame.png"
(artifacts / "dbus.conf").write_text(f'''<busconfig>
  <type>session</type><listen>unix:tmpdir={runtime}</listen>
  <policy context="default"><allow send_destination="*"/><allow receive_sender="*"/><allow own="*"/></policy>
</busconfig>
''', encoding="utf-8")
(home / "work/original.txt").write_bytes(b"original payload\n")
(home / ".config/gds/app/files/settings.toml").write_text(
    'language = "en-US"\nanimations_enabled = false\n[appearance]\nsource = "builtin"\nmode = "dark"\n', encoding="utf-8")
environment = os.environ.copy()
for variable in ("DISPLAY", "WAYLAND_DISPLAY", "DBUS_SESSION_BUS_ADDRESS"):
    environment.pop(variable, None)
environment.update(HOME=str(home), XDG_CONFIG_HOME=str(home / ".config"),
                   XDG_DATA_HOME=str(home / ".local/share"), XDG_STATE_HOME=str(home / ".local/state"),
                   XDG_CACHE_HOME=str(home / ".cache"), XDG_RUNTIME_DIR=str(runtime),
                   DBUS_SYSTEM_BUS_ADDRESS=f"unix:path={runtime}/unavailable-system-bus",
                   GDS_E2E_ARTIFACTS=str(artifacts), RUST_LOG="info,gds=debug,gds_files=debug")
bus = ["dbus-run-session", f"--config-file={artifacts / 'dbus.conf'}", "--"]
driven_command = bus + [str(binary)]
wayland_command = bus + [str(weston), "--backend=headless", "--renderer=pixman",
                        "--socket=files-e2e-wayland", "--width=1280", "--height=900", "--fake-seat",
                        "--no-config", "--idle-time=0", "--debug", "--", sys.executable,
                        str(Path(__file__).resolve()), "--wayland-child", str(binary), str(artifacts)]
with binary.open("rb") as source:
    digest = hashlib.file_digest(source, "sha256").hexdigest()
(artifacts / "manifest.json").write_text(json.dumps({
    "driven_command": driven_command, "wayland_command": wayland_command,
    "binary_sha256": digest,
    "revision": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=repository, text=True).strip(),
    "replay": [sys.executable, str(Path(__file__).resolve()), str(binary), str(weston)],
}, indent=2) + "\n", encoding="utf-8")
(artifacts / "source.diff").write_bytes(subprocess.check_output(["git", "diff", "--", "Cargo.toml", "bin/files", "src"], cwd=repository))
subprocess.run(["tar", "czf", str(artifacts / "source.tar.gz"), "Cargo.toml", "Cargo.lock", "build.rs",
                "src", "bin/files/src", "bin/files/tests", "assets", "macros", "packaging/nix",
                "shell.nix", "flake.lock"], cwd=repository, check=True)
print(f"Files E2E artifacts: {artifacts}", flush=True)
with (artifacts / "driven.log").open("w", encoding="utf-8") as log:
    result = subprocess.run(driven_command, env={**environment, "GDS_E2E_MODE": "driven"},
                            stdout=log, stderr=subprocess.STDOUT, timeout=180)
(artifacts / "driven-exit.txt").write_text(f"{result.returncode}\n", encoding="utf-8")
if result.returncode != 0 or not (artifacts / "driven-passed.txt").is_file():
    raise SystemExit(f"Driven native UI scenario failed; inspect {artifacts / 'driven.log'}")
print((artifacts / "driven-passed.txt").read_text(encoding="utf-8"), end="", flush=True)

with (artifacts / "wayland-session.log").open("w", encoding="utf-8") as log:
    compositor = subprocess.Popen(wayland_command, env={**environment, "GDS_E2E_MODE": "wayland", "ANTIWEBUI_DUMP_FRAME": str(frame_path)},
                                  stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
    try:
        deadline = time.monotonic() + 45
        app_log = artifacts / "wayland-app.log"
        while time.monotonic() < deadline:
            if compositor.poll() is not None:
                raise RuntimeError("Wayland session exited before the application became ready")
            if app_log.exists():
                messages = app_log.read_text(encoding="utf-8", errors="replace")
                if "Files ready" in messages and "[anti-webui] frame " in messages and frame_path.is_file():
                    break
            time.sleep(0.05)
        else:
            raise TimeoutError(f"Wayland app did not become ready; inspect {app_log}")
        screenshot = subprocess.run([str(weston.with_name("weston-screenshooter"))], cwd=capture_directory,
                                    env={**environment, "WAYLAND_DISPLAY": "files-e2e-wayland"},
                                    stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=20)
        (capture_directory / "capture.log").write_bytes(screenshot.stdout)
        if screenshot.returncode != 0 or not [path for path in capture_directory.glob("*.png") if path != frame_path]:
            raise RuntimeError(f"Wayland compositor capture failed; inspect {capture_directory}")
        (artifacts / "stop-wayland").touch()
        deadline = time.monotonic() + 25
        exit_file = artifacts / "wayland-app-exit.txt"
        while not exit_file.exists() and time.monotonic() < deadline:
            time.sleep(0.05)
        if not exit_file.exists() or exit_file.read_text(encoding="utf-8").strip() != "0":
            raise RuntimeError(f"Native shutdown did not drain cleanly; inspect {app_log}")
        (artifacts / "wayland-passed.txt").write_text(
            "PASS (actual Wayland): native app startup, compositor-presented screenshot, graceful shutdown\n", encoding="utf-8")
    finally:
        if compositor.poll() is None:
            os.killpg(compositor.pid, signal.SIGTERM)
        try:
            compositor.wait(timeout=10)
        except subprocess.TimeoutExpired:
            os.killpg(compositor.pid, signal.SIGKILL)
            compositor.wait()

passed = (artifacts / "driven-passed.txt").read_text(encoding="utf-8") + (artifacts / "wayland-passed.txt").read_text(encoding="utf-8")
(artifacts / "passed.txt").write_text(passed, encoding="utf-8")
print((artifacts / "wayland-passed.txt").read_text(encoding="utf-8"), end="")
