{ pkgs ? import <nixpkgs> { }
, buildTag ? "unknown"
}:
let
  inherit (pkgs.callPackage ./rust.nix { }) rustPlatform;
in
{
  gds-images = pkgs.callPackage ./apps/images/package.nix { inherit buildTag rustPlatform; };
  gds-files = pkgs.callPackage ./apps/files/package.nix { inherit buildTag rustPlatform; };
  xdg-desktop-portal-gds = pkgs.callPackage ./portal/package.nix { inherit buildTag rustPlatform; };
}
