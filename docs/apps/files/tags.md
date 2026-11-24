# File Tags & URI Schemes

GDS Files exchanges per-file tags with KDE Dolphin via the `user.xdg.tags` extended attribute and queries hierarchical tag catalogs (`tags:/`) through the system Baloo index and KIO CLI tools.

## Configuration & Dependencies

Tag browsing is disabled by default. Enable it under **Settings → Extensions → Tags** once backend detection verifies:

- `balooctl6` / `balooctl` (index status check)
- `kioclient6` / `kioclient` / `kioclient5` (tag enumeration and global tag deletion via Baloo's `tags:` KIO worker, typically provided by `kde-cli-tools`)

```toml
[tags]
enabled = false
backend = "baloo"
# control = "/custom/bin/balooctl6"
# kio_client = "/custom/bin/kioclient"
```

GDS Files neither starts nor configures the Baloo daemon or its index scopes. Trade-offs and CLI limitations (such as Baloo 6.31's quote handling and unpaginated CLI output) are documented in [External Tool Integrations](../../decisions/external-integrations.md).

## Browsing & Editing

- **`tags:/` navigation**: `tags:/` lists root tags and `tags:/work/design` browses hierarchical subtags in table or grid view with full history, breadcrumb, filter, and tab-pinning support. Pure tag folders show only the Name column; folders containing indexed local files display full file columns and enable the details panel.
- **Inline tag editor**: Available from file context menus, Details (`Ctrl+P`), and Properties (`Enter` adds a tag, `Backspace` removes the last tag, `Ctrl+Z` / `Ctrl+Shift+Z` undo/redo). Saving re-reads `user.xdg.tags` and applies only the user's delta so tags added concurrently by another app are preserved.
- **Copy preservation & global removal**: `gds::service::file_tags` (`file-tags` Cargo feature) reads/writes comma-separated UTF-8 `user.xdg.tags` values and preserves them across local copies and cross-filesystem moves. Deleting a tag entry in `tags:/` removes that tag (and its subtags) from indexed files via Baloo/KIO after confirmation without trashing the underlying files. SFTP/SMB files do not support xattr tags.

## Supported URI Schemes

| URI | Meaning |
| --- | --- |
| `file:///…` | Local file or directory |
| `sftp://…`, `smb://…` | Native remote SFTP or SMB location |
| `trash:///`, `recent:///`, `network:///` | Standard virtual pages (also accepts `scheme:/`, `recentlyused:/`, `remote:/`, and legacy `location:/…`) |
| `location:/favorites`, `location:/volumes` | GDS built-in virtual pages (also accepts `starred:///` and `computer:///`) |
| `location:/volume/<stable\|attachment>/<id>?path=<relative-path>` | Device-identity-bound volume path |
| `tags:/`, `tags:/work` | Tag catalog and tag contents (also accepts `tags:///…`; non-empty host `tags://host/…` is invalid) |

For E2E and opt-in real-Baloo test commands, see [Files Testing](testing.md).

## References

- [KFileMetaData `user.xdg.tags` encoding](https://github.com/KDE/kfilemetadata/blob/master/src/usermetadata.cpp)
- [Baloo `tags:` KIO worker](https://github.com/KDE/baloo/blob/master/src/kioworkers/tags/kio_tags.cpp)
- [RFC 3986 (URI)](https://www.rfc-editor.org/rfc/rfc3986.html#section-3) & [RFC 3987 (IRI)](https://www.rfc-editor.org/rfc/rfc3987.html)
