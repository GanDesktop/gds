# GDS Files Tags and URI Schemes

GDS Files shares file tags with Dolphin through the `user.xdg.tags` extended attribute and queries tag directories and associated files through the system Baloo index. A standalone embedded index backend is not implemented.

## Configuration and Dependencies

Tag support is disabled by default. Enable it under **Settings → Extensions → Tags** after backend detection succeeds. Changes apply immediately and save automatically; when the backend is unavailable, GDS Files reports the specific error while preserving the saved preference.

Tag queries depend on Baloo's `tags:` KIO worker and the following executables:

- `balooctl6` / `balooctl`: checks index status.
- `kioclient6` / `kioclient` / `kioclient5`: queries and removes tags (typically provided by `kde-cli-tools`).

Executables are discovered from `PATH` by default or configured explicitly. Detection verifies command versions, `balooctl status`, and `kioclient ls tags:/`; tag support cannot be newly enabled when commands fail, indexing is disabled, or the worker is missing.

```toml
[tags]
enabled = false
backend = "baloo"
# control = "/custom/bin/balooctl6"
# kio_client = "/custom/bin/kioclient"
```

GDS Files does not start or configure the indexer, nor does it manage scan scopes, exclusion rules, or the index database. Global tag queries depend on Baloo's index state and support manual refresh; open local directories re-read file tags through attribute monitoring.

## Browsing and Editing

- `tags:/` displays the root tag catalogue, and `tags:/work` displays the contents of a tag, with `/` denoting hierarchy. Table view, grid view, filtering, keyboard navigation, breadcrumbs, and history all work with tag locations. Mouse back/forward buttons and toolbar buttons share navigation history, and Alt+Up navigates to the parent level.
- Pure tag directories display only the name column and have no file details panel. Directories containing real files display name, modification time, type, and size columns and provide the details panel. Full addresses are shown on hover.
- Context menus group open, copy, organize, and refresh actions. Real files support copying, revealing in their containing folder, and editing tags; tag collections do not accept paste or new-folder creation.
- Selecting a tag exposes **Delete tag** in the toolbar, context menu, and compact menu, sharing a confirmation dialog with Delete and Shift+Delete. Deletion uses Baloo/KIO to remove the tag and its subtags from indexed files while keeping the files and their remaining tags intact; it does not use the Trash or support undo. Deleting from Favorites only removes the bookmark.
- Tag, recent, favorite, remote, and volume locations can all be pinned as tabs and restore their order on launch.
- The details panel and properties window display tags. Read-only tags match the font size and line height of other metadata attributes, wrap across lines, and truncate overly long names with a hover tooltip.
- The tag editor uses `gpui-kit` `Tag` elements with inline input: Enter adds a tag, Backspace on an empty input removes the last tag, and Ctrl+Z / Ctrl+Shift+Z (or Ctrl+Y) undo and redo both text and tag edits. Changes are written only when saved; cancelling leaves the file untouched.

## Data and Abstraction Boundaries

`gds::service::file_tags` owns single-file tag reading, writing, and copy preservation behind the `file-tags` Cargo feature. It depends on `desktop`, while `files` explicitly enables `file-tags`. Runtime preferences remain independent of compile-time features.

The file's `user.xdg.tags` extended attribute is the source of truth: UTF-8 text separated by ASCII commas without CSV escaping, removed when empty. Tags preserve case, spaces, and hierarchy; they cannot contain ASCII commas, NUL bytes, or newlines.

Before saving, the editor re-reads `user.xdg.tags` and applies only the additions and removals relative to when the editor opened, preserving tags added concurrently by other applications. Extended attributes do not provide compare-and-swap, so atomic concurrent merges are not guaranteed. Local copies and cross-filesystem moves preserve tags before restoring read-only permissions; if tag preservation fails, the error is reported and cross-device moves keep the source file. Symlinks read the target's tags, editing requires opening the target, and copying a symlink does not modify its target. Native SFTP and SMB locations do not support tag reading or writing.

Location schemes handle URI parsing, navigation, and browsing semantics, while the tag backend handles tag enumeration, lookup, and global tag removal. Tag entries and local file targets remain distinct in the location model and are never converted into fictitious local paths.

| Address | Meaning |
| --- | --- |
| `file://…` | Local file or directory |
| `sftp://…`, `smb://…` | Remote location |
| `location:/recent`, `/favorites`, `/volumes`, `/network`, `/trash` | Built-in collections (`location:` scheme) |
| `location:/volume/<stable\|attachment>/<id>?path=<relative-path>` | Volume location preserving device identity |
| `tags:/`, `tags:/work` | Tag catalogue and tag contents |

`location://…` and `tags://name` are not supported. The address bar displays readable Unicode while preserving required `%`, `?`, and `#` escapes; persistence and command arguments use canonical encoding.

## Queries and Limitations

The Baloo backend enumerates entries with `kioclient ls`, inspects entries to distinguish child tags from real `LOCAL_PATH` targets, and removes tags via `kioclient remove tags:/…` using the same KIO worker as Dolphin. Commands are invoked with argument arrays (never through a shell) with a 15-second timeout; navigating away or changing settings cancels in-flight queries so stale results never overwrite newer navigation. No Qt or C++ libraries are linked into the binary.

- The KDE Baloo 6.31 `tags` worker returns an empty set for tag queries containing double quotes (`"`); GDS Files reports an explicit error for those queries and global deletions, while per-file attribute reading, editing, and copy preservation still work.
- Line-oriented CLI output cannot losslessly represent tag names containing newlines.
- Queries are unpaginated and unsuitable for very large tag sets; KIO CLI text output can vary across KDE releases.

Test commands, fixture requirements, and desktop verification steps are documented in [Files testing](testing.md).

## Upstream References

- [KFileMetaData tag encoding](https://github.com/KDE/kfilemetadata/blob/master/src/usermetadata.cpp)
- [Baloo tags KIO worker](https://github.com/KDE/baloo/blob/master/src/kioworkers/tags/kio_tags.cpp)
- [Dolphin Baloo integration](https://github.com/KDE/dolphin/blob/master/CMakeLists.txt)
- [URI Syntax (RFC 3986)](https://www.rfc-editor.org/rfc/rfc3986.html#section-3) and [IRIs (RFC 3987)](https://www.rfc-editor.org/rfc/rfc3987.html)
