{ pkgs ? import <nixpkgs> { }
, buildTag ? "v${(builtins.fromTOML (builtins.readFile ../../Cargo.toml)).package.version}"
}:
let
  applications = builtins.attrNames (pkgs.lib.filterAttrs
    (_: type: type == "directory") (builtins.readDir ./apps));
  packages = pkgs.lib.genAttrs (map (name: "gds-${name}") applications) (name:
    pkgs.callPackage (./apps + "/${pkgs.lib.removePrefix "gds-" name}/package.nix") {
      inherit buildTag;
    });
in packages // {
  gds = pkgs.buildEnv {
    name = "gds";
    paths = builtins.attrValues packages;
    meta = {
      description = "GDS native desktop application suite";
      homepage = "https://github.com/GanDesktop/gds";
      license = pkgs.lib.licenses.asl20;
      platforms = pkgs.lib.platforms.linux;
    };
  };
  xdg-desktop-portal-gds = pkgs.callPackage ./portal/package.nix { inherit buildTag; };
}
