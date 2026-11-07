flake:
{ config, lib, pkgs, ... }:
let
  cfg = config.programs.gds-agents;
in {
  options.programs.gds-agents = import ./options.nix flake { inherit lib pkgs; };

  config = lib.mkIf cfg.enable {
    home.packages = [ cfg.package ];
  };
}
