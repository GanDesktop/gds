//! Shared Stylo CSS rules for GDS UI controls and surfaces.
//!
//! Theme palette, typography, and motion custom properties (`--gds-*` and
//! `--zui-*`) are installed per window by
//! [`crate::app::appearance::Appearance::install_window_sheet`]. This module
//! installs shared structural component classes, neutral hover rules, and
//! GDS's `1px` thin focus-contour contract once per window document.

use std::sync::OnceLock;

use zgui::view::install_stylesheet;
use zgui_ui::{
    ButtonStyle, InputGroupStyle, InputStyle, NativeSelectStyle, SelectStyle, SliderStyle,
    SwitchStyle, TextareaStyle,
};

const BASE_SHEET: &str = "gds-ui-base";

const STATIC_BASE_CSS: &str = r#"
.gds-icon {
    display: inline-block;
    flex-shrink: 0;
    width: var(--gds-icon-size, 16px);
    height: var(--gds-icon-size, 16px);
    color: inherit;
}

.gds-surface {
    background: var(--gds-surface);
    color: var(--gds-surface-foreground);
    border: 1px solid var(--gds-border);
    border-radius: var(--gds-radius-control, 6px);
}

.gds-card-surface {
    background: var(--gds-panel);
    color: var(--gds-panel-foreground);
    border: 1px solid var(--gds-border);
    border-radius: var(--gds-radius-control, 6px);
    padding: var(--gds-content-padding, 12px);
}

.gds-glass-panel {
    position: relative;
    border-radius: 20px;
    border: 1px solid var(--gds-border);
    color: var(--gds-panel-foreground);
    background-color: var(
        --gds-glass-bg,
        color-mix(in srgb, var(--gds-panel) 70%, transparent)
    );
    box-shadow:
        0 12px 36px -10px color-mix(in srgb, var(--gds-shadow) 28%, transparent),
        0 2px 8px -2px color-mix(in srgb, var(--gds-shadow) 12%, transparent),
        inset 0 1px 0 0 color-mix(in srgb, var(--gds-panel-foreground) 12%, transparent);
}

.gds-content-container {
    display: flex;
    flex-direction: column;
    flex: 1 1 0px;
    min-width: 0;
    min-height: 0;
    padding: var(--gds-content-padding, 12px);
}

.gds-frame {
    display: flex;
    flex-direction: column;
    width: 100%;
    height: 100%;
    min-width: 0;
    min-height: 0;
    border-radius: var(--gds-radius-window, 0px);
    border: 1px solid var(--gds-border);
    overflow: hidden;
    color: var(--gds-foreground);
    font-family: var(--gds-font-sans);
    font-size: var(--gds-font-size-small, 14px);
}

.gds-frame-content {
    display: flex;
    flex-direction: column;
    flex: 1 1 0px;
    min-width: 0;
    min-height: 0;
    background: transparent;
}

.gds-toolbar {
    display: flex;
    flex-direction: row;
    align-items: center;
    height: var(--gds-toolbar-height, 52px);
    flex-shrink: 0;
}

.gds-footer {
    display: flex;
    flex-direction: row;
    align-items: center;
    height: var(--gds-footer-height, 36px);
    flex-shrink: 0;
}

.gds-sidebar {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    color: var(--gds-sidebar-foreground);
}

.gds-nav-item {
    display: flex;
    flex-direction: row;
    align-items: center;
    flex-shrink: 0;
    gap: var(--gds-nav-item-gap, 8px);
    min-height: var(--gds-sidebar-item-height, 38px);
    padding: 0 var(--gds-nav-item-pad-right, 8px) 0 var(--gds-nav-item-pad-left, 8px);
    border: none;
    border-radius: var(--gds-radius-control, 6px);
    color: inherit;
    cursor: pointer;
}

.gds-nav-item:hover {
    background-color: var(--gds-sidebar-hover);
}

.gds-nav-item:active {
    background-color: var(--gds-pressed);
}

.gds-nav-item[data-selected="true"] {
    background-color: var(--gds-selection);
    color: var(--gds-selection-foreground);
}

.gds-nav-item[data-drop-target="true"] {
    background-color: var(--gds-drag-target);
    outline: 1px solid var(--gds-primary);
    outline-offset: -1px;
}

.gds-nav-item:focus:not(:focus-visible) {
    outline: none;
}

.gds-nav-item:focus-visible {
    outline: 1px solid var(--gds-primary);
    outline-offset: -1px;
}

.gds-hover-slot {
    position: relative;
    display: flex;
    align-items: center;
    justify-content: center;
    width: var(--gds-icon-button-size, 32px);
    height: var(--gds-icon-button-size, 32px);
    flex-shrink: 0;
}

.gds-hover-slot__action {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    opacity: 0;
    pointer-events: none;
}

.gds-hover-slot[data-has-action="true"]:hover .gds-hover-slot__action,
.gds-hover-slot[data-has-action="true"][data-revealed="true"] .gds-hover-slot__action,
.gds-nav-item:not([data-collapsed="true"]):hover .gds-hover-slot[data-has-action="true"] .gds-hover-slot__action,
.gds-nav-item:not([data-collapsed="true"]):focus-within .gds-hover-slot[data-has-action="true"] .gds-hover-slot__action {
    opacity: 1;
    pointer-events: auto;
}

.gds-hover-slot[data-has-action="true"]:hover .gds-hover-slot__status,
.gds-hover-slot[data-has-action="true"][data-revealed="true"] .gds-hover-slot__status,
.gds-nav-item:not([data-collapsed="true"]):hover .gds-hover-slot[data-has-action="true"] .gds-hover-slot__status,
.gds-nav-item:not([data-collapsed="true"]):focus-within .gds-hover-slot[data-has-action="true"] .gds-hover-slot__status {
    opacity: 0;
}

.gds-sidebar[data-collapsed="true"] .gds-nav-item .gds-hover-slot,
.gds-sidebar[data-collapsed="true"] .gds-nav-item .gds-hover-slot .gds-hover-slot__action,
.gds-nav-item[data-collapsed="true"] .gds-hover-slot,
.gds-nav-item[data-collapsed="true"] .gds-hover-slot .gds-hover-slot__action {
    opacity: 0;
    pointer-events: none;
}

.gds-corner-action {
    position: absolute;
    top: 1px;
    right: 1px;
    opacity: 0;
    pointer-events: none;
    visibility: hidden;
}

.gds-sidebar[data-collapsed="true"] .gds-corner-action,
.gds-nav-item[data-collapsed="true"] .gds-corner-action {
    visibility: visible;
}

.gds-sidebar[data-collapsed="true"] .gds-corner-action[data-revealed="true"],
.gds-sidebar[data-collapsed="true"] .gds-nav-item:hover .gds-corner-action,
.gds-sidebar[data-collapsed="true"] .gds-nav-item:focus-within .gds-corner-action,
.gds-nav-item[data-collapsed="true"] .gds-corner-action[data-revealed="true"],
.gds-nav-item[data-collapsed="true"]:hover .gds-corner-action,
.gds-nav-item[data-collapsed="true"]:focus-within .gds-corner-action {
    opacity: 1;
    pointer-events: auto;
    visibility: visible;
}

.gds-sidebar[data-expanded="true"] .gds-corner-action,
.gds-nav-item[data-expanded="true"] .gds-corner-action {
    opacity: 0;
    pointer-events: none;
    visibility: hidden;
}

.gds-pinned-icon {
    position: relative;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: var(--gds-sidebar-icon-size, 18px);
    height: var(--gds-sidebar-icon-size, 18px);
    flex-shrink: 0;
}

.gds-pinned-icon__badge {
    position: absolute;
    left: -6px;
    top: -6px;
    width: var(--gds-small-icon-size, 12px);
    height: var(--gds-small-icon-size, 12px);
    display: flex;
    align-items: center;
    justify-content: center;
}

.gds-list-surface {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
}

.gds-list-surface:focus:not(:focus-visible),
.gds-region-surface:focus:not(:focus-visible),
.gds-focus-ring:focus:not(:focus-visible) {
    outline: none;
}

.gds-list-surface:focus-visible,
.gds-region-surface:focus-visible,
.gds-focus-ring:focus-visible,
.zui-scroll-area__viewport.zui-scroll-area__viewport:focus-visible,
.zui-dialog__dismiss.zui-dialog__dismiss:focus-visible {
    outline: 1px solid var(--gds-primary);
    outline-offset: -1px;
    box-shadow: none;
}

.zui-button .gds-icon {
    pointer-events: none;
    flex: none;
}

.gds-button:not(:disabled),
.gds-icon-button:not(:disabled) {
    cursor: pointer;
}

.gds-button[data-toggled="true"],
.gds-button[data-active="true"] {
    background-color: var(--gds-selection);
    color: var(--gds-selection-foreground);
    border-color: transparent;
}

.gds-button[data-loading="true"] {
    opacity: 0.85;
}

.gds-icon-button {
    padding: 0;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    position: relative;
}

.gds-icon-button:not([data-size="icon-xs"]):not([data-size="icon-sm"]):not([data-size="icon-lg"]) {
    width: var(--gds-icon-button-size, 32px);
    height: var(--gds-icon-button-size, 32px);
}

.gds-icon-button[data-toggled="true"],
.gds-icon-button[data-active="true"] {
    background-color: var(--gds-selection);
    color: var(--gds-selection-foreground);
    border-color: transparent;
}

.gds-icon-button__artwork {
    position: relative;
    display: flex;
    align-items: center;
    justify-content: center;
    width: 100%;
    height: 100%;
    pointer-events: none;
}

.gds-icon-button__primary {
    opacity: 1;
    transform: scale(1);
    transition:
        opacity var(--gds-motion-fast, 100ms) var(--gds-motion-ease-out, ease-out),
        transform var(--gds-motion-fast, 100ms) var(--gds-motion-ease-out, ease-out);
}

.gds-icon-button__hover {
    position: absolute;
    opacity: 0;
    transform: scale(0.92);
    transition:
        opacity var(--gds-motion-fast, 100ms) var(--gds-motion-ease-out, ease-out),
        transform var(--gds-motion-fast, 100ms) var(--gds-motion-ease-out, ease-out);
}

.gds-icon-button:hover .gds-icon-button__primary {
    opacity: 0;
    transform: scale(0.92);
}

.gds-icon-button:hover .gds-icon-button__hover {
    opacity: 1;
    transform: scale(1);
}

.gds-app-menu-button__artwork {
    position: relative;
    display: flex;
    align-items: center;
    justify-content: center;
    width: var(--gds-app-menu-icon-size, 24px);
    height: var(--gds-app-menu-icon-size, 24px);
    pointer-events: none;
}

.gds-app-menu-button__brand {
    display: flex;
    align-items: center;
    justify-content: center;
    width: var(--gds-app-menu-icon-size, 24px);
    height: var(--gds-app-menu-icon-size, 24px);
    color: var(--gds-foreground);
    opacity: 1;
    transform: scale(1);
    transition:
        opacity var(--gds-motion-fast, 100ms) var(--gds-motion-ease-out, ease-out),
        transform var(--gds-motion-fast, 100ms) var(--gds-motion-ease-out, ease-out);
}

.gds-app-menu-button__glyph {
    position: absolute;
    opacity: 0;
    transform: scale(0.85);
    color: var(--gds-foreground);
    transition:
        opacity var(--gds-motion-fast, 100ms) var(--gds-motion-ease-out, ease-out),
        transform var(--gds-motion-fast, 100ms) var(--gds-motion-ease-out, ease-out);
}

.gds-app-menu-button:hover .gds-app-menu-button__brand {
    opacity: 0;
    transform: scale(0.88);
}

.gds-app-menu-button:hover .gds-app-menu-button__glyph {
    opacity: 1;
    transform: scale(1);
}

.zui-menu__item.zui-menu__item:not(.zui-menu__item--destructive):hover,
.zui-menu__item.zui-menu__item:not(.zui-menu__item--destructive):focus,
.zui-menu__item.zui-menu__item[data-state="open"]:not(.zui-menu__item--destructive),
.zui-select__item.zui-select__item[data-active="true"] {
    background-color: var(--gds-hover);
    color: var(--zui-color-popover-foreground);
}

.zui-menu__item.zui-menu__item:not(.zui-menu__item--destructive):active,
.zui-select__item.zui-select__item:active {
    background-color: var(--gds-pressed);
}

.gds-capacity {
    display: flex;
    flex-direction: column;
    gap: 4px;
    width: 100%;
}

.gds-capacity__label {
    font-size: var(--gds-font-size-xs, 12px);
    color: var(--gds-muted-foreground);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
}

.gds-capacity[data-selected="true"] .gds-capacity__label {
    color: var(--gds-selection-foreground);
}

.gds-capacity__track {
    width: 100%;
    height: 4px;
    border-radius: 2px;
    background: color-mix(in srgb, var(--gds-foreground) 12%, transparent);
    border: none;
    overflow: hidden;
}

.gds-capacity__fill {
    height: 100%;
    width: var(--gds-capacity-width, 0%);
    background: var(--gds-primary);
    transform: translateX(0);
    transition: width var(--gds-motion-normal, 160ms) var(--gds-motion-ease-out, ease-out);
}

.gds-capacity[data-selected="true"] .gds-capacity__fill {
    background: var(--gds-selection-foreground);
}

.gds-capacity[data-unavailable="true"] .gds-capacity__fill {
    width: 0%;
    background: var(--gds-muted-foreground);
    opacity: 0.35;
}

.gds-capacity[data-loading="true"] .gds-capacity__fill {
    width: 40%;
    animation: gds-capacity-indeterminate 1200ms var(--gds-motion-ease-standard, ease) infinite;
}

@keyframes gds-capacity-indeterminate {
    0% { transform: translateX(-100%); }
    100% { transform: translateX(250%); }
}

.gds-capacity[data-compact="true"] {
    width: auto;
    min-width: 28px;
    align-items: center;
}

.gds-capacity[data-compact="true"] .gds-capacity__label {
    font-variant-numeric: tabular-nums;
}

.gds-disclosure {
    display: flex;
    flex-direction: column;
    width: 100%;
}

.gds-disclosure__trigger {
    display: flex;
    flex-direction: row;
    align-items: center;
    gap: 6px;
    cursor: pointer;
    background: transparent;
    border: none;
    padding: 4px 6px;
    border-radius: var(--gds-radius-control, 6px);
    color: var(--gds-foreground);
}

.gds-disclosure__trigger:hover {
    background: var(--gds-hover);
}

.gds-disclosure-icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex: none;
    color: inherit;
    transform: rotate(0deg);
    transition: transform var(--gds-motion-inline, 140ms) var(--gds-motion-ease-out, ease-out);
}

.gds-disclosure-icon[data-state="open"] {
    transform: rotate(90deg);
    color: inherit;
}

.gds-link {
    color: var(--gds-link);
    cursor: pointer;
    background: transparent;
    border: none;
    padding: 0;
}

.gds-help-page {
    width: 100%;
    height: 100%;
    padding: 24px;
    background: var(--gds-content-bg, var(--gds-content));
}

.gds-help-page__body {
    display: flex;
    flex-direction: column;
    gap: 18px;
    max-width: 720px;
    width: 100%;
}

.gds-shortcut-section {
    display: flex;
    flex-direction: column;
    padding: 0 0 24px 0;
    background: transparent;
    border: none;
    border-radius: 0;
}

.gds-shortcut-heading {
    font-size: var(--gds-font-size-2xl, 24px);
    font-weight: 600;
    color: var(--gds-foreground);
    padding-bottom: 16px;
}

.gds-shortcut-row {
    display: flex;
    flex-direction: row;
    align-items: center;
    justify-content: space-between;
    gap: 24px;
    padding: 8px 0;
    border-bottom: 1px solid var(--gds-border);
}

.gds-shortcut-row__label {
    font-size: var(--gds-font-size-small, 14px);
    color: var(--gds-foreground);
}

.gds-shortcut-row__keys {
    display: flex;
    flex-direction: row;
    align-items: center;
    gap: 6px;
}

.gds-shortcut-hint {
    display: inline-flex;
    flex-direction: row;
    align-items: center;
    gap: 16px;
}

.gds-tooltip-text {
    color: var(--gds-panel-foreground);
    font-size: var(--gds-font-size-small, 14px);
}

.gds-timestamp,
.gds-clock {
    font-variant-numeric: tabular-nums;
}

.gds-field-placeholder {
    color: var(--gds-muted-foreground);
    font-size: var(--gds-font-size-small, 14px);
}

.gds-setting-form {
    display: flex;
    flex-direction: column;
    gap: 16px;
    width: 100%;
}

.gds-setting-form[data-disabled="true"] {
    opacity: 0.6;
}

.gds-setting-section {
    display: flex;
    flex-direction: column;
    width: 100%;
}

.gds-setting-group {
    font-size: var(--gds-font-size-small, 14px);
    font-weight: 600;
    color: var(--gds-muted-foreground);
    padding-top: 6px;
}

.gds-setting-row {
    display: flex;
    flex-direction: row;
    align-items: center;
    justify-content: space-between;
    gap: 24px;
    padding: 16px 0;
    border-radius: 0;
    background: transparent;
    border: none;
    border-bottom: 1px solid var(--gds-border);
}

.gds-setting-label {
    display: flex;
    flex-direction: column;
    gap: 3px;
    flex: 1 1 auto;
    min-width: 0;
}

.gds-setting-title {
    font-size: var(--gds-font-size-small, 14px);
    font-weight: 500;
    color: var(--gds-foreground);
}

.gds-setting-description {
    font-size: var(--gds-font-size-small, 14px);
    color: var(--gds-muted-foreground);
}

.gds-setting-error {
    font-size: var(--gds-font-size-xs, 12px);
    color: var(--gds-error);
}

.gds-setting-actions {
    display: flex;
    flex-direction: row;
    align-items: center;
    gap: var(--gds-setting-gap, 8px);
    flex: none;
}

.gds-setting-control {
    width: var(--gds-setting-control-width, 260px);
    display: flex;
    flex-direction: row;
    justify-content: flex-end;
    align-items: center;
}

.gds-setting-reset {
    width: var(--gds-setting-reset-size, 32px);
    height: var(--gds-setting-reset-size, 32px);
    display: inline-flex;
    align-items: center;
    justify-content: center;
}

.gds-setting-number {
    width: 100%;
}

.gds-setting-number-row {
    display: flex;
    flex-direction: row;
    align-items: center;
    gap: 10px;
    width: 100%;
}

.gds-setting-number-value {
    width: 4.5rem;
    text-align: right;
    white-space: nowrap;
    font-size: var(--gds-font-size-small, 14px);
    font-variant-numeric: tabular-nums;
    color: var(--gds-muted-foreground);
}
"#;

fn base_css() -> &'static str {
    static SHEET_TEXT: OnceLock<String> = OnceLock::new();
    SHEET_TEXT.get_or_init(|| {
        let btn = ButtonStyle::CLASS;
        let inp = InputStyle::CLASS;
        let txt = TextareaStyle::CLASS;
        let grp = InputGroupStyle::CLASS;
        let sel = SelectStyle::CLASS;
        let nsel = NativeSelectStyle::CLASS;
        let sld = SliderStyle::CLASS;
        let sw = SwitchStyle::CLASS;

        format!(
            "{STATIC_BASE_CSS}
.zui-button.{btn},
.{inp}.{inp},
.{txt}.{txt},
.{grp}.{grp},
.{sel}.{sel},
.{nsel}.{nsel},
.{sw}.{sw} {{
    --zui-button-ring: 0 0 transparent;
    box-shadow: none;
}}

.zui-slider.{sld} > .zui-slider__track {{
    background-color: color-mix(in srgb, var(--gds-foreground) 14%, transparent);
}}

.zui-slider.{sld}:focus-visible > .zui-slider__thumb {{
    box-shadow: none;
    outline: 1px solid var(--gds-primary);
    outline-offset: 1px;
}}

.zui-button.{btn}:focus:not(:focus-visible) {{
    outline: none;
    box-shadow: none;
}}

.zui-button.{btn}[data-variant=\"ghost\"]:hover,
.zui-button.{btn}[data-variant=\"outline\"]:hover {{
    background-color: var(--gds-hover);
    color: inherit;
}}

.zui-button.{btn}[data-variant=\"ghost\"]:active,
.zui-button.{btn}[data-variant=\"outline\"]:active {{
    background-color: var(--gds-pressed);
    color: inherit;
}}

.zui-button.{btn}.gds-button[data-toggled=\"true\"],
.zui-button.{btn}.gds-button[data-toggled=\"true\"]:hover,
.zui-button.{btn}.gds-icon-button[data-toggled=\"true\"],
.zui-button.{btn}.gds-icon-button[data-toggled=\"true\"]:hover {{
    background-color: var(--gds-selection);
    color: var(--gds-selection-foreground);
    border-color: transparent;
}}

.zui-button.{btn}:focus-visible {{
    --zui-button-ring: 0 0 transparent;
    outline: 1px solid var(--gds-primary);
    outline-offset: -1px;
}}

.zui-button.{btn}[data-variant=\"ghost\"]:focus-visible,
.zui-button.{btn}[data-variant=\"secondary\"]:focus-visible,
.zui-button.{btn}[data-variant=\"link\"]:focus-visible {{
    border-color: transparent;
}}

.zui-button.{btn}[data-variant=\"default\"]:focus-visible {{
    border-color: transparent;
    outline: 1px solid var(--gds-primary-foreground);
    outline-offset: -1px;
}}

.zui-button.{btn}[data-variant=\"destructive\"]:focus-visible {{
    border-color: transparent;
    outline: 1px solid var(--gds-error-foreground);
    outline-offset: -1px;
}}

.{inp}.{inp}:focus,
.{inp}.{inp}:focus-visible,
.zui-input.zui-input:focus,
.zui-input.zui-input:focus-visible,
.{txt}.{txt}:focus,
.{txt}.{txt}:focus-visible,
.{grp}.{grp}:focus-within,
.{sel}.{sel}:focus,
.{sel}.{sel}:focus-visible,
.{nsel}.{nsel}:focus,
.{nsel}.{nsel}:focus-visible {{
    border-color: var(--gds-primary);
    box-shadow: none;
    outline: none;
}}

.{sw}.{sw}:focus:not(:focus-visible) {{
    outline: none;
    box-shadow: none;
}}

.{sw}.{sw}:focus-visible {{
    box-shadow: none;
    outline: 1px solid var(--gds-primary);
    outline-offset: 2px;
}}

.{inp}.{inp}:invalid:focus,
.{inp}.{inp}:invalid:focus-visible,
.{txt}.{txt}:invalid:focus,
.{txt}.{txt}:invalid:focus-visible,
.{grp}.{grp}[data-invalid=\"true\"],
.{sel}.{sel}:invalid:focus,
.{sel}.{sel}:invalid:focus-visible {{
    border-color: var(--gds-error);
    box-shadow: none;
}}
"
        )
    })
}

/// Install shared GDS structural CSS classes into the active `zgui` window document.
///
/// Safe to call repeatedly across components; installation is idempotent per document.
pub fn install_ui_stylesheets() {
    install_stylesheet(BASE_SHEET, base_css());
}
