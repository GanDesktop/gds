flake:
{ config, lib, pkgs, ... }:
let
  cfg = config.programs.gds-files;
  desktopFile = "io.github.GanDesktop.GdsFiles.desktop";
  activation = pkgs.writeTextDir "share/dbus-1/services/org.freedesktop.FileManager1.service" ''
    [D-BUS Service]
    Name=org.freedesktop.FileManager1
    Exec=${cfg.package}/bin/gds-files --dbus-service
  '';
in {
  options.programs.gds-files = {
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
      description = "Set the system default handler for inode/directory. User MIME settings take precedence.";
    };
    fileManager1.enable = lib.mkEnableOption "GDS Files FileManager1 D-Bus activation";
  };

  config = lib.mkIf cfg.enable (lib.mkMerge [
    {
      environment.systemPackages = [ cfg.package ];
      services.udisks2.enable = lib.mkDefault true;
    }
    (lib.mkIf cfg.defaultFileManager {
      xdg.mime.enable = true;
      xdg.mime.defaultApplications."inode/directory" = lib.mkDefault [ desktopFile ];
      xdg.mime.addedAssociations."inode/directory" = [ desktopFile ];
    })
    (lib.mkIf cfg.fileManager1.enable {
      environment.systemPackages = [ activation ];
      services.dbus.packages = [ activation ];
    })
  ]);
}
