{ callPackage, buildTag ? "unknown" }:
callPackage ../../package.nix {
  inherit buildTag;
  application = "files";
  description = "Native GPUI file manager and file chooser";
  # Nix's Linux sandbox rejects chmod with setuid/setgid bits and mounts /build on
  # tmpfs, which does not support user.* extended attributes (EOPNOTSUPP).
  checkFlags = [
    "--skip=directory::tests::permissions_owner_group_and_access_time_come_from_real_metadata"
    "--skip=service::file_tags::tests::associations_and_copy_preserve_other_tags"
    "--skip=service::file_tags::tests::editor_delta_retains_unrelated_external_changes"
    "--skip=service::file_tags::tests::apply_edit_works_on_directories_and_clears_removed_tags"
    "--skip=service::file_tags::tests::apply_edit_rejects_symlinks_and_fifos_without_blocking_or_following"
    "--skip=service::file_tags::tests::copy_from_writes_files_and_dirs_and_rejects_symlink_or_fifo_destinations"
    "--skip=watching::changes::tests::external_xattr_change_updates_metadata_and_keeps_canonical_entry_order"
    "--skip=transfer::tests::copies_file_and_directory_tags_before_restoring_readonly_permissions"
    "--skip=transfer::tests::cross_device_cleanup_preserves_tags_changed_after_copy"
  ];
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
