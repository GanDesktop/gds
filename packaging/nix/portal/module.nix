flake:
{ config, lib, pkgs, ... }:
let
  cfg = config.services.gds-portal;
in {
  options.services.gds-portal = {
    enable = lib.mkEnableOption "GDS Portal";
    package = lib.mkOption {
      type = lib.types.package;
      default = flake.packages.${pkgs.stdenv.hostPlatform.system}.xdg-desktop-portal-gds;
      defaultText = lib.literalExpression "inputs.gds.packages.\${pkgs.stdenv.hostPlatform.system}.xdg-desktop-portal-gds";
      description = "GDS Portal backend package to install.";
    };
    desktops = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ "niri" ];
      description = "Desktop-specific portal configurations in addition to the common configuration.";
    };
  };

  config = lib.mkIf cfg.enable {
    xdg.portal.enable = true;
    xdg.portal.extraPortals = [ cfg.package pkgs.xdg-desktop-portal-gtk ];
    xdg.portal.config = lib.mkMerge [
      { common.default = lib.mkDefault [ "gtk" ]; }
      (lib.genAttrs ([ "common" ] ++ cfg.desktops) (_: {
        "org.freedesktop.impl.portal.FileChooser" = lib.mkDefault [ "gds" ];
        "org.freedesktop.impl.portal.AppChooser" = lib.mkDefault [ "gtk" ];
      }))
    ];
  };
}
