# Motion

Motion should explain an interaction, not compete with content. Prefer Kit's existing behavior; do not wrap an already animated control in another entrance effect.

## Preference and scope

Files and Agents expose **Interface animations** in their appearance settings. The default is enabled, which follows the system's reduced-motion preference; disabled forces decorative UI motion off. This is a process-wide preference covering existing and newly opened application windows, not a shared suite configuration. Each application persists its own field, and restoring this field's default must not reset other appearance or session preferences.

The shared appearance layer owns the effective reduced-motion value: `!animations_enabled || system_reduced_motion`. Components consume Kit/GPUI's value, never query desktop settings themselves. Linux follows Settings Portal reads and change signals through the application's existing Tokio runtime. Without a usable portal it retains the last known system preference (initially no reduction); explicit disable still forces motion off. macOS and Windows use Kit's startup system snapshot and re-read it when animations are re-enabled; live system-change notifications are not provided there. No platform is polled, and no component reads system settings per frame. Disabling motion settles in-flight transitions and stops decorative spinner frames. Status artwork and actual progress remain visible. It does not pause notification expiry, networking, file work, keyboard input, focus, terminal output or media playback.

## Added, retained and deliberately unchanged

- **Added:** shared `DisclosureIcon` artwork for Files' volumes/locations headings and Agents' workspace groups. A right-facing chevron rotates a quarter-turn toward expanded state over **140 ms, ease-out cubic**, using Kit's target transition. The host retains the accessible name, expanded semantics, tooltip and existing click target. The icon and label slots do not change size or alignment.
- **Retained:** the existing **160 ms, ease-out cubic** sidebar width transition, sampled once by the host and shared across the whole column. See [sidebar presentation](sidebar.md#continuous-presentation).
- **Reused without another wrapper:** Kit dialogs, notifications, dropdowns, loading spinners and their reduced-motion behavior. Notification lifetime remains independent of decorative animation.
- **Files workspace:** details width, terminal dock height, and volumes/locations content height use **160 ms, ease-out cubic** target transitions. Exiting content remains clipped until the reveal ends. Split resizing retains the expanded preferred height; the divider is interactive only when fully expanded. Terminal maximize and restore use the same transition, shrinking the workspace and divider continuously while preserving the preferred split height. Terminal sessions survive hiding the dock.
- **Files selection:** tab, file-row and grid-tile highlights and foreground colors crossfade over **160 ms, ease-out cubic**. Selection and keyboard actions take effect immediately. Tab content gently enters from 88% opacity after switching, while the initial window contents appear immediately.
- **Files grid:** visible tiles transition their bounds using stable file-path identities while the virtual list retains its target row geometry. Newly visible tiles adopt their initial bounds immediately.
- **Deliberately immediate:** chat/tool disclosures and settings groups retain their established visibility and focus rules.
- **Not animated:** streaming text, table rows, terminal cells, editor/input text, glass opacity or surface backgrounds. No per-file timers, repeated text entrance, whole-panel scale or duplicated translucent layers.

## Component anatomy

```text
Existing disclosure control (host: label, tooltip, expanded state, click/focus)
├─ DisclosureIcon (shared: fixed-size decorative chevron)
└─ Existing label
Existing content (host: visibility, reveal progress and focus rules)
```

Create `DisclosureIcon::default()`, supply a stable section identity with `id`, and set `expanded` from the actual displayed state. Size is optional. Keep the component inside the existing control; it is not a second button. IDs must identify the section, not a position that changes when filtering a list.

Kit adopts the first target immediately, retargets from the currently displayed value, and stops requesting frames when settled. Reduced motion snaps to the target, including halfway through a transition. A queued frame may drain after the preference changes; it must not schedule a continuing decorative loop. All arrows share one component policy; do not tune their duration independently in applications.

Deterministic GPUI tests cover reversal, fixed icon/label slots, in-flight reduced motion and stopped frame requests (including a native Kit spinner). A real GDS notification overlay/Kit queue test also verifies that auto-hide and manual dismissal still complete while decorative frames are stopped. These checks are not an on-device visual review or an FPS measurement.
