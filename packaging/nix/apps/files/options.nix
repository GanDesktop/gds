flake:
{ lib, pkgs, ... }:
{
  enable = lib.mkEnableOption "GDS Files";
  package = lib.mkOption {
    type = lib.types.package;
    default = flake.packages.${pkgs.stdenv.hostPlatform.system}.gds-files;
    defaultText = lib.literalExpression "inputs.gds.packages.\${pkgs.stdenv.hostPlatform.system}.gds-files";
    description = "GDS Files package to install.";
  };
  defaultFileManager = lib.mkOption {
    type = lib.types.bool;
    default = true;
    description = "Set the default handler for inode/directory.";
  };
  fileManager1.enable = lib.mkEnableOption "GDS Files FileManager1 D-Bus activation";
}
