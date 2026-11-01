{ lib, pkgs, rustPlatform, makeWrapper, makeDesktopItem, copyDesktopItems, pkg-config, cmake
, application, desktopItem, description, postInstall ? "", checkFlags ? [ ]
, extraBuildInputs ? [ ], extraNativeCheckInputs ? [ ]
, buildTag ? "unknown"
}:
let
  binary = "gds-${application}";
  manifest = builtins.fromTOML (builtins.readFile ../../Cargo.toml);
  target = lib.findFirst (target: target.name == binary)
    (throw "Cargo.toml does not declare ${binary}") manifest.bin;
  features = target.required-features or [ ];
  labels = builtins.fromTOML (builtins.readFile (../../assets + "/${application}/desktop.toml"));
  translations = lib.concatMapAttrs (locale: entries:
    lib.mapAttrs' (key: value: lib.nameValuePair "${key}[${locale}]" value) entries
  ) (builtins.removeAttrs labels [ "default" ]);
  libraries = with pkgs; [
    wayland libxkbcommon fontconfig freetype openssl zstd zlib
    libGL vulkan-loader dbus libx11 libxcb
  ] ++ extraBuildInputs;
in
rustPlatform.buildRustPackage {
  pname = binary;
  version = manifest.package.version;
  GDS_BUILD_TAG = buildTag;

  src = lib.fileset.toSource {
    root = ../../.;
    fileset = lib.fileset.unions [
      ../../Cargo.toml
      ../../Cargo.lock
      ../../build.rs
      ../../LICENSE
      ../../assets
      ../../bin
      ../../macros
      ../../src
    ];
  };

  cargoLock = import ./cargo-lock.nix;

  nativeBuildInputs = [ pkg-config cmake makeWrapper copyDesktopItems rustPlatform.bindgenHook ];
  nativeCheckInputs = [ pkgs.dbus pkgs.ffmpeg-headless ] ++ extraNativeCheckInputs;
  buildInputs = libraries;
  dontUseCmakeConfigure = true;
  cargoBuildFlags = [ "--bin" binary ];
  buildFeatures = features;
  checkFeatures = features ++ [ "gpui-kit/test-support" ];
  cargoTestFlags = [ "--bin" binary "--lib" ];

  desktopItems = [ (makeDesktopItem (desktopItem // {
    desktopName = labels.default.Name;
    genericName = labels.default.GenericName;
    comment = labels.default.Comment;
    extraConfig = (desktopItem.extraConfig or { }) // translations;
    terminal = false;
    startupWMClass = desktopItem.name;
  })) ];

  inherit checkFlags;
  postInstall = ''
    install -Dm644 LICENSE "$out/share/licenses/${binary}/LICENSE"
    install -Dm644 assets/${application}/icon-desktop.svg "$out/share/icons/hicolor/scalable/apps/${desktopItem.name}.svg"
    ${postInstall}
  '';

  postFixup = ''
    wrapProgram "$out/bin/${binary}" \
      --prefix PATH : ${lib.makeBinPath ([ pkgs.xdg-utils pkgs.xdg-terminal-exec ] ++ lib.optionals (application == "files") [ pkgs.ffmpeg-headless ])} \
      --suffix TERMINFO_DIRS : ${pkgs.ncurses}/share/terminfo \
      --prefix LD_LIBRARY_PATH : "/run/opengl-driver/lib:${lib.makeLibraryPath libraries}"
  '';

  meta = {
    inherit description;
    homepage = "https://github.com/GanDesktop/gds";
    mainProgram = binary;
    license = lib.licenses.asl20;
    platforms = lib.platforms.linux;
  };
}
