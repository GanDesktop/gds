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
  options.programs.gds-files = import ./options.nix flake { inherit lib pkgs; };

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
