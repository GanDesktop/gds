{ pkgs ? import <nixpkgs> { }
, buildTag ? "unknown"
}:
{
  gds-files = pkgs.callPackage ./apps/files/package.nix { inherit buildTag; };
  xdg-desktop-portal-gds = pkgs.callPackage ./portal/package.nix { inherit buildTag; };
}
