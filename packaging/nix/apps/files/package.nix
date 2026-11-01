{ callPackage, buildTag ? "unknown" }:
callPackage ../../package.nix {
  inherit buildTag;
  application = "files";
  description = "Native GPUI file manager and file chooser";
  # Nix's Linux sandbox rejects chmod with setuid/setgid bits.
  checkFlags = [ "--skip=directory::tests::permissions_owner_group_and_access_time_come_from_real_metadata" ];
  desktopItem = {
    name = "io.github.GanDesktop.GdsFiles";
    exec = "gds-files -- %f";
    icon = "io.github.GanDesktop.GdsFiles";
    categories = [ "System" "FileTools" "FileManager" ];
    mimeTypes = [ "inode/directory" ];
  };
  postInstall = ''
    install -Dm644 ${./niri.kdl} "$out/share/gds-files/niri.kdl"
  '';
}
