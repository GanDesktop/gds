flake:
{ config, lib, pkgs, ... }:
let
  cfg = config.programs.gds-images;
in {
  options.programs.gds-images = import ./options.nix flake { inherit lib pkgs; };
  config = lib.mkIf cfg.enable {
    environment.systemPackages = [ cfg.package ];
  };
}
