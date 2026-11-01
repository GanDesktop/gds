//! Passive session indicators shared by the conversation sidebar and history browser.

use crate::shared::i18n::tr;
use bas::model::{SessionMeta, SessionSnapshot, SessionStatus};
use gds_uim::el;
use gpui_kit::{
    AnyElement, App, SharedString, TestSupportExt,
    assets::IconName,
    component::{ActiveTheme, Icon, Sizable, spinner::Spinner},
    div,
    prelude::*,
    px,
};

const STATUS_SIZE: f32 = crate::shared::ui::SMALL_ICON;
const LIGHT_SIZE: f32 = 6.;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Status {
    Starting,
    Working,
    Unread,
    Idle,
    Failed,
    Stopped,
}

impl From<SessionStatus> for Status {
    fn from(status: SessionStatus) -> Self {
        match status {
            SessionStatus::Starting => Self::Starting,
            SessionStatus::Working => Self::Working,
            SessionStatus::Idle => Self::Idle,
            SessionStatus::Failed => Self::Failed,
            SessionStatus::Stopped => Self::Stopped,
        }
    }
}

impl Status {
    pub(crate) fn for_session(session: &SessionMeta, seen: u64, active_unread: bool) -> Self {
        match Self::from(session.status) {
            Self::Idle if session.last_reply.raw() > seen || active_unread => Self::Unread,
            status => status,
        }
    }

    pub(crate) fn for_snapshot(session: &SessionSnapshot) -> Self {
        session.status().into()
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Starting => tr!("status-starting"),
            Self::Working => tr!("status-running"),
            Self::Unread => tr!("status-unread"),
            Self::Idle => tr!("status-ready"),
            Self::Failed => tr!("status-failed"),
            Self::Stopped => tr!("status-stopped"),
        }
    }

    fn indicator(self, id: SharedString, cx: &App) -> AnyElement {
        let indicator = el! {
            .div {
                # id;
                test_support;
                flex;
                items_center;
                justify_center;
                flex_shrink_0;
            }
        };
        match self {
            Self::Starting | Self::Working => el! {
                indicator {
                    size: STATUS_SIZE.px;
                } [
                    .Spinner::new {
                        icon: IconName::LoaderCircle;
                        with_size: STATUS_SIZE.px;
                        color: $(cx.theme().primary);
                    },
                ]
            },
            Self::Idle => el! {
                indicator {
                    size_0;
                }
            },
            Self::Unread | Self::Stopped => el! {
                indicator {
                    size: LIGHT_SIZE.px;
                    rounded_full;
                    bg: $(match self {
                            Self::Stopped => cx.theme().muted_foreground,
                            _ => cx.theme().primary,
                    });
                }
            },
            Self::Failed => el! {
                indicator {
                    size: STATUS_SIZE.px;
                } [
                    (IconName::TriangleAlert).Icon::new {
                        with_size: STATUS_SIZE.px;
                        text_color: $(cx.theme().danger);
                    },
                ]
            },
        }
        .into_any_element()
    }

    pub(crate) fn render(
        self,
        id: impl Into<SharedString>,
        indicator_id: impl Into<SharedString>,
        cx: &App,
    ) -> AnyElement {
        let id: SharedString = id.into();
        let slot = el! {
            .div {
                size: crate::shared::ui::COMPACT_CONTROL.px;
                flex_shrink_0;
                flex;
                items_center;
                justify_center;
            } [
                $(self.indicator(indicator_id.into(), cx)),
            ]
        };
        if self == Self::Idle {
            return slot.into_any_element();
        }
        el! {
            slot {
                # id;
                test_support;
                cursor_default;
                role: gpui_kit::Role::Status;
                aria_label: $(self.label());
                hoverable_tooltip: $(move |window, cx| crate::shared::ui::tooltip::view(self.label(), window, cx));
            }
        }
        .into_any_element()
    }

    /// The same state vocabulary in an avatar's corner, without a full action slot.
    pub(crate) fn badge(self, id: impl Into<SharedString>, cx: &App) -> Option<AnyElement> {
        if self == Self::Idle {
            return None;
        }
        let id = id.into();
        Some(
            el! {
                .div {
                    # $(id.clone());
                    test_support;
                    role: gpui_kit::Role::Status;
                    aria_label: $(self.label());
                    size: STATUS_SIZE.px;
                    flex;
                    items_center;
                    justify_center;
                } [$(self.indicator(format!("{id}-indicator").into(), cx))]
            }
            .into_any_element(),
        )
    }
}

pub(crate) fn archived(id: impl Into<SharedString>) -> AnyElement {
    let id: SharedString = id.into();
    let label = tr!("session-archived");
    el! {
        .div {
            # id;
            test_support;
            role: gpui_kit::Role::Status;
            aria_label: label;
            size: crate::shared::ui::COMPACT_CONTROL.px;
            flex;
            items_center;
            justify_center;
            flex_shrink_0;
            cursor_default;
            hoverable_tooltip: $(move |window, cx| crate::shared::ui::tooltip::view(label, window, cx));
        } [
            (IconName::Archive).Icon::new {
                size: STATUS_SIZE.px;
            },
        ]
    }
    .into_any_element()
}

#[cfg(test)]
mod tests;
