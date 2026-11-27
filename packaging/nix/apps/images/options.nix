flake:
{ lib, pkgs, ... }:
{
  enable = lib.mkEnableOption "GDS Images";
  package = lib.mkOption {
    type = lib.types.package;
    default = flake.packages.${pkgs.stdenv.hostPlatform.system}.gds-images;
    defaultText = lib.literalExpression "inputs.gds.packages.\${pkgs.stdenv.hostPlatform.system}.gds-images";
    description = "GDS Images package to install.";
  };
}
