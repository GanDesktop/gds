//! Application pages and confirmations; these never send commands to bas.

use crate::shared::i18n::tr;

use gds_uim::el;
use gpui_kit::{
    App, AppContext, Bounds, ClickEvent, Context, Focusable, Global, Render, TestSupportExt,
    Window, WindowBounds, WindowHandle, WindowKind, WindowOptions,
    component::{ActiveTheme, Root},
    div,
    prelude::*,
    px, size,
};

use crate::shared::lib::appearance as theme;
use crate::shared::ui::window_controls::TitleBar;

use gds::ui::about::About;
use gds::ui::dialog::{DIALOG_CONTEXT, DialogCommand, DialogDecision, DialogFooter};
use gpui_kit::component::dialog::Cancel;
const WIDTH: f32 = 480.;
const HEIGHT: f32 = 420.;
const ABOUT_HEIGHT: f32 = gds::ui::about::HEIGHT + gds::ui::window::HEIGHT + 48.;

pub(crate) use crate::shared::routes::{OpenSettings, Quit};

#[derive(Clone, Copy)]
enum InfoWindow {
    About,
    Confirmation,
}

#[derive(Default)]
struct OpenWindows {
    about: Option<WindowHandle<Root>>,
    confirmation: Option<WindowHandle<Root>>,
}
impl Global for OpenWindows {}

pub(crate) struct Confirmation {
    pub title: String,
    pub message: String,
    pub confirm_label: String,
    pub destructive: bool,
}

/// Repeated requests focus the pending confirmation and discard the new request.
/// Cancel/close drops its callback without executing it.
///
/// # Errors
/// Returns a platform error if the confirmation window cannot be created.
pub(crate) fn open_confirmation(
    request: Confirmation,
    on_confirm: impl FnOnce(&mut App) + 'static,
    cx: &mut App,
) -> anyhow::Result<WindowHandle<Root>> {
    open_confirmation_with_option(request, None, on_confirm, cx)
}

/// An optional choice whose value is committed only when the dialog is confirmed.
pub(crate) struct ConfirmationOption {
    pub id: gpui_kit::ElementId,
    pub label: gpui_kit::SharedString,
    pub checked: bool,
    pub on_confirm: Box<dyn Fn(bool, &mut Window, &mut App) -> Result<(), String>>,
}

/// # Errors
/// Reports a native-window failure. An option error keeps the dialog open.
pub(crate) fn open_confirmation_with_option(
    request: Confirmation,
    option: Option<ConfirmationOption>,
    on_confirm: impl FnOnce(&mut App) + 'static,
    cx: &mut App,
) -> anyhow::Result<WindowHandle<Root>> {
    open_host(
        Host {
            kind: InfoWindow::Confirmation,
            title: request.title,
            message: request.message,
            confirm_label: request.confirm_label,
            destructive: request.destructive,
            on_confirm: Some(Box::new(on_confirm)),
            option,
            about: None,
        },
        cx,
    )
}

/// # Errors
/// Returns a platform error if the About window cannot be created.
pub(crate) fn open_about_window(cx: &mut App) -> anyhow::Result<WindowHandle<Root>> {
    open_host(Host::new(InfoWindow::About), cx)
}

impl InfoWindow {
    fn window_kind(self) -> WindowKind {
        if matches!(self, Self::Confirmation) {
            WindowKind::Floating
        } else {
            WindowKind::Normal
        }
    }

    fn title(self) -> &'static str {
        match self {
            // [!todo] here we have product name `Agents`, witch should be a static value and easy to replace with
            // other name
            Self::About => tr!("app-about"),
            Self::Confirmation => tr!("app-confirm"),
        }
    }

    fn slot(self, windows: &mut OpenWindows) -> &mut Option<WindowHandle<Root>> {
        match self {
            Self::About => &mut windows.about,
            Self::Confirmation => &mut windows.confirmation,
        }
    }
}

/// # Errors
/// Returns a platform error when opening a native window fails.
fn open_host(mut host: Host, cx: &mut App) -> anyhow::Result<WindowHandle<Root>> {
    let kind = host.kind;
    let height = if matches!(kind, InfoWindow::About) {
        ABOUT_HEIGHT
    } else {
        HEIGHT
    };
    if !cx.has_global::<OpenWindows>() {
        cx.set_global(OpenWindows::default());
    }
    if let Some(handle) = *kind.slot(cx.global_mut::<OpenWindows>())
        && handle
            .update(cx, |_, window, _| window.activate_window())
            .is_ok()
    {
        return Ok(handle);
    }
    if matches!(kind, InfoWindow::About) {
        host.about = Some(cx.new(|cx| {
            About::new(
                gds::ui::about::Application::cargo(env!("CARGO_BIN_NAME")),
                cx,
            )
        }));
    }
    let handle = cx.open_window(
        WindowOptions {
            kind: kind.window_kind(),
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                gds::ui::window::fit_size(size(px(WIDTH), px(height)), cx),
                cx,
            ))),
            window_background: cx.global::<theme::Appearance>().background,
            app_id: Some("io.github.GanDesktop.GdsAgents".into()),
            ..TitleBar::window_options()
        },
        |window, cx| {
            window.set_window_title(&host.title);
            if let Some(about) = &host.about {
                about.focus_handle(cx).focus(window, cx);
            }
            let host = cx.new(|_| host);
            window.activate_window();
            cx.new(|cx| theme::window_root(host, window, cx))
        },
    )?;
    *kind.slot(cx.global_mut::<OpenWindows>()) = Some(handle);
    Ok(handle)
}

type ConfirmCallback = Box<dyn FnOnce(&mut App)>;

struct Host {
    kind: InfoWindow,
    title: String,
    message: String,
    confirm_label: String,
    destructive: bool,
    on_confirm: Option<ConfirmCallback>,
    option: Option<ConfirmationOption>,
    about: Option<gpui_kit::Entity<About>>,
}

impl Host {
    fn new(kind: InfoWindow) -> Self {
        Self {
            kind,
            title: kind.title().into(),
            message: String::new(),
            confirm_label: String::new(),
            destructive: false,
            on_confirm: None,
            option: None,
            about: None,
        }
    }

    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(option) = &self.option
            && let Err(message) = (option.on_confirm)(option.checked, window, cx)
        {
            self.message = message;
            cx.notify();
            return;
        }
        let callback = self.on_confirm.take();
        Self::close(window);
        if let Some(callback) = callback {
            callback(cx);
        }
    }

    fn close(window: &mut Window) {
        window.remove_window();
    }

    fn on_close(_: &ClickEvent, window: &mut Window, _: &mut App) {
        Self::close(window);
    }

    fn on_about_cancel(&mut self, _: &Cancel, window: &mut Window, cx: &mut Context<Self>) {
        cx.stop_propagation();
        Self::close(window);
    }

    fn on_confirm(&mut self, _: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.confirm(window, cx);
    }

    fn message(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let confirmation = matches!(self.kind, InfoWindow::Confirmation);
        let actions = el! {
            .div {
                flex;
                flex_shrink_0;
                justify_end;
                gap_3;
            } [
                ("message-close").crate::shared::ui::action_button {
                    label: $(if confirmation {
                            tr!("action-cancel")
                        } else {
                            tr!("action-close")
                    });
                    on_click: Self::on_close;
                },
                ..$(confirmation.then(|| el! {("message-confirm").crate::shared::ui::action_button {
                                label: $(self.confirm_label.clone());
                                when: $(self.destructive), {
                                    text_color: $(cx.theme().danger);
                                };
                                on_click: $(cx.listener(Self::on_confirm));
                }})),
            ]
        };
        el! {
            .div {
                flex;
                flex_col;
                flex_1;
                min_h_0;
                gap_6;
                p_6;
            } [
                .div {
                    # "application-message-scroll";
                    flex_1;
                    min_h_0;
                    overflow_y_scroll;
                    whitespace_normal;
                } [
                    $(crate::shared::ui::feedback("application-message-body", self.message.clone(), cx.theme().foreground)),
                ],
                ..$(self.option.as_ref().map(|option| el! {($(option.id.clone())).crate::shared::ui::checkbox {
                                checked: $(option.checked);
                                label: $(option.label.clone());
                                on_click: $(cx.listener(|this, checked: &bool, _, cx| {
                                            this.option.as_mut().expect("the option checkbox belongs to this dialog").checked = *checked;
                                            cx.notify();
                                }));
                }})),
                actions,
            ]
        }
    }
}

impl Render for Host {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if matches!(self.kind, InfoWindow::About) {
            self.title = self.kind.title().into();
            window.set_window_title(&self.title);
        }
        let id = match self.kind {
            InfoWindow::About => "about-window",
            InfoWindow::Confirmation => "confirmation-window",
        };
        let content = match self.kind {
            InfoWindow::About => el! {
                .div { flex_1; min_h_0; flex; flex_col; } [
                    $(self.about.as_ref().expect("About host owns its content").clone()),
                    .DialogFooter::default { flex_shrink_0; px_6; pb_4; } [
                        DialogCommand::Cancel.DialogDecision::new { label: tr!("action-close"); },
                    ],
                ]
            }
            .into_any_element(),
            InfoWindow::Confirmation => self.message(cx).into_any_element(),
        };
        el! {
            .div {
                # id;
                test_support;
                size_full;
                flex;
                flex_col;
                bg: $(gds::appearance::theme::content_background(cx));
                text_color: $(cx.theme().foreground);
                @if (matches!(self.kind, InfoWindow::About))
                    key_context: DIALOG_CONTEXT;
                    on_action: (cx, Self::on_about_cancel).Context::listener;
                @end;
            } [
                .TitleBar::default {
                    on_close_window: Self::on_close;
                } [
                    $(self.title.clone()),
                ],
                content,
            ]
        }
    }
}
