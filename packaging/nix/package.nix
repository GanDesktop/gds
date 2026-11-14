{ lib, pkgs, rustPlatform, makeWrapper, makeDesktopItem, copyDesktopItems, pkg-config
, application, desktopItem, description, postInstall ? "", checkFlags ? [ ]
, extraNativeBuildInputs ? [ ], extraBuildInputs ? [ ], extraNativeCheckInputs ? [ ]
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
    wayland libxkbcommon fontconfig freetype libGL vulkan-loader libxcb
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

  nativeBuildInputs = [ pkg-config makeWrapper copyDesktopItems ] ++ extraNativeBuildInputs;
  nativeCheckInputs = [ pkgs.dbus pkgs.ffmpeg-headless pkgs.util-linux ] ++ extraNativeCheckInputs;
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
  preCheck = lib.optionalString (application == "files") ''
    export XDG_DATA_DIRS="${pkgs.shared-mime-info}/share''${XDG_DATA_DIRS:+:$XDG_DATA_DIRS}"
  '';
  postInstall = ''
    install -Dm644 LICENSE "$out/share/licenses/${binary}/LICENSE"
    install -Dm644 assets/${application}/icon-desktop.svg "$out/share/icons/hicolor/scalable/apps/${desktopItem.name}.svg"
    ${postInstall}
  '';

  postFixup = ''
    wrapProgram "$out/bin/${binary}" \
      --suffix TERMINFO_DIRS : ${pkgs.ncurses}/share/terminfo \
      ${lib.optionalString (application == "files") "--suffix XDG_DATA_DIRS : ${pkgs.shared-mime-info}/share"} \
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
