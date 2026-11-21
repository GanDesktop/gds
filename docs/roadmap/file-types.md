# File Type Detection and Display in GDS Files

Use Linux `shared-mime-info` as the primary source of truth for MIME detection, localized type descriptions, and generic icon hints, supplemented by a generated GitHub Linguist language table for source-code classification and modern developer extensions. GDS Files already uses `shared-mime-info` for detection. Reading its descriptions lets the Detailed Type column and Properties show recognizable format names in the app's language without maintaining a large manual mapping table. GDS owns the mapping from generic icon hints to its file categories and icons. [Shared MIME-info specification](https://specifications.freedesktop.org/shared-mime-info/latest-single/)

## Current behavior

- **Detection.** `xdg-mime` matches the full filename against installed plain-text rules, including compound extensions and literal names. GDS Files takes the first MIME candidate from an unordered `HashSet` and normalizes aliases. A local file with no usable filename match can be classified using a header probe bounded to 8 KiB, then a text or binary heuristic. [Detection code](../../bin/files/src/entities/file/api/directory/content.rs)
- **Display.** `mime-types.toml` contains 109 presentation rules listing 166 MIME strings, including aliases. It supplies categories and untranslated short names such as `Rust` and `PNG`. For known MIME types outside the table, the detailed label falls back to the final extension in uppercase or to the MIME string. An unlisted `application/*` type falls into the Binary category and receives a generic file icon. Properties also overrides the Format row for `FileFacts::Text` and media decoders instead of sharing the Detailed Type lookup. [Presentation table](../../bin/files/src/entities/file/api/directory/content/assets/mime-types.toml) · [Labels](../../bin/files/src/entities/file/ui/presentation.rs) · [Icons](../../bin/files/src/entities/file/ui/icon.rs) · [Properties view](../../bin/files/src/pages/workspace/inspection.rs)

## Data sources and language coverage

The installed `shared-mime-info` 2.4 package on the development machine contains 908 MIME types with default English descriptions. Of these, 735 (81%) have Simplified Chinese descriptions tagged `zh-Hans-CN`. These counts come from the generated per-type XML files for the package and vary with the installed database version. [Per-type XML format](https://specifications.freedesktop.org/shared-mime-info/latest-single/#id-1.3.6)

| Filename | Canonical MIME | Simplified Chinese description in 2.4 |
| --- | --- | --- |
| `scene.blend` | `application/x-blender` | Blender 场景 |
| `installer.dmg` | `application/x-apple-diskimage` | Apple 磁盘映像 |
| `template.ots` | `application/vnd.oasis.opendocument.spreadsheet-template` | ODS 模板 |

All three MIME types above are absent from the GDS presentation table. The ODS template also has the `x-office-spreadsheet` generic icon hint, which drives a GDS document icon and category. [Upstream MIME definitions](https://cgit.freedesktop.org/xdg/shared-mime-info/tree/data/freedesktop.org.xml.in)

**Source-code coverage.** `shared-mime-info` does not define a dedicated `Code` category: only ~30 scripting/data types carry `text-x-script` (including `application/json`), while common source types such as `text/rust`, `text/x-csrc`, `text/x-go`, and `text/x-python` have no generic icon hint, and modern developer extensions such as `.zig`, `.nix`, `.jsx`, and `Dockerfile` are absent in 2.4. Generating a compact read-only table from [GitHub Linguist `languages.yml`](https://github.com/github-linguist/linguist/blob/main/lib/linguist/languages.yml) (`programming` and code `markup` vs `data`) provides maintainable `Code` classification and zero-I/O fallback for missing developer extensions without hand-maintaining a large local table.

`mime-db` v1.54.0 lists 2,522 MIME types, 1,015 of which have at least one extension. Its published JSON has no localized type descriptions. It can supplement remaining observed filename coverage gaps if needed. The IANA registry is a reference for registered MIME names. [mime-db schema](https://github.com/jshttp/mime-db#data-structure) · [Versioned data](https://github.com/jshttp/mime-db/blob/v1.54.0/db.json) · [IANA Media Types](https://www.iana.org/assignments/media-types/)

## Decision flow

The pipeline separates authoritative filesystem object classification (`lstat`), MIME resolution (keeping the vast majority of files off disk I/O), category/icon derivation, and localized human-readable naming.

### 1. Entry and MIME detection

```mermaid
flowchart TD
    Start["Directory entry (lstat metadata)"] --> Kind{"Entry kind?"}
    Kind -->|"Directory / Other / Unknown"| NonFile["Classify directly from lstat (no content I/O)"]
    Kind -->|"SymbolicLink"| Sym{"Resolved target is regular file?"}
    Sym -->|"No (dir / broken / unreachable)"| SymDone["Keep link classification (no content I/O)"]
    Sym -->|"Yes"| Glob
    Kind -->|"Regular file"| Glob["Zero-I/O filename lookup: mime.cache or globs2 + aliases dedup"]

    Glob --> Cand{"Distinct canonical MIME count?"}
    Cand -->|"1 (unique match)"| Shadow{"Legacy binary or XML MIME shadowed by Linguist text/code?"}
    Shadow -->|"No (normal unique match)"| FastUnique["Adopt MIME (no enrich)"]
    Shadow -->|"Yes (Pattern 2: e.g. .tsx, .mts, .mdx, go.mod)"| EnrichB
    Cand -->|"2+ (multiple matches)"| SubCheck{"One candidate is superclass of all others?"}
    SubCheck -->|"Yes (e.g. .json, .opus, .djvu)"| FastBase["Adopt common base MIME (no enrich)"]
    SubCheck -->|"No (Pattern 1: e.g. .ogg, .ts, .smi, .dot)"| EnrichB{"Content enrichment enabled?"}
    EnrichB -->|"No"| FallbackB["Deterministic candidate convergence (no disk I/O)"]
    EnrichB -->|"Yes"| ProbeB["Bounded 8 KiB header read (disambiguation module)"]
    ProbeB --> MagicB{"Unreadable or empty (0 B)?"}
    MagicB -->|"Yes"| FallbackB
    MagicB -->|"No"| MatchB{"Magic matches candidate/subclass or priority >= 80?"}
    MatchB -->|"Yes"| AdoptMagicB["Adopt matched MIME (downgrade disguised x-desktop)"]
    MatchB -->|"No"| HeurB["Structural filter: text vs binary AND application/xml subclass check"]
    HeurB --> HeurBRes{"Valid system candidates left?"}
    HeurBRes -->|"1"| AdoptHeurB["Adopt remaining MIME"]
    HeurBRes -->|"2+"| FallbackB
    HeurBRes -->|"0 (plain text failed XML/binary checks)"| LingFb{"Linguist text/code fallback?"}
    LingFb -->|"Yes (e.g. .ts -> TypeScript, .tsx -> TSX, go.mod -> Go Module)"| AdoptLingFb["Adopt Linguist language"]
    LingFb -->|"No"| FallbackB

    Cand -->|"0 (no filename match)"| LingC{"Matches Linguist code extension/filename?"}
    LingC -->|"Yes (e.g. .zig, .nix, Dockerfile)"| FastCode["Adopt code language match (no enrich)"]
    LingC -->|"No"| EnrichC{"Content enrichment enabled?"}
    EnrichC -->|"No"| UnknownNone["No MIME: None (no disk I/O)"]
    EnrichC -->|"Yes"| ProbeC["Bounded 8 KiB header read (enrich)"]
    ProbeC --> ReadC{"Read result?"}
    ReadC -->|"Unreadable"| UnknownNone
    ReadC -->|"Empty (0 B)"| EmptyText["text/plain"]
    ReadC -->|"Has bytes"| MagicC{"Magic signature matched?"}
    MagicC -->|"Yes"| AdoptMagicC["Adopt sniffed MIME (downgrade disguised x-desktop)"]
    MagicC -->|"No"| HeurC{"looks_like_text?"}
    HeurC -->|"Yes"| HeurText["text/plain (heuristic fallback)"]
    HeurC -->|"No"| HeurBin["application/octet-stream (heuristic fallback)"]
```

### 2. Category and icon classification

```mermaid
flowchart TD
    In["Detection output"] --> HasType{"Has MIME or Linguist code match?"}
    HasType -->|"None"| CatNone["Category: None (Type: File, Icon: File)"]
    HasType -->|"Linguist code match only"| CatCode0["Category: Code (Icon: FileCode)"]
    HasType -->|"Canonical MIME"| GI{"Own explicit generic-icon in shared-mime-info?"}

    GI -->|"x-office-document / spreadsheet / presentation"| CatDoc["Category: Document"]
    GI -->|"package-x-generic / media-optical"| CatArch["Category: Archive"]
    GI -->|"application-x-executable / bundle"| CatExec["Category: Executable"]
    GI -->|"text-html / text-x-script (non-data)"| CatCode1["Category: Code"]
    GI -->|"text-x-generic / template, or Linguist data (e.g. JSON, XML)"| CatText0["Category: Text"]
    GI -->|"image / audio / video / font-x-generic"| CatMedia0["Matching media or font category"]
    GI -->|"None"| Prefix{"MIME top-level prefix?"}

    Prefix -->|"image / audio / video / font"| CatMedia1["Matching media or font category"]
    Prefix -->|"text/*"| LingRefine{"Linguist programming or code markup?"}
    LingRefine -->|"Yes (e.g. Rust, C, Python, Go)"| CatCode2["Category: Code"]
    LingRefine -->|"No (prose / data)"| CatText1["Category: Text"]
    Prefix -->|"Other (e.g. application/*)"| Sub{"subclasses inheritance?"}

    Sub -->|"Parent has mapped generic-icon"| CatParent["Adopt parent mapped category"]
    Sub -->|"Subclass of text/plain"| CatText2["Category: Text"]
    Sub -->|"None"| Patch{"Minimal last-resort patch rule?"}
    Patch -->|"Yes"| CatPatch["Adopt patched category"]
    Patch -->|"No"| CatBin["Category: Binary"]
```

### 3. Human-readable name resolution

```mermaid
flowchart TD
    NameIn["Detection output + extension + category + GDS UI language"] --> Src{"What was detected?"}
    Src -->|"None (no MIME)"| ExtNone{"Has filename extension?"}
    ExtNone -->|"Yes (e.g. foo.xyz)"| LabelExtFile["Localized extension template (e.g. XYZ 文件 / XYZ file)"]
    ExtNone -->|"No"| LabelDash["Unknown placeholder (—)"]

    Src -->|"Linguist code match only (e.g. .zig)"| LabelCode["Localized source code template (e.g. Zig 源代码 / Zig source code)"]

    Src -->|"Canonical MIME"| HeurExt{"Heuristic text/plain or octet-stream with unknown extension?"}
    HeurExt -->|"Yes (e.g. notes.foo, data.bar)"| LabelHeurExt["Localized extension + category (e.g. FOO 纯文本 / BAR 二进制)"]
    HeurExt -->|"No"| XmlLoc["1. shared-mime-info XML comment for GDS UI language (async + cached)"]

    XmlLoc --> Hit1{"Matched BCP 47 locale (language + script)?"}
    Hit1 -->|"Yes"| Done1["Use localized comment (primary source of truth)"]
    Hit1 -->|"No"| Fluent["2. GDS Fluent missing-translation fallback (e.g. epub, text/plain)"]
    Fluent --> Hit2{"Matched?"}
    Hit2 -->|"Yes"| Done2["Use Fluent fallback"]
    Hit2 -->|"No"| XmlEn["3. shared-mime-info XML default English comment"]
    XmlEn --> Hit3{"Matched?"}
    Hit3 -->|"Yes"| Done3["Use default English comment"]
    Hit3 -->|"No"| CatFb{"4. Has filename extension?"}
    CatFb -->|"Yes"| Done4a["Localized extension + category (e.g. FOO 文档 / FOO document)"]
    CatFb -->|"No"| Done4b["Localized category name (e.g. 文档 / 纯文本 / 二进制)"]
```

## Proposed rollout

1. **Localized descriptions.** Make `shared-mime-info` per-type XML `<comment>` entries the primary source of truth for display names, retiring the untranslated short-name overrides in `mime-types.toml`. Look up the canonical MIME in XDG data locations (`$XDG_DATA_HOME/mime`, then `$XDG_DATA_DIRS/*/mime`) off the UI thread and cache parsed `<comment>` entries in memory. Select the description strictly by the GDS interface language (ignoring system `LANG`/`LC_*`) using `unic-langid` (`likelysubtags`) generic BCP 47 `(language, script, region)` matching with CLDR likely-script inference—requiring matching language and compatible script (so e.g. `zh-CN` matches `zh-Hans-CN` and rejects `zh-Hant-TW`, and `sr-Latn` rejects `sr-Cyrl`, without per-language special cases) and ranking exact region, neutral region (or untagged neutral English for `en`), then other regions. Fall back to a small Fluent table for common missing translations (such as `application/epub+zip`, `text/plain`, and `application/octet-stream`), then the default English `<comment>`, then a localized category label (including the uppercase extension when available). Heuristic generic fallbacks (`text/plain` / `application/octet-stream`) and unprobed unknown files retain their uppercase extension in the label when present (e.g., `FOO 纯文本`, `XYZ 文件`). Unify the Detailed Type column, `DetailedType` sorting, and the Properties Format row on this lookup, and expose the raw canonical MIME on a dedicated row in Properties. [XDG locations used by `xdg-mime`](https://docs.rs/xdg-mime/0.4.0/xdg_mime/#loading-the-shared-mime-database) · [GDS language settings](../../src/i18n.rs)
2. **Category and icon coverage.** Derive `ContentCategory` primarily from `shared-mime-info` and generated GitHub Linguist data rather than a large manual table: check own explicit `generic-icons` first, then MIME top-level prefixes (before `subclasses`, preventing `text/x-python` from inheriting `application-x-executable`), refine `Text` results matching Linguist `programming` or code `markup` into `Code` (keeping `data` formats such as JSON/YAML/TOML/XML in `Text`), then check `subclasses` inheritance, and demote manual rules to a minimal last-resort patch before `Binary`. Apply the category consistently to the Type column, icons, and Type sorting, and keep non-text `Document` formats (such as `.ots`, `.pdf`, `.docx`) out of `Kind::Text` preview selection. [Generic icons](https://specifications.freedesktop.org/shared-mime-info/latest-single/#id-1.3.10) · [Type sorting](../../bin/files/src/entities/file/model/sorting.rs) · [Preview selection](../../bin/files/src/entities/file/api/directory/inspection.rs)
3. **Unified disambiguation and toggleable content enrichment.** Keep the vast majority of files on the zero-I/O fast path: unique non-shadowed MIME matches, parent-plus-subclass-only multi-matches (such as `.json`, `.opus`, `.oga`, and `.djvu`, collapsed directly to their common base MIME), genuine XML code formats (`.xhtml`, `.xslt`), and Linguist zero-I/O code-extension matches all skip disk reads. Consolidate conflict detection and bounded 8 KiB resolution into a dedicated [`disambiguation`](../../bin/files/src/entities/file/api/directory/content/disambiguation.rs) module covering two general patterns: (1) multi-MIME filename conflicts (~37 globs such as `.ogg`, `.ts`, `.smi`, `.otf`, `.dot`, `.m`) and (2) single legacy binary or XML `shared-mime-info` matches shadowed by a non-XML Linguist text/code format (such as `.tsx` vs Tiled XML, `.mts` vs `video/mp2t`, `.mdx` vs Sega 32X ROM, `go.mod` vs `audio/x-mod`, `.sml` vs SMIL XML). During 8 KiB probing, apply magic matching followed by structural validation (text vs binary and `application/xml` subclass header checks); if all system candidates fail structural validation on plain-text content, fall back to the matching Linguist language. Make background content enrichment a user-toggleable setting for lower-end machines: when disabled, conflicts converge deterministically without disk I/O and unmatched entries stay `None` (rendered via extension/unknown fallbacks). [Recommended checking order](https://specifications.freedesktop.org/shared-mime-info/latest-single/#id-1.3.15) · [Local entry enrichment](../../bin/files/src/entities/file/api/directory/enrich.rs)
4. **Binary `mime.cache` alignment.** Align the MIME index backend with mainstream Linux file managers (GNOME/GIO and KDE/Qt): prefer reading the precompiled binary `mime.cache` (via `mmap` with indexed literal, reverse-suffix, and glob lookups) and fall back to the plain-text index files (`globs2`, `aliases`, `subclasses`, `generic-icons`, `magic`) only when `mime.cache` is absent. Currently `xdg-mime` 0.4 parses only the plain-text indexes and scans all glob rules linearly in an unordered `HashSet` per filename lookup. Because `mime.cache` does not contain `<comment>` strings, localized descriptions still come from on-demand per-type XML reads.

## Acceptance checks

- With GDS set to Simplified Chinese, `.blend`, `.dmg`, and `.ots` show their localized descriptions in Detailed Type and Properties even when the system locale is English, and Properties also displays the raw canonical MIME.
- A MIME with no Simplified Chinese comment, such as `application/epub+zip` in the sampled 2.4 package, skips Traditional Chinese (`zh-Hant-TW`) and follows the fallback order. A missing database description or heuristic fallback on an unknown extension (e.g., `notes.foo`, `data.bar`) produces a readable label retaining the extension.
- An `.ots` file uses the document category and icon without triggering plain-text preview. A `.tar.gz` file retains its compound-extension match and uses the archive category and icon. Source files such as `.rs`, `.py`, `.zig`, and `.nix` classify as Code with the code icon, while `.json` stays Text.
- Unique non-shadowed filename matches, genuine XML code formats (`.xhtml`, `.xslt`), and parent+subclass-only globs (such as `.json`, `.opus`) avoid opening files for detection. Audio and video `.ogg` fixtures, TypeScript vs Qt Linguist vs MPEG-2 `.ts` fixtures, and shadowed formats (`.tsx`, `.mts`, `.mdx`, `go.mod`) are distinguished structurally when enrichment is enabled; disabling content enrichment skips header I/O while keeping deterministic, readable labels.
