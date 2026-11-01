//! A saved configuration and the outcome of starting it are separate results.

use super::*;
use crate::shared::i18n::tr;
use gds_uim::el;
use gpui_kit::assets::IconName;

impl Agents {
    pub(in crate::widgets::settings) fn save_status(&self) -> Option<String> {
        if self.editor.spec().is_some() {
            return None;
        }
        let spec = self
            .saved
            .as_ref()
            .filter(|saved| self.display_catalog().iter().any(|spec| spec == *saved))?;
        let status = self.status(spec);
        Some(match status {
            Some(Status::Checking | Status::Connecting) => {
                tr!("agent-saved-connecting", agent = spec.id.to_string())
            }
            Some(Status::Failed(_) | Status::Missing(_)) => {
                tr!("agent-saved-unavailable", agent = spec.id.to_string())
            }
            Some(Status::AuthenticationRequired { .. } | Status::Authenticating) => {
                tr!("agent-saved-signin", agent = spec.id.to_string())
            }
            _ => tr!("agent-saved", agent = spec.id.to_string()),
        })
    }

    pub(super) fn saved_feedback(&self, cx: &mut Context<Self>) -> Option<gpui_kit::AnyElement> {
        let label = self.save_status()?;
        let spec = self.saved.as_ref().expect("validated saved configuration");
        let status = self.status(spec);
        let error = match status {
            Some(Status::Failed(error) | Status::Missing(error))
                if !self.authentications.contains_key(&spec.id) =>
            {
                Some(error.clone())
            }
            _ => None,
        };
        let id = spec.id.to_string();
        let manage_id = id.clone();
        let target = self.target();
        let owner = cx.entity().downgrade();
        Some(el! {
            .div {
                # "agents-save-feedback";
                test_support;
                flex;
                flex_col;
                gap_2;
                p_3;
                border_1;
                border_color: $(cx.theme().border);
                rounded_md;
            } [
                $(crate::widgets::settings::ui::feedback("agents-save-result", label, cx.theme().foreground)),
                ..$(error.map(|error| crate::widgets::settings::ui::feedback("agents-save-error", error, cx.theme().danger))),
                .div {
                    flex;
                    items_center;
                    gap_2;
                } [
                    ("agents-saved-chat").Button::new {
                        small;
                        icon: IconName::MessageCircle;
                        label: tr!("agent-start-chat");
                        disabled: $(!status.is_some_and(Status::connected) || !self.registry.editable() || self.logging_out(spec));
                        on_click: $(cx.listener(move |this, _, _, cx| this.begin_chat(id.clone(), cx)));
                    },
                    ("agents-saved-manage").Button::new {
                        small;
                        icon: IconName::Settings;
                        label: tr!("agent-manage");
                        on_click: $(move |_, _, cx| {
                                let id = manage_id.clone();
                                let target = target.clone();
                                let owner = owner.clone();
                                cx.defer(move |cx| {
                                        if let Err(error) = crate::widgets::settings::ui::window::open_agent_management(&id, target, cx) {
                                            let _ = owner.update(cx, |page, cx| { page.message = error.to_string(); cx.notify(); });
                                        }
                                });
                        });
                    },
                ],
            ]
        }.into_any_element())
    }
}
