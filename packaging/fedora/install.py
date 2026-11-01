#!/usr/bin/env python3
"""Stage the Fedora packages, sharing the application's desktop translations."""

from pathlib import Path
import shutil
import sys
import tomllib


def main():
    root, binaries = map(Path, sys.argv[1:])
    project = Path(__file__).resolve().parents[2]

    def install(source, destination):
        path = root / destination
        path.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, path)

    def write(destination, content):
        path = root / destination
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content)

    for binary, destination in [
        ("gds-files", "usr/bin/gds-files"),
        ("xdg-desktop-portal-gds", "usr/libexec/xdg-desktop-portal-gds"),
    ]:
        install(binaries / binary, destination)
        install(project / "LICENSE", f"usr/share/licenses/{binary}/LICENSE")

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
    write("usr/share/xdg-desktop-portal/portals/gds.portal", """[portal]
DBusName=org.freedesktop.impl.portal.desktop.gds
Interfaces=org.freedesktop.impl.portal.FileChooser;
""")
    write("usr/share/dbus-1/services/org.freedesktop.impl.portal.desktop.gds.service", """[D-BUS Service]
Name=org.freedesktop.impl.portal.desktop.gds
Exec=/usr/libexec/xdg-desktop-portal-gds
SystemdService=xdg-desktop-portal-gds.service
""")
    write("usr/lib/systemd/user/xdg-desktop-portal-gds.service", """[Unit]
Description=GDS Portal
PartOf=graphical-session.target
After=graphical-session.target

[Service]
Type=dbus
BusName=org.freedesktop.impl.portal.desktop.gds
ExecStart=/usr/libexec/xdg-desktop-portal-gds
Slice=session.slice
""")


if __name__ == "__main__":
    main()
