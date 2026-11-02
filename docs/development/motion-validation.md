# Interface motion validation

## Scope

This change adds an application-wide animation preference to Files and Agents, plus shared 140 ms disclosure-arrow transitions. Existing 160 ms sidebar motion and Kit dialogs, dropdowns, notifications and spinners are reused. Content mount/unmount, glass surfaces, streaming text, virtual lists and terminal grids are unchanged. See [motion guidelines](../vistual-language/motion.md).

The policy combines the independently retained system preference with the application switch. Every production window uses Base Root; a Root plugin reasserts the effective flag before content/overlays render, covering Kit 0.8.1's late system-write ownership collision. The plugin does no platform I/O and only changes the flag on a mismatch.

Linux uses the existing Tokio runtime and Settings Portal `ReadAll`/`SettingChanged`, without polling. Missing portal support retains the last system reading (initially no reduction); explicit disable still wins. If the listener fails, it retains that reading rather than reconnecting. macOS/Windows use Kit's synchronous startup reading and re-read when re-enabled; live change notification is not implemented there.

## Reproduction

From the repository root:

```sh
nix develop --impure --expr 'import ./shell.nix' --command cargo fetch --locked
nix develop --impure --expr 'import ./shell.nix' --command cargo fmt --all --check
nix develop --impure --expr 'import ./shell.nix' --command cargo test --locked --workspace --features gui-tests -- --skip entities::backend::ui::presentation::tests::menu_rows_keep_full_labels_inside_fixed_width
```

The only explicit exclusion is the pre-existing fixed-width backend-menu label test, reproduced before this work on `76e6edd` and `f208588`. Its implementation/assertions remain unchanged. This is **not an unqualified all-green test claim**. Existing manual benchmarks and the existing translation doctest remain ignored by their original annotations.

## Combined result

Final combined source `d52a6fc44816b9ad606ad5f3bf7f132665e40ca2` (settings, policy and disclosures) passed the formatting check and normal parallel workspace command above:

| Target | Passed | Existing ignored | Explicit exclusions |
| --- | ---: | ---: | ---: |
| Shared library | 172 | 0 | 0 |
| Agents | 419 | 2 | 1 baseline menu test |
| Files | 228 | 0 | 0 |
| Settings derive macro | 2 | 0 | 0 |
| Doctests | 0 | 1 | 0 |

The combined source was accepted by Host verification at `be3bad93cca2f3593ee899414c169d6c3cd224eb`. Existing deprecation/unused-code warnings were not treated as newly fixed or expanded in scope.

## Behavior evidence

- Shared policy tests exercise system reduction plus off/on/off/on preference changes, two existing windows and a new window, a late competing Base write before child rendering, stopped native Spinner frame requests and resumed motion after enabling.
- The Linux protocol fixture uses in-process D-Bus peers, exercising the real initial snapshot and false/true change signals without desktop mutation, a daemon subprocess or sleeping.
- Disclosure tests use the deterministic GPUI clock to cover first render, continuous reversal, fixed icon/label slots, settling and zero continuing frame requests. Existing sidebar boundary, scrolling and focus assertions remain in place.
- The real GDS notification overlay/Kit queue test checks both timeout and manual dismissal while decorative animation is disabled.
- Both application settings interaction tests navigate/search to the control, toggle it, verify native effective state, and restore only this field. Agents' UI fixture also saves/reloads its configuration; Files' separate configuration tests cover persistence roundtrips and legacy defaults. Independent reset checks preserve unrelated preferences. Four existing settled-layout fixtures now express their pre-existing no-motion request through the shared setter instead of bypassing it; no new blanket animation suppression was added.

## Diagnostic history and limits

The initial shared-policy candidate passed a full local parallel run (169 library, 418 Agents and 226 Files tests), but its first integration attempt hit a transient `Busy` error in `persistence::tests::writable_configuration_below_a_readonly_ancestor_can_be_created_and_replaced`. The same second-write failure was reproduced on an untouched `32b209e52ee71ebb792dd5d8cebbc4c507ad7704` checkout: three parallel library runs passed all 166 tests, the fourth failed only this test. Neither the persistence implementation nor its test was changed or excluded. A separate parallel candidate run reported a transient database-lock failure in the existing concurrent-storage test; that test also remains unchanged and included.

The portal fixture was changed from a private daemon to in-process D-Bus peers so this feature adds no test subprocess that could inherit unrelated file descriptors. The persistence failure also occurred without that subprocess; this fixture refinement is not claimed to fix the baseline race.

Policy plus disclosure commit `b63e1bcf4ee160cf76f413294b89ff2683d95551` passed the complete workspace suite with `--test-threads=1` and the same single menu exclusion: 172 library, 418 Agents, 226 Files and 2 settings-macro tests passed; 2 existing Agents tests and 1 doctest were ignored. Host subsequently accepted the normal parallel verification at `8b89aaac24a53deb5a5ab3d1fdcdcbfe2d649d60`. The standalone `appearance` capability also passed `cargo check --locked --no-default-features --features appearance --lib`.

These are automated Linux checks, not a live desktop accessibility-toggle trial, a Wayland/X11 visual review, macOS/Windows execution, or an FPS measurement. No build was installed or pushed, and the result remains on the isolated review branch.
