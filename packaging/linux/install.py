#!/usr/bin/env python3
"""Stage native Linux packages, sharing the application's desktop translations."""

import argparse
from pathlib import Path
import shutil
import tomllib


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", type=Path)
    parser.add_argument("binaries", type=Path)
    parser.add_argument("--component", choices=["all", "files", "portal"], default="all")
    parser.add_argument("--libexec-dir", default="usr/libexec")
    args = parser.parse_args()
    root, binaries = args.root, args.binaries
    portal_path = f"{args.libexec_dir}/xdg-desktop-portal-gds"
    project = Path(__file__).resolve().parents[2]

    def install(source, destination):
        path = root / destination
        path.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, path)

    def write(destination, content):
        path = root / destination
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content)

    for component, binary, destination in [
        ("files", "gds-files", "usr/bin/gds-files"),
        ("portal", "xdg-desktop-portal-gds", portal_path),
    ]:
        if args.component not in ("all", component):
            continue
        install(binaries / binary, destination)
        install(project / "LICENSE", f"usr/share/licenses/{binary}/LICENSE")

    if args.component in ("all", "files"):
        app_id = "io.github.GanDesktop.GdsFiles"
        install(project / "assets/files/icon-desktop.svg",
                f"usr/share/icons/hicolor/scalable/apps/{app_id}.svg")
        labels = tomllib.loads((project / "assets/files/desktop.toml").read_text())
        desktop = ["[Desktop Entry]", "Type=Application"]
        for locale, entries in labels.items():
            suffix = "" if locale == "default" else f"[{locale}]"
            desktop.extend(f"{key}{suffix}={value}" for key, value in entries.items())
        desktop.extend([
            "Exec=gds-files -- %f", f"Icon={app_id}",
            "Categories=System;FileTools;FileManager;", "MimeType=inode/directory;",
            f"StartupWMClass={app_id}", "Terminal=false", "",
        ])
        write(f"usr/share/applications/{app_id}.desktop", "\n".join(desktop))
    if args.component == "files":
        return
    write("usr/share/xdg-desktop-portal/portals/gds.portal", """[portal]
DBusName=org.freedesktop.impl.portal.desktop.gds
Interfaces=org.freedesktop.impl.portal.FileChooser;
""")
    write("usr/share/dbus-1/services/org.freedesktop.impl.portal.desktop.gds.service", f"""[D-BUS Service]
Name=org.freedesktop.impl.portal.desktop.gds
Exec=/{portal_path}
SystemdService=xdg-desktop-portal-gds.service
""")
    write("usr/lib/systemd/user/xdg-desktop-portal-gds.service", f"""[Unit]
Description=GDS Portal
PartOf=graphical-session.target
After=graphical-session.target

[Service]
Type=dbus
BusName=org.freedesktop.impl.portal.desktop.gds
ExecStart=/{portal_path}
Slice=session.slice
""")


if __name__ == "__main__":
    main()
