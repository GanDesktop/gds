{ callPackage, rustPlatform, buildTag ? "unknown" }:
callPackage ../../package.nix {
  inherit buildTag rustPlatform;
  application = "images";
  description = "Native image viewer and annotation editor for Wayland and X11";
  # The Nix build directory does not support the user xattrs exercised by these tests.
  checkFlags = [
    "--skip=service::file_tags::tests::associations_and_copy_preserve_other_tags"
    "--skip=service::file_tags::tests::comment_round_trip_clears_validates_and_rejects_symlinks_and_fifos"
    "--skip=service::file_tags::tests::copy_from_preserves_portable_user_xattrs_and_reads_origin_and_mime"
    "--skip=service::file_tags::tests::editor_delta_retains_unrelated_external_changes"
    "--skip=service::file_tags::tests::apply_edit_works_on_directories_and_clears_removed_tags"
    "--skip=service::file_tags::tests::apply_edit_rejects_symlinks_and_fifos_without_blocking_or_following"
    "--skip=service::file_tags::tests::copy_from_writes_files_and_dirs_and_rejects_symlink_or_fifo_destinations"
  ];
  desktopItem = {
    name = "io.github.GanDesktop.GdsImages";
    exec = "gds-images -- %f";
    icon = "io.github.GanDesktop.GdsImages";
    categories = [ "Graphics" "2DGraphics" "RasterGraphics" "Viewer" ];
    mimeTypes = [ "image/png" "image/jpeg" "image/webp" "image/gif" "image/svg+xml"
      "image/svg+xml-compressed" "image/bmp" "image/tiff" "image/x-icon" "image/x-tga"
      "image/x-portable-anymap" "image/qoi" ];
    startupNotify = true;
  };
  postInstall = ''
    install -Dm644 ${./niri.kdl} "$out/share/gds-images/niri.kdl"
  '';
}
