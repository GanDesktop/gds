# GDS Files Archive Support

GDS Files currently delegates archive files to external archive applications (targeting 7-Zip). Built-in archive browsing, extraction, creation, and modification are planned for future releases.

## External Archive Handling

Archive files such as ZIP, 7z, tar, and tar.gz follow the standard document activation flow: double-clicking or pressing Enter opens the file with the desktop's default application, and “Open with…” allows selecting an external archive manager. GDS Files provides MIME/format recognition and archive icons without adding in-archive browsing, compression, or extraction actions to the primary toolbar or context menus.

Graphical desktop entries and MIME associations for 7-Zip are configured at the desktop environment level while GDS Files uses the existing OpenURI Portal channel. Because upstream Linux 7-Zip releases provide the `7zz` command-line tool rather than a GUI window, NixOS deployments should distinguish between CLI binaries and graphical archive manager desktop entries ([7-Zip download notes](https://www.7-zip.org/download.html)).

Relevant entry points: [`pages/workspace/ui.rs`](../../../bin/files/src/pages/workspace/ui.rs), [`service/desktop.rs`](../../../src/service/desktop.rs), [`service/desktop/portal.rs`](../../../src/service/desktop/portal.rs).

## Future Built-In Scope

The initial built-in phase targets read-only browsing of ZIP, 7z, tar, and tar.gz archives, navigating into subdirectories inside an archive, extracting all or selected entries, and opening archived files on demand. Password prompts and error reporting should integrate with background loading tasks. Modifying archives in place, dragging files into an archive to repack, and writing back edited temporary files belong to a later phase.

Key design areas:

1. **Location and entry model.** Represent the local archive path, internal path, and stable entry identity across breadcrumbs, tabs, back/forward history, sorting, selection, and table/grid views. Archive entries must not masquerade as ordinary local `PathBuf` paths, and duplicate internal names must be disambiguated by entry identity.
2. **Unified background backend.** Centralize format detection, directory indexing, entry streaming, and extraction behind a single background service interface, reusing GDS Files task cancellation, progress reporting, and stale-navigation guards.
3. **External application and clipboard handoff.** Extract temporary files on demand when opening an archived entry in an external application and manage their cleanup lifecycle. Convert archive entry sources when copying, pasting, or dragging out to the system file clipboard.
4. **Extraction semantics.** Preserve the non-overwriting target policy, handle cancellation and partial failures cleanly, and validate non-UTF-8 names, symlinks, and path-traversal boundaries so extraction never escapes the destination directory.

Related entry points: [`entities/location/model/address.rs`](../../../bin/files/src/entities/location/model/address.rs), [`entities/file/api/directory.rs`](../../../bin/files/src/entities/file/api/directory.rs), [`entities/file/api/directory/content.rs`](../../../bin/files/src/entities/file/api/directory/content.rs), [`features/file_operations/api/transfer.rs`](../../../bin/files/src/features/file_operations/api/transfer.rs), [`shared/lib/clipboard.rs`](../../../bin/files/src/shared/lib/clipboard.rs).

## Backend Candidates and Format Considerations

| Format | Rust crate candidate | Integration considerations |
| --- | --- | --- |
| ZIP | [`zip`](https://docs.rs/zip/latest/zip/read/struct.ZipArchive.html) | Central directory indexing, filename encodings, password decryption, and compression methods; whole-archive extraction helpers must be adapted to GDS Files' non-overwriting target policy. |
| 7z | [`sevenz-rust2`](https://docs.rs/sevenz-rust2/latest/sevenz_rust2/) | Solid-block decompression, password decryption, encrypted headers, and compression method coverage. |
| tar | [`tar`](https://docs.rs/tar/latest/tar/struct.Archive.html) | Sequential entry scanning, directory indexing, symlinks/hardlinks, and permissions; extraction must enforce GDS Files' conflict policy rather than silently merging directories. |
| tar.gz | `tar` with [`flate2`](https://docs.rs/flate2/latest/flate2/) | Sequential gzip stream wrapping a tar archive; requires managing repeated-access overhead and cache usage. |

An alternative unified backend is [libarchive](https://github.com/libarchive/libarchive/wiki/LibarchiveFormats) (for example via [`compress-tools`](https://docs.rs/compress-tools/latest/compress_tools/)). However, libarchive 3.8.11 still rejects encrypted 7z payloads and headers ([`archive_read_support_format_7zip.c`](https://github.com/libarchive/libarchive/blob/v3.8.11/libarchive/archive_read_support_format_7zip.c)), so password-protected 7z support must be verified explicitly before selecting a backend.

Because `tar` streams sequentially and solid `7z` archives may require decoding earlier entries in the same block ([`tar::Archive`](https://docs.rs/tar/latest/tar/struct.Archive.html), [`sevenz_rust2::ArchiveReader`](https://docs.rs/sevenz-rust2/latest/sevenz_rust2/struct.ArchiveReader.html)), the UI must use background indexing and on-demand extraction with bounded caching rather than assuming instant random access.
