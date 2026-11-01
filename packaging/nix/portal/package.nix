{ lib, pkgs, rustPlatform, buildTag ? "unknown" }:
let
  manifest = builtins.fromTOML (builtins.readFile ../../../Cargo.toml);
in
rustPlatform.buildRustPackage {
  pname = "xdg-desktop-portal-gds";
  version = manifest.package.version;
  GDS_BUILD_TAG = buildTag;
  src = lib.fileset.toSource {
    root = ../../..;
    fileset = lib.fileset.unions [
      ../../../Cargo.toml
      ../../../Cargo.lock
      ../../../build.rs
      ../../../LICENSE
      ../../../bin
      ../../../macros
      ../../../src
    ];
  };
  cargoLock = import ../cargo-lock.nix;
  cargoBuildFlags = [ "--bin" "xdg-desktop-portal-gds" ];
  buildFeatures = [ "portal" ];
  checkFeatures = [ "portal" ];
  cargoTestFlags = [ "--bin" "xdg-desktop-portal-gds" ];
  nativeCheckInputs = [ pkgs.dbus ];
  preCheck = ''
    export SHELL=${pkgs.runtimeShell}
  '';
  postInstall = ''
    install -Dm644 LICENSE "$out/share/licenses/xdg-desktop-portal-gds/LICENSE"
    mkdir -p "$out/libexec"
    ln -s "$out/bin/xdg-desktop-portal-gds" "$out/libexec/xdg-desktop-portal-gds"
    mkdir -p "$out/share/xdg-desktop-portal/portals" "$out/share/dbus-1/services"
    cat > "$out/share/xdg-desktop-portal/portals/gds.portal" <<EOF
    [portal]
    DBusName=org.freedesktop.impl.portal.desktop.gds
    Interfaces=org.freedesktop.impl.portal.FileChooser;
    EOF
    cat > "$out/share/dbus-1/services/org.freedesktop.impl.portal.desktop.gds.service" <<EOF
    [D-BUS Service]
    Name=org.freedesktop.impl.portal.desktop.gds
    Exec=$out/libexec/xdg-desktop-portal-gds
    SystemdService=xdg-desktop-portal-gds.service
    EOF
    mkdir -p "$out/lib/systemd/user"
    cat > "$out/lib/systemd/user/xdg-desktop-portal-gds.service" <<EOF
    [Unit]
    Description=GDS Portal
    PartOf=graphical-session.target
    After=graphical-session.target

    [Service]
    Type=dbus
    BusName=org.freedesktop.impl.portal.desktop.gds
    ExecStart=$out/libexec/xdg-desktop-portal-gds
    Slice=session.slice
    EOF
  '';
  meta = {
    description = "GDS desktop portal backend";
    platforms = lib.platforms.linux;
    license = lib.licenses.asl20;
    mainProgram = "xdg-desktop-portal-gds";
  };
}
