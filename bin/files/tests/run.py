"""Run the Files scenario in a private native Wayland session; retain its artifacts."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

repository = Path(__file__).resolve().parents[3]
binary = Path(sys.argv[1]).resolve()
weston = Path(sys.argv[2]).resolve()
output = repository / "target/files-e2e"
output.mkdir(parents=True, exist_ok=True)
artifacts = Path(tempfile.mkdtemp(prefix="run-", dir=output))
home = artifacts / "home"
(home / "work").mkdir(parents=True)
(home / ".config/gds/app/files").mkdir(parents=True)
(home / ".local/share").mkdir(parents=True)
(home / ".local/state").mkdir(parents=True)
(home / ".cache").mkdir()
runtime = artifacts / "runtime"
runtime.mkdir(mode=0o700)
(artifacts / "dbus.conf").write_text(f'''<busconfig>
  <type>session</type><listen>unix:tmpdir={runtime}</listen>
  <policy context="default"><allow send_destination="*"/><allow receive_sender="*"/><allow own="*"/></policy>
</busconfig>
''', encoding="utf-8")
(home / "work/original.txt").write_bytes(b"original payload\n")
(home / ".config/gds/app/files/settings.toml").write_text(
    'language = "en-US"\nanimations_enabled = false\n[appearance]\nsource = "builtin"\nmode = "dark"\n',
    encoding="utf-8",
)
environment = os.environ.copy()
environment.pop("DISPLAY", None)
environment.pop("WAYLAND_DISPLAY", None)
environment.pop("DBUS_SESSION_BUS_ADDRESS", None)
environment.update(HOME=str(home), XDG_CONFIG_HOME=str(home / ".config"),
                   XDG_DATA_HOME=str(home / ".local/share"), XDG_STATE_HOME=str(home / ".local/state"),
                   XDG_CACHE_HOME=str(home / ".cache"), XDG_RUNTIME_DIR=str(runtime),
                   DBUS_SYSTEM_BUS_ADDRESS=f"unix:path={runtime}/unavailable-system-bus",
                   GDS_E2E_ARTIFACTS=str(artifacts), GDS_E2E_SCREENSHOT=str(weston.with_name("weston-screenshooter")))
command = ["timeout", "--kill-after=5", "90", "dbus-run-session", f"--config-file={artifacts / 'dbus.conf'}", "--", str(weston), "--backend=headless",
           "--renderer=pixman", "--width=1280", "--height=900", "--fake-seat",
           "--no-config", "--idle-time=0", "--debug", "--", str(binary), str(home / "work")]
with binary.open("rb") as binary_file:
    binary_sha256 = hashlib.file_digest(binary_file, "sha256").hexdigest()
(artifacts / "manifest.json").write_text(json.dumps({
    "command": command, "binary_sha256": binary_sha256,
    "revision": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=repository, text=True).strip(),
    "replay": [sys.executable, str(Path(__file__).resolve()), str(binary), str(weston)],
}, indent=2) + "\n", encoding="utf-8")
(artifacts / "source.diff").write_bytes(subprocess.check_output(["git", "diff", "--", "Cargo.toml", "bin/files", "src"], cwd=repository))
subprocess.run(["tar", "czf", str(artifacts / "source.tar.gz"), "Cargo.toml", "Cargo.lock", "build.rs",
                "src", "bin/files/src", "bin/files/tests", "assets", "macros", "packaging/nix",
                "shell.nix", "flake.lock"], cwd=repository, check=True)
print(f"Files E2E artifacts: {artifacts}", flush=True)
log_path = artifacts / "session.log"
with log_path.open("w", encoding="utf-8") as log:
    result = subprocess.run(command, env=environment, stdout=log, stderr=subprocess.STDOUT)
(artifacts / "exit-code.txt").write_text(f"{result.returncode}\n", encoding="utf-8")
if result.returncode != 0:
    raise SystemExit(f"Native session failed (exit {result.returncode}); inspect {log_path}")
log_text = log_path.read_text(encoding="utf-8", errors="replace")
if "timed out waiting on app_will_quit" in log_text:
    raise SystemExit(f"Shutdown did not drain; inspect {log_path}")
passed_path = artifacts / "passed.txt"
if not passed_path.is_file():
    raise SystemExit(f"Scenario incomplete; inspect {log_path}")
print(passed_path.read_text(encoding="utf-8", errors="replace"), end="")
