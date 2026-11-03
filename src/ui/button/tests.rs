use super::*;
use gpui_kit::{
    AppContext, Bounds, Context, Entity, Pixels, Render, TestAppContext,
    component::{ActiveTheme, Root, input::InputState, select::SelectState, switch::Switch},
    div,
    prelude::*,
    size,
    test::TestWindowExt,
};

struct Controls {
    input: Entity<InputState>,
    select: Entity<SelectState<Vec<SharedString>>>,
    clicks: usize,
    enabled: bool,
}

impl Render for Controls {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .w(px(280.))
            .child(
                Button::new("plain")
                    .label("Plain")
                    .on_click(cx.listener(|this, _, _, _| this.clicks += 1)),
            )
            .child(
                Button::new("primary")
                    .primary()
                    .label("Primary")
                    .on_click(cx.listener(|this, _, _, _| this.clicks += 1)),
            )
            .child(
                Button::new("danger")
                    .danger()
                    .label("Danger")
                    .on_click(cx.listener(|this, _, _, _| this.clicks += 1)),
            )
            .child(
                Button::icon_button("icon")
                    .control_icon(IconName::Plus)
                    .on_click(cx.listener(|this, _, _, _| this.clicks += 1)),
            )
            .child(crate::ui::field::input(&self.input, cx).id("focus-input"))
            .child(crate::ui::field::select(&self.select, cx).id("select"))
            .child(
                Switch::new("switch")
                    .checked(self.enabled)
                    .on_click(cx.listener(|this, checked, _, cx| {
                        this.enabled = *checked;
                        cx.notify();
                    })),
            )
    }
}

fn borders_within(window: &Window, bounds: Bounds<Pixels>) -> Vec<gpui_kit::Quad> {
    let scale = window.scale_factor();
    window
        .painted_quads()
        .into_iter()
        .filter(|quad| {
            let painted = quad.bounds.map(|value| px(value.0 / scale));
            quad.border_color.a > 0.
                && quad.border_widths.any(|width| width.0 > 0.)
                && bounds.contains(&painted.center())
        })
        .collect()
}

#[gpui_kit::test]
fn thin_control_focus_preserves_geometry_contrast_and_activation(cx: &mut TestAppContext) {
    use crate::appearance::theme::{self, Appearance, BuiltinTheme, ModePreference};
    cx.update(|cx| {
        gpui_kit::init(cx);
        cx.set_reduce_motion(true);
    });
    let mut controls = None;
    let window = cx.open_window(size(px(500.), px(600.)), |window, cx| {
        let view = cx.new(|cx| Controls {
            input: cx.new(|cx| InputState::new(window, cx)),
            select: cx.new(|cx| {
                SelectState::new(
                    vec![SharedString::from("One"), "Two".into()],
                    None,
                    window,
                    cx,
                )
            }),
            clicks: 0,
            enabled: false,
        });
        controls = Some(view.clone());
        Root::new(view, window, cx)
    });
    let controls = controls.expect("controls view");
    for builtin in [BuiltinTheme::Bamboo, BuiltinTheme::HighContrast] {
        for mode in [ModePreference::Light, ModePreference::Dark] {
            cx.update_window(window.into(), |_, window, cx| {
                theme::apply(
                    &Appearance {
                        builtin,
                        mode,
                        ..Default::default()
                    },
                    cx,
                )
                .expect("theme");
                window.blur(cx);
                window.render_frame(cx);
                window.click("switch", cx);
                let ids = [
                    "plain",
                    "primary",
                    "danger",
                    "icon",
                    "focus-input",
                    "select",
                    "switch",
                ];
                let bounds: Vec<_> = ids.iter().map(|id| window.find(*id).bounds()).collect();
                for (id, bounds) in ids.into_iter().zip(bounds) {
                    window.press("tab", cx);
                    window.render_frame(cx);
                    assert_eq!(window.find(id).focused(), Some(true), "Tab reaches {id}");
                    assert_eq!(window.find(id).bounds(), bounds, "focus must not move {id}");
                    let borders = borders_within(window, bounds);
                    assert!(
                        !borders.is_empty(),
                        "{id} must retain focus ink in {builtin:?}/{mode:?}"
                    );
                    for border in &borders {
                        assert!(
                            !border
                                .border_widths
                                .any(|width| width.0 > window.scale_factor()),
                            "no thick halo on {id}"
                        );
                        let painted = border
                            .bounds
                            .map(|value| px(value.0 / window.scale_factor()));
                        assert!(bounds.contains(&painted.origin), "focus stays inside {id}");
                        assert!(
                            painted.right() <= bounds.right()
                                && painted.bottom() <= bounds.bottom(),
                            "focus stays inside {id}"
                        );
                    }
                    if ["primary", "danger", "icon"].contains(&id) {
                        // Filled controls need a contrasting inner line, not the same
                        // accent as their fill. The shared ghost button is custom too.
                        assert!(
                            borders
                                .iter()
                                .any(|quad| quad.border_color != cx.theme().ring)
                        );
                        window.click(id, cx);
                        window.render_frame(cx);
                        assert_eq!(
                            window.find(id).focused(),
                            Some(true),
                            "pointer does not steal established keyboard focus"
                        );
                        assert!(
                            borders_within(window, bounds).is_empty(),
                            "returning to pointer input hides the keyboard cursor on {id}"
                        );
                    }
                }
                window.press("shift-tab", cx);
                assert_eq!(window.find("select").focused(), Some(true));
            })
            .expect("keyboard focus paints a stable thin contour");
        }
    }
    cx.update_window(window.into(), |_, window, cx| {
        let before = controls.read(cx).clicks;
        for id in ["plain", "primary", "danger", "icon"] {
            window.click(id, cx);
            window.render_frame(cx);
            let bounds = window.find(id).bounds();
            if id != "plain" {
                assert!(
                    borders_within(window, bounds).is_empty(),
                    "pointer activation must not retain the keyboard-only line on {id}"
                );
            }
        }
        assert_eq!(
            controls.read(cx).clicks,
            before + 4,
            "focus decoration cannot intercept clicks"
        );
        window.click("focus-input", cx);
        assert!(
            !borders_within(window, window.find("focus-input").bounds()).is_empty(),
            "editing keeps a boundary even with pointer focus"
        );
        window.input("edited", cx);
        assert_eq!(controls.read(cx).input.read(cx).value(), "edited");
        window.click("switch", cx);
        assert!(controls.read(cx).enabled);
        window.press("space", cx);
        assert!(
            !controls.read(cx).enabled,
            "keyboard activation still toggles the switch"
        );
    })
    .expect("pointer and keyboard interaction survive decorations");
}

struct ShortcutButton {
    focus: gpui_kit::FocusHandle,
}

impl Render for ShortcutButton {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .track_focus(&self.focus)
            .on_action(|_: &crate::ui::actions::FocusNotifications, _, _| {})
            .flex()
            .child(
                Button::new("shortcut-button")
                    .label("Notify")
                    .shortcut(crate::ui::actions::Command::FocusNotifications),
            )
    }
}

#[gpui_kit::test]
fn shortcut_button_grows_reverses_and_releases_its_hint_space(cx: &mut TestAppContext) {
    use gpui_kit::{InputEvent, KeyBinding, Modifiers, ModifiersChangedEvent};
    use std::time::Duration;
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::ui::keytips::install(cx);
        cx.bind_keys([KeyBinding::new(
            "alt-n",
            crate::ui::actions::FocusNotifications,
            None,
        )]);
    });
    let window = cx.open_window(size(px(400.), px(100.)), |window, cx| {
        let view = cx.new(|cx| {
            let focus = cx.focus_handle();
            focus.focus(window, cx);
            ShortcutButton { focus }
        });
        Root::new(view, window, cx)
    });
    cx.run_until_parked();
    let width = |window: &mut Window, cx: &mut gpui_kit::App| {
        window.render_frame(cx);
        window.find("shortcut-button").bounds().size.width
    };
    let modifiers = |alt, window: &mut Window, cx: &mut gpui_kit::App| {
        window.dispatch_event(
            ModifiersChangedEvent {
                modifiers: Modifiers {
                    alt,
                    ..Default::default()
                },
                capslock: window.capslock(),
            }
            .to_platform_input(),
            cx,
        );
        window.render_frame(cx);
    };
    let initial = cx
        .update_window(window.into(), |_, window, cx| width(window, cx))
        .unwrap();
    cx.update_window(window.into(), |_, window, cx| {
        modifiers(true, window, cx);
        assert_eq!(width(window, cx), initial);
    })
    .unwrap();
    cx.executor().advance_clock(Duration::from_millis(50));
    let middle = cx
        .update_window(window.into(), |_, window, cx| {
            window.simulate_next_frame(cx);
            let middle = width(window, cx);
            assert!(middle > initial);
            modifiers(false, window, cx);
            assert_eq!(
                width(window, cx),
                middle,
                "release reverses from the visible width"
            );
            middle
        })
        .unwrap();
    cx.executor().advance_clock(Duration::from_millis(180));
    cx.update_window(window.into(), |_, window, cx| {
        window.simulate_next_frame(cx);
        assert_eq!(width(window, cx), initial, "no hint space remains");
        cx.set_reduce_motion(true);
        modifiers(true, window, cx);
        assert!(width(window, cx) > middle);
        modifiers(false, window, cx);
        assert_eq!(
            width(window, cx),
            initial,
            "reduced motion settles immediately"
        );
    })
    .unwrap();
}
