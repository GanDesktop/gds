flake:
{ config, lib, pkgs, ... }:
let
  cfg = config.programs.gds-files;
  desktopFile = "io.github.GanDesktop.GdsFiles.desktop";
in {
  options.programs.gds-files = import ./options.nix flake { inherit lib pkgs; };

  config = lib.mkIf cfg.enable (lib.mkMerge [
    {
      home.packages = [ cfg.package ];
    }
    (lib.mkIf cfg.defaultFileManager {
      xdg.mimeApps = {
        enable = true;
        defaultApplications."inode/directory" = lib.mkDefault [ desktopFile ];
        associations.added."inode/directory" = [ desktopFile ];
      };
    })
    (lib.mkIf cfg.fileManager1.enable {
      xdg.dataFile."dbus-1/services/org.freedesktop.FileManager1.service".text = ''
        [D-BUS Service]
        Name=org.freedesktop.FileManager1
        Exec=${cfg.package}/bin/gds-files --dbus-service
      '';
    })
  ]);
}
