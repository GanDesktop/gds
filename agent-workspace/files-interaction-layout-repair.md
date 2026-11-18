# Files interaction and layout repair

The reference is the GPUI Files application at `5fc77a8`. Compare its layout, location resolution, input routing and paint paths with the AntiWebUI branch. Screenshot 4 shows the current migration on the left and the reference on the right. Work on the Wayland Files target.

| Workstream | Report | Acceptance |
| --- | --- | --- |
| Terminal | 2 | Match the original panel and grid insets and palette. Type into a real PTY, exercise sustained output, compare redraw cost and confirm the final output is visible. |
| Browser interaction | 1, 3, 6, 7 | Marquee tracks the visible browser and selects by the same geometry as drawing. Both sidebar groups use the same header. Mounted volumes open with a meaningful title and location in the tab and have a unique active state. Pointer editing, ancestor navigation, Enter and Escape work in the path field. |
| Artwork and layout | 4, 5, 8 | Landscape and portrait assets preserve their proportions in the grid and inspector. Use a shared SVG asset library and the original file/folder icon resolution. Handle long names and metadata at constrained widths using shared layout rules. |
| Dialog actions | Additional new-folder screenshot | Compare the reference footer button anatomy, icon, inline shortcut hints, primary/secondary styling, input validation and focus. Verify the Chinese new-folder dialog and reuse the component for rename. |

Implementation belongs to three parallel coding agents. Review against the original source and the screenshots, coordinate any shared module or toolkit changes, then exercise the integrated native application. Use a disposable directory for interaction checks. Record relevant measurements, remaining limitations and preview information for the user.

## Native observations

The reference and migrated binaries ran at the same window size with a Chinese locale in private Wayland sessions. Two isolated UDisks entries pointed at different fixture directories.

* Clicking Amber, then Birch, then Back navigated to three observable directory states with the corresponding tab title and single highlighted drive. The context menu opened Birch in its own tab and the pinned volume reopened after process restart. A taller window displayed all seven fixture volumes. [Tab and volume restoration](../target/files-e2e/repair-browser-menu-4gdrzkqd/07-restored-volume.png) · [Seven-volume sidebar](../target/files-e2e/repair-browser-tall-7kwfiyww/01-browse.png)
* The native clipboard contained exactly the marquee's selected files after release. Ctrl merged the selection, Escape during a grab restored the previous selection, and scrolling preserved the marquee's clipping and alignment. Clicking the path field, entering the Amber mount path and pressing Enter switched to that volume. [Scrolled marquee](../target/files-e2e/repair-browser-interactions-sp27zhei/05-scrolled-marquee.png) · [Pointer-edited address](../target/files-e2e/repair-geometry-q1be7wti/path-committed.png)
* Both the keyboard shortcut and a pointer click on the revealed A suffix created the expected folder on disk. Invalid input kept the dialog open. [Chinese dialog with Alt held](../target/files-e2e/repair-geometry-q1be7wti/badge-pointer.png) · [Activation results](../target/files-e2e/repair-geometry-q1be7wti/dialog.txt)
* The 80 × 240 portrait and 300 × 100 landscape fixtures occupied 33 × 100 and 145 × 48 pixels in the grid, then 55 × 166 and 245 × 82 in the inspector. The original migration rendered both grid images at 100 × 100. The grid wraps long filenames, and the inspector preserves the full value in its tooltip. [Measured image bounds](../target/files-e2e/repair-integrated-g3fw6km_/aspect.json) · [Folder glyphs](../target/files-e2e/repair-icon-default-ej2kmk8m)
* In a 1280 × 900 scale-1 software-rendered Wayland session, the real PTY produced 240 colored lines in four seconds. The migration rendered 19 frames before the change and 159 afterwards. Median UI frame construction fell from 170.96 ms to 1.25 ms. In a separate Sway session a full 13-character input line reached the PTY, followed by a Chinese commit delivered through the Wayland input-method protocol. [Frame statistics](../target/files-e2e/repair-reviewed-zRLTaIJx/terminal/results.json) · [Chinese text received by the PTY](../target/files-e2e/repair-ime-x5wc1std/terminal-ime.txt)

The native drawing uses the font's available bold and italic faces. Fonts without the corresponding face may render those cells in the regular face. The terminal measurements apply to the stated fixture and display scale.

The inspector and hidden-file controls use the primary active treatment. Grid mode keeps sorting available and disables table-column configuration for both the toolbar and its shortcut. The underlying toolkit revision is [GanDesktop/AntiWebUI 1e55ade](https://github.com/GanDesktop/AntiWebUI/commit/1e55adecf93bf14d69c0150712de628dced1147b). [Three-way native screenshot comparison](../target/files-e2e/repair-reviewed-zRLTaIJx/comparison.png) · [Final stock Files end-to-end run](../target/files-e2e/run-y605r2uy/passed.txt)
