//! Read-only help content and shortcut reference components.

use zgui::prelude::*;
use zgui::reactive::{LocalStorage, Signal, StoredValue};
use zgui::view::install_stylesheet;
use zgui::vocab::Role;
use zgui::{component, css, view};
use zgui_ui_primitives::Binding;

use crate::ui::dialog::{DialogHeaderProps, DialogSize, DialogTitleProps, ModalDialogProps};
use crate::ui::{Shortcut, ShortcutBadgeProps};

const HELP_SHEET: &str = "gds-ui-help";

const HELP_CSS: &str = css!(
    r#"
    .gds-help-page.gds-help-page {
        width: 100%;
        height: 100%;
        min-width: 0px;
        min-height: 0px;
        overflow-y: auto;
        overflow-x: hidden;
        padding: 24px;
        background: var(--gds-content-bg, var(--gds-content));
        color: var(--gds-foreground);
        font-size: var(--gds-text-sm, 14px);
    }

    .gds-help-page__body.gds-help-page__body {
        display: flex;
        flex-direction: column;
        gap: 0px;
        width: 100%;
        max-width: 880px;
        min-width: 0px;
    }

    .gds-shortcut-section.gds-shortcut-section {
        display: flex;
        flex-direction: column;
        gap: 0px;
        padding: 0px 0px 24px 0px;
        border: none;
        border-radius: 0px;
        background: transparent;
    }

    .gds-shortcut-heading.gds-shortcut-heading {
        font-size: var(--gds-text-lg, 20px);
        line-height: 1.25;
        font-weight: 600;
        color: var(--gds-foreground);
        padding: 0px 0px 16px 0px;
    }

    .gds-shortcut-row.gds-shortcut-row {
        display: flex;
        flex-direction: row;
        align-items: center;
        justify-content: space-between;
        gap: 24px;
        padding: 8px 0px;
        border-bottom: 1px solid var(--gds-border);
    }

    .gds-shortcut-row__label.gds-shortcut-row__label {
        flex: 1 1 0%;
        min-width: 0px;
        font-size: var(--gds-text-sm, 14px);
        color: var(--gds-foreground);
    }

    .gds-shortcut-row__keys.gds-shortcut-row__keys {
        display: flex;
        flex-direction: row;
        align-items: center;
        flex-shrink: 0;
        gap: 8px;
        white-space: nowrap;
    }

    .gds-help-dialog.gds-help-dialog {
        height: 560px;
        max-height: calc(100vh - 32px);
        padding: 0px;
        overflow: hidden;
    }
    "#
);

fn install_help_stylesheets() {
    crate::ui::styles::install_ui_stylesheets();
    install_stylesheet(HELP_SHEET, HELP_CSS);
}

/// Semantic trigger token for opening application help.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OpenHelp;

/// Scrollable keyboard-shortcut and reference page container.
#[component]
pub fn HelpPage(
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
    children: Children,
) -> impl IntoView {
    install_help_stylesheets();
    view! {
        scroll(class = "gds-help-page", class = class, attr:data-testid = "help-page", {..attrs}) {
            column(class = "gds-help-page__body") {
                {children.into_view_once()}
            }
        }
    }
}

/// Group of related shortcuts inside a [`HelpPage`].
#[component]
pub fn ShortcutSection(
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
    children: Children,
) -> impl IntoView {
    install_help_stylesheets();
    view! {
        column(class = "gds-shortcut-section", class = class, {..attrs}) {
            {children.into_view_once()}
        }
    }
}

/// Section heading inside a [`ShortcutSection`].
#[component]
pub fn ShortcutHeading(
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
    children: Children,
) -> impl IntoView {
    install_help_stylesheets();
    view! {
        text(
            class = "gds-shortcut-heading",
            a11y:role = Role::Heading,
            class = class,
            {..attrs}
        ) {
            {children.into_view_once()}
        }
    }
}

/// Single labelled keyboard shortcut row (or range of two shortcuts).
#[component]
pub fn ShortcutRow(
    #[prop(into)] label: Signal<String, LocalStorage>,
    shortcut: Shortcut,
    #[prop(optional)] range_end: Option<Shortcut>,
) -> impl IntoView {
    install_help_stylesheets();
    view! {
        row(class = "gds-shortcut-row") {
            text(class = "gds-shortcut-row__label") {{move || label.get()}}
            row(class = "gds-shortcut-row__keys") {
                ShortcutBadge(shortcut = shortcut) {}
                {range_end.map(|end| view! {
                    row(class = "gds-shortcut-row__keys") {
                        text {"\u{2013}"}
                        ShortcutBadge(shortcut = end)
                    }
                })}
            }
        }
    }
}

/// Optional modal dialog wrapper around [`HelpPage`].
#[component]
pub fn HelpDialog(
    #[prop(into, optional)] open: Binding<bool>,
    #[prop(into)] title: Signal<String, LocalStorage>,
    children: ChildrenFn,
) -> impl IntoView {
    install_help_stylesheets();
    let children_store = StoredValue::new_local(children);
    view! {
        ModalDialog(
            open = open,
            size = DialogSize::Lg,
            disabled = true,
            class = "gds-help-dialog",
        ) {
            DialogHeader {
                DialogTitle {{move || title.get()}}
            }
            HelpPage {
                {children_store.with_value(ChildrenFn::view)}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use zgui::app::app;
    use zgui::platform::{AppHandler, PlatformError, Surface, SurfaceEvent};
    use zgui::prelude::*;
    use zgui::reactive::{Mounted, RwSignal, install};
    use zgui::vocab::{KeyEvent, KeyState, Modifiers, Timestamp};
    use zgui_platform_headless::Harness;

    use super::{
        HelpDialogProps, HelpPageProps, OpenHelp, Shortcut, ShortcutHeadingProps, ShortcutRowProps,
        ShortcutSectionProps,
    };

    #[derive(Default)]
    struct NullRenderer {
        target: Option<zgui::render::RenderTarget>,
        atlas: zgui::atlas::MemorySink,
    }

    impl zgui::render::Renderer for NullRenderer {
        fn capabilities(&self) -> zgui::render::RenderCapabilities {
            zgui::render::RenderCapabilities::MINIMAL
        }

        fn configure(&mut self, target: zgui::render::RenderTarget) {
            self.target = Some(target);
        }

        fn target(&self) -> Option<zgui::render::RenderTarget> {
            self.target
        }

        fn draw(
            &mut self,
            _scene: &zgui::scene::Scene,
            _damage: &zgui::bits::DamageSet,
        ) -> zgui::render::FrameOutcome {
            zgui::render::FrameOutcome::Presented(zgui::render::FrameStats {
                vector_passes: 0,
                draw_calls: 0,
                damage_px: 0,
                bytes_uploaded: 0,
                memory: zgui::render::MemoryReport::ZERO,
            })
        }

        fn register_external(
            &mut self,
            _texture: zgui::render::ExternalTexture,
        ) -> zgui::render::TextureHandle {
            zgui::render::TextureHandle(0)
        }

        fn release_external(&mut self, _handle: zgui::render::TextureHandle) {}

        fn memory(&self) -> zgui::render::MemoryReport {
            zgui::render::MemoryReport::ZERO
        }

        fn texture_sink(&mut self) -> &mut dyn zgui::atlas::TextureSink {
            &mut self.atlas
        }
    }

    #[test]
    fn help_components_build_sections_shortcut_ranges_and_dialog_dismissal() {
        install().ok();
        let scope = Mounted::new();
        scope.with(|| {
            let _token = OpenHelp;
            let _view = view! {
                HelpPage {
                    ShortcutSection {
                        ShortcutHeading { "Navigation" }
                        ShortcutRow(
                            label = "Back",
                            shortcut = Shortcut::parse("Alt+Left").expect("valid Alt+Left shortcut"),
                        )
                        ShortcutRow(
                            label = "Switch tab",
                            shortcut = Shortcut::parse("Ctrl+1").expect("valid Ctrl+1 shortcut"),
                            range_end = Shortcut::parse("Ctrl+9"),
                        )
                    }
                }
            };
        });
        scope.unmount();

        let open_cell = std::rc::Rc::new(std::cell::RefCell::new(None));
        let open_for_view = std::rc::Rc::clone(&open_cell);
        let handler = app()
            .with_title("help-test")
            .with_application_id("dev.gds.HelpTest")
            .with_size(800.0, 600.0)
            .with_stylesheet(":root { display: flex; width: 100%; height: 100%; }")
            .with_renderer(Box::new(
                move |_surface: &std::sync::Arc<dyn Surface>,
                      target: zgui::render::RenderTarget| {
                    let mut renderer = NullRenderer::default();
                    zgui::render::Renderer::configure(&mut renderer, target);
                    Ok::<_, zgui::Error>(Box::new(renderer) as Box<dyn zgui::render::Renderer>)
                },
            ))
            .into_handler(move || {
                let open = RwSignal::new_local(true);
                *open_for_view.borrow_mut() = Some(open);
                view! {
                    HelpDialog(open = open, title = "Keyboard Shortcuts") {
                        ShortcutSection {
                            ShortcutHeading { "General" }
                            ShortcutRow(
                                label = "Help",
                                shortcut = Shortcut::parse("F1").expect("F1 shortcut"),
                            )
                        }
                    }
                }
            })
            .expect("build headless help dialog test");

        handler
            .drive(|app| -> Result<(), PlatformError> {
                let mut harness: Harness<Box<dyn AppHandler>> = Harness::new(app);
                for _ in 0..4 {
                    harness.pump();
                }
                harness.deliver_to_first(SurfaceEvent::Focused(true));
                harness.pump();
                let open = open_cell.borrow().expect("open signal");
                assert!(open.get_untracked());

                let alt_c = KeyEvent::character("c");
                harness.deliver_to_first(SurfaceEvent::Key {
                    state: KeyState::Pressed,
                    event: alt_c.clone(),
                    modifiers: Modifiers::ALT,
                    timestamp: Timestamp::from_origin(Duration::from_millis(10)),
                });
                harness.deliver_to_first(SurfaceEvent::Key {
                    state: KeyState::Released,
                    event: alt_c,
                    modifiers: Modifiers::ALT,
                    timestamp: Timestamp::from_origin(Duration::from_millis(15)),
                });
                for _ in 0..4 {
                    harness.pump();
                }
                assert!(!open.get_untracked(), "Alt+C should dismiss HelpDialog");
                harness.shut_down();
                Ok(())
            })
            .expect("drive headless help dialog test");
    }
}
