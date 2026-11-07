flake:
{ lib, pkgs, ... }:
{
  enable = lib.mkEnableOption "GDS Agents";
  package = lib.mkOption {
    type = lib.types.package;
    default = flake.packages.${pkgs.stdenv.hostPlatform.system}.gds-agents;
    defaultText = lib.literalExpression "inputs.gds.packages.\${pkgs.stdenv.hostPlatform.system}.gds-agents";
    description = "GDS Agents frontend package. Bas runs as a separate service.";
  };
}
