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
    parser.add_argument("--component", choices=["all", "files", "images", "portal"], default="all")
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
        ("images", "gds-images", "usr/bin/gds-images"),
        ("portal", "xdg-desktop-portal-gds", portal_path),
    ]:
        if args.component not in ("all", component):
            continue
        install(binaries / binary, destination)
        install(project / "LICENSE", f"usr/share/licenses/{binary}/LICENSE")

    for app, app_id, arguments, categories, mime_types in [
        ("files", "io.github.GanDesktop.GdsFiles", "%U", "System;FileTools;FileManager;", "inode/directory;"),
        ("images", "io.github.GanDesktop.GdsImages", "%f", "Graphics;2DGraphics;RasterGraphics;Viewer;",
         "image/png;image/jpeg;image/webp;image/gif;image/svg+xml;image/svg+xml-compressed;image/bmp;image/tiff;image/x-icon;image/x-tga;image/x-portable-anymap;image/qoi;"),
    ]:
        if args.component not in ("all", app):
            continue
        assets = project / "assets" / app
        install(assets / "icon-desktop.svg", f"usr/share/icons/hicolor/scalable/apps/{app_id}.svg")
        if app == "files":
            install(assets / f"{app_id}.metainfo.xml", f"usr/share/metainfo/{app_id}.metainfo.xml")
        install(project / "src/appearance/theme/provider/system/noctalia/assets/gds.json",
                f"usr/share/gds-{app}/noctalia/gds.json")
        labels = tomllib.loads((assets / "desktop.toml").read_text())
        desktop = ["[Desktop Entry]", "Type=Application"]
        for locale, entries in labels.items():
            suffix = "" if locale == "default" else f"[{locale}]"
            desktop.extend(f"{key}{suffix}={value}" for key, value in entries.items())
        desktop.extend([f"Exec=gds-{app} -- {arguments}", f"Icon={app_id}",
                        f"Categories={categories}", f"MimeType={mime_types}", "StartupNotify=true",
                        f"StartupWMClass={app_id}", "Terminal=false"])
        if app == "files":
            desktop.extend(["Actions=new-window;", "", "[Desktop Action new-window]", "Name=New Window",
                            "Name[zh_CN]=新建窗口", "Exec=gds-files --new-window"])
        write(f"usr/share/applications/{app_id}.desktop", "\n".join([*desktop, ""]))
    if args.component not in ("all", "portal"):
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
