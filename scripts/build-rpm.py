#!/usr/bin/env python3
"""Build Fedora RPMs on the host using a cached native container toolchain."""

import argparse
from datetime import datetime, timezone
import os
from pathlib import Path
import shutil
import subprocess
import tomllib


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--fedora", type=int, default=43)
    parser.add_argument("--profile", choices=["dev", "release"], default="dev")
    parser.add_argument("--engine", choices=["docker", "podman"],
                        default="docker" if shutil.which("docker") else "podman")
    args = parser.parse_args()
    if not shutil.which(args.engine):
        parser.error(f"{args.engine} is required; start its container service before building")

    project = Path(__file__).resolve().parent.parent
    manifest = tomllib.loads((project / "Cargo.toml").read_text())
    version = manifest["package"]["version"]
    revision = subprocess.check_output(
        ["git", "rev-parse", "HEAD"], cwd=project, text=True,
    ).strip()
    release = "0." + datetime.now(timezone.utc).strftime("%Y%m%d%H%M%S")
    cache = project / "target" / f"fedora-{args.fedora}"
    output = project / "target/rpm" / f"fedora-{args.fedora}"
    cache.mkdir(parents=True, exist_ok=True)
    output.mkdir(parents=True, exist_ok=True)

    image = f"gds-rpm-builder:fedora-{args.fedora}"
    subprocess.run([
        args.engine, "build", "--tag", image,
        "--file", str(project / "packaging/fedora/Containerfile"),
        "--build-arg", f"FEDORA_VERSION={args.fedora}",
        str(project / "packaging/fedora"),
    ], check=True)

    command = [args.engine, "run", "--rm", "--user", f"{os.getuid()}:{os.getgid()}"]
    if args.engine == "podman":
        command.append("--userns=keep-id")
    for source, destination, readonly in [
        (project, "/source", True), (cache, "/cache", False), (output, "/output", False),
    ]:
        mount = f"type=bind,source={source},target={destination}"
        command.extend(["--mount", mount + (",readonly" if readonly else "")])
    command.extend([
        "--env", "HOME=/tmp", "--env", "CARGO_HOME=/cache/cargo",
        "--env", "CARGO_TARGET_DIR=/cache/target",
    ])
    if "CARGO_BUILD_JOBS" in os.environ:
        command.extend(["--env", f"CARGO_BUILD_JOBS={os.environ['CARGO_BUILD_JOBS']}"])
    command.extend([image, "rpmbuild", "-bb", "/source/packaging/fedora/gds.spec"])
    for name, value in {
        "_topdir": "/tmp/rpmbuild", "_rpmdir": "/output",
        "gds_source": "/source", "gds_target": "/cache/target",
        "gds_profile": args.profile,
        "gds_binary_profile": "debug" if args.profile == "dev" else "release",
        "gds_version": version, "gds_release": release, "gds_build_tag": revision,
    }.items():
        command.extend(["--define", f"{name} {value}"])
    subprocess.run(command, check=True)
    for package in sorted(output.glob(f"*/*-{version}-{release}.*.rpm")):
        print(package)


if __name__ == "__main__":
    main()
