# Files sidebar expansion on GPUI-fast and zgui

Expanding the sidebar in the Files list view took a median **26.4 ms per frame on GPUI-fast and 51.1 ms on zgui** after matching the sidebar and table animations. The largest individually verified cost was a zgui debug assertion that recalculated coordinate transforms. It cost about **6.6 ms per expansion frame**.

## Workload and measurement

Both applications opened the same home directory with the same saved settings, in a 1244 × 1412 logical-pixel window on a 60 Hz nested Niri/Sway session. Both used Vulkan on the same GeForce RTX 3060. Every crate used Cargo's development profile with `opt-level = 0` and debug assertions enabled.

The baseline used [Files at 5fc77a86](https://github.com/GanDesktop/gds/tree/5fc77a86f9707ae7ecabde75767cb114f19d10b0) with [GPUI-fast at 8e6e750c](https://github.com/GanDesktop/gpui-fast/tree/8e6e750c16477337f29cb3dac271a6e8fc11665b). The migration build included the [Files workspace-width changes](https://github.com/GanDesktop/gds/commit/5ceb61f344631cf09ec87a2acf8cfb2f861d5016) and [zgui at 73114cd5](https://github.com/GanDesktop/zgui/tree/73114cd5a61bbaf353aae22af933239fe00a7138).

Each click advanced the sidebar from 58 to 222 logical pixels in eight equal steps. Two runs of four expansions produced 56 interior frames per implementation. The GPUI-fast timer covered drawing, presentation and arena cleanup. The zgui timer used `FrameReport.elapsed`. These are CPU-side wall times. All 56 zgui frames submitted commands, with swapchain presentation deferred by the presentation gate. GPU execution and display scanout require separate measurements.

| Expansion workload | GPUI-fast median | zgui median |
| --- | ---: | ---: |
| Each application’s existing sidebar and table implementation | 26.4 ms | 66.0 ms |
| Matched sidebar styling and name-column resizing | 26.4 ms | 51.1 ms |
| Sidebar contents and name-column width held fixed while pane boundaries moved | 27.6 ms | 37.9 ms |

The matched zgui build retained the moving icon inset and fading label, removed duplicate volume-row insets and used a layout rule for the name column in place of per-row width writes. At each sidebar width, the name header and first data cell agreed. Their widths were within 1.4 logical pixels of the GPUI-fast column. The other three columns stayed at 160, 110 and 100 pixels. This change saved 14.9 ms per frame in the test window. Its width rule was calibrated for that window and used only for the comparison.

## Where the remaining 24.8 ms went

For each engine, sort the 56 frames by total duration, take the middle 28, and average every phase over those same frames. The phases then add up to the mean frame time before rounding. GPUI-fast's geometry entry is prepaint with its nested layout solver time subtracted. The columns pair the work each engine does at that point in the frame. [Per-frame samples in milliseconds](sidebar-frame-samples.csv)

| Work per frame | GPUI-fast (ms) | zgui (ms) | Difference (ms) |
| --- | ---: | ---: | ---: |
| Build and request layout / apply reactive and style updates | 7.81 | 9.88 | +2.07 |
| Solve layout | 3.36 | 8.88 | +5.53 |
| Update geometry, clips and hit information | 7.33 | 14.52 | +7.19 |
| Build the scene and draw list | 4.62 | 10.71 | +6.09 |
| CPU rendering backend | 2.04 | 6.19 | +4.15 |
| Other work within the frame | 1.15 | 0.91 | −0.24 |
| **Total** | **26.32** | **51.10** | **+24.78** |

### Clip geometry and hit testing

When a fragment changes, zgui walks its clip chain to find the bounds on screen. `Pass::resolve` caches the last two spatial transforms. A debug assertion walks the transform tree again and compares the matrices bit for bit, including on a cache hit. An expansion frame made a median of 7,220 checks. Function timers attributed 6.37 ms to the assertion, and an independent CPU profile found 46% of geometry-stage samples inside clip-boundary resolution. [Transform lookup and assertion](https://github.com/GanDesktop/zgui/blob/73114cd5a61bbaf353aae22af933239fe00a7138/crates/zgui-layout/src/fragment/diff/mod.rs#L727)

Skipping just that assertion reduced the fragment stage from 13.9–14.1 ms to 7.39 ms while hit updates continued. A second experiment kept the assertion and reused clip bounds within a pass, keyed by clip ID and spatial-tree write version. This took 10.31 ms. A verification run recomputed every reused result and checked equality. The frame also updated about 781 hit entries. Direct timing put `Pass::touch_hit` at 0.58 ms and `HitIndex::settle` at 0.14 ms. Other paths also maintain hit entries.

### Draw-list construction

Changing pane width also changes the bounds of row containers, drag targets, menu hosts and name cells. A median of 317 box fragments per frame failed the paint cache's size check. About 295 of them had emitted zero primitives on the previous frame. The file table accounted for about 125 size misses, including 124 previously empty records. Sidebar row wrappers accounted for another 32. [Paint-cache size check](https://github.com/GanDesktop/zgui/blob/73114cd5a61bbaf353aae22af933239fe00a7138/crates/zgui-paint/src/walk/replay.rs#L556)

The empty-record experiment allowed reuse only for an empty box record whose current flags included `PAINTS_NOTHING`, subject to the remaining cache checks. A separate verification run encoded the candidates normally and asserted that each still produced zero primitives. This reduced re-encodes from about 356 to 56, saving about 1.2 ms of draw-list construction. Across the original expansion samples, `Painter::emit` took a median of 10.92 ms. Additional timers measured roughly 746 fragment paint checks, 1.50 ms replaying about 396 chunks, 2.83 ms generating new primitives and 0.60 ms saving new records. These timers include nested work. [Paint traversal](https://github.com/GanDesktop/zgui/blob/73114cd5a61bbaf353aae22af933239fe00a7138/crates/zgui-paint/src/walk/mod.rs#L953)

### Layout and rendering backend

Of 312 main-thread CPU samples taken during zgui layout, 287 were in Cephal’s flex layout path. Work included computing flex base sizes, cross-axis sizes and final child layouts. The median frame had 512 uncached final-layout requests, 469 uncached size requests and zero text runs shaped. GPUI-fast reused 917 layout nodes, wrote 23 node styles and invoked 32 measurement callbacks. The engines count different events. Their solver times were 8.88 and 3.36 ms respectively.

GPUI-fast recorded eight draw calls per expansion frame, compared with 180 on zgui. The zgui backend spent a median 1.30 ms preparing GPU tables, 0.35 ms planning draws, 1.06 ms uploading buffers and 3.44 ms recording and submitting commands. A separate 42-frame GPUI-fast run measured 0.21 ms recording and 0.35 ms submitting. Its total rendering step took 0.88 ms, with a further 0.09 ms acquiring a surface texture and 1.48 ms presenting it. Draw counts describe commands recorded on the CPU.

## Controlled zgui runs

These runs used the same matched workload, with individual diagnostic switches in a separate binary. Values are medians over 28 interior expansion frames per run. The version pinned by GDS contains full-scene repaint on surface recovery and the identity clip-transform fast path. The matrix-check, clip-cache and empty-record switches below were experiments.

| Change from matched zgui | Frame (ms) | Fragments and hits (ms) | Draw list (ms) | Backend (ms) | Re-encoded chunks |
| --- | ---: | ---: | ---: | ---: | ---: |
| Baseline | 51.06 | 14.11 | 11.57 | 6.25 | 356 |
| Skip the matrix cross-check | 43.87 | 7.39 | 11.63 | 6.28 | 357 |
| Reuse clip bounds within a pass | 47.90 | 10.31 | 11.68 | 6.39 | 357 |
| Re-encode empty candidates and verify the result | 50.76 | 14.23 | 11.90 | 6.17 | 356.5 |
| Reuse eligible empty records | 49.10 | 13.96 | 10.32 | 5.83 | 56 |
| Reuse both clip bounds and eligible empty records | 44.18 | 9.63 | 10.65 | 5.94 | 56 |
| Repeat baseline | 49.81 | 13.91 | 11.47 | 6.02 | 356.5 |

All seven runs emitted a median of 733 primitives and recorded 180 draws per expansion frame.
