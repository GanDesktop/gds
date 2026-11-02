{ pkgs }:
let
  libraries = with pkgs; [
    wayland libxkbcommon fontconfig freetype openssl zstd zlib
    libGL vulkan-loader dbus libx11 libxcb alsa-lib
  ];
  runtimePrograms = with pkgs; [ ffmpeg-headless xdg-terminal-exec ];
  libraryPath = "/run/opengl-driver/lib:${pkgs.lib.makeLibraryPath libraries}";
  runtime = pkgs.writeShellScript "gds-development-runtime" ''
    export PATH="${pkgs.lib.makeBinPath runtimePrograms}''${PATH:+:$PATH}"
    export LD_LIBRARY_PATH="${libraryPath}''${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
    export TERMINFO_DIRS="''${TERMINFO_DIRS:+$TERMINFO_DIRS:}${pkgs.ncurses}/share/terminfo"
    exec "$@"
  '';
in
pkgs.mkShell {
  packages = with pkgs; [
    cargo rustc rustfmt clippy just nil just-lsp pkg-config clang lld cmake python3 ncurses git
  ] ++ runtimePrograms;
  buildInputs = libraries;
  # FHS shells (e.g. steam-run) inject -L/usr/lib here, selecting a foreign glibc.
  # Unlike NIX_LDFLAGS, this variable is not rebuilt by the stdenv setup hooks.
  NIX_CFLAGS_LINK = "";
  LIBCLANG_PATH = "${pkgs.libclang.lib}/lib";
  GDS_DEV_RUNTIME = runtime;
  shellHook = ''
    export LD_LIBRARY_PATH="${libraryPath}''${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
    export TERMINFO_DIRS="''${TERMINFO_DIRS:+$TERMINFO_DIRS:}${pkgs.ncurses}/share/terminfo"
  '';
}
