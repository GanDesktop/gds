{ pkgs }:
let
  inherit (pkgs.callPackage ./rust.nix { }) toolchain;
  libraries = with pkgs; [
    wayland libxkbcommon libx11 libxcursor libxi libxrandr fontconfig freetype libGL vulkan-loader libxcb sqlite alsa-lib
  ];
  runtimePrograms = with pkgs; [ xdg-terminal-exec ];
  libraryPath = "/run/opengl-driver/lib:${pkgs.lib.makeLibraryPath libraries}";
  runtime = pkgs.writeShellScript "gds-development-runtime" ''
    export PATH="${pkgs.lib.makeBinPath runtimePrograms}''${PATH:+:$PATH}"
    export LD_LIBRARY_PATH="${libraryPath}''${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
    export TERMINFO_DIRS="''${TERMINFO_DIRS:+$TERMINFO_DIRS:}${pkgs.ncurses}/share/terminfo"
    export XDG_DATA_DIRS="''${XDG_DATA_DIRS:-/usr/local/share:/usr/share}:${pkgs.shared-mime-info}/share"
    exec "$@"
  '';
in
pkgs.mkShell {
  packages = [ toolchain ] ++ (with pkgs; [
    bashInteractive just nil just-lsp pkg-config clang lld cmake python3 ncurses dbus git
  ]) ++ runtimePrograms;
  buildInputs = libraries;
  # FHS shells (e.g. steam-run) inject -L/usr/lib here, selecting a foreign glibc.
  # Unlike NIX_LDFLAGS, this variable is not rebuilt by the stdenv setup hooks.
  NIX_CFLAGS_LINK = "";
  GDS_DEV_RUNTIME = runtime;
  shellHook = ''
    export LD_LIBRARY_PATH="${libraryPath}''${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
    export TERMINFO_DIRS="''${TERMINFO_DIRS:+$TERMINFO_DIRS:}${pkgs.ncurses}/share/terminfo"
    export XDG_DATA_DIRS="''${XDG_DATA_DIRS:-/usr/local/share:/usr/share}:${pkgs.shared-mime-info}/share"
  '';
}
