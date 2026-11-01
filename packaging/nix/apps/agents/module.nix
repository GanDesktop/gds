flake:
{ config, lib, pkgs, ... }:
let
  cfg = config.programs.gds-agents;
in {
  options.programs.gds-agents = {
    enable = lib.mkEnableOption "GDS Agents";
    package = lib.mkOption {
      type = lib.types.package;
      default = flake.packages.${pkgs.stdenv.hostPlatform.system}.gds-agents;
      defaultText = lib.literalExpression "inputs.gds.packages.\${pkgs.stdenv.hostPlatform.system}.gds-agents";
      description = "GDS Agents frontend package. Bas runs as a separate service.";
    };
  };

  config = lib.mkIf cfg.enable {
    environment.systemPackages = [ cfg.package ];
  };
}
