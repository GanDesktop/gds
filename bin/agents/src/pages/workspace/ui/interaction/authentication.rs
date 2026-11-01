use super::super::*;
use crate::shared::i18n::tr;
use gds_uim::el;

impl Workspace {
    fn on_workspace_auth_open(
        &mut self,
        _: &gpui_kit::ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_agent_auth(window, cx);
    }

    fn on_workspace_auth_changed(
        &mut self,
        _: Entity<crate::features::agent_auth::Authentication>,
        cx: &mut Context<Self>,
    ) {
        cx.notify();
    }

    pub(in crate::pages::workspace) fn authentication_button(
        &self,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        crate::shared::ui::tooltip::bounded(
            "authentication-hint",
            el! {
                ("agent-authenticate").Button::new {
                    xsmall;
                    label: tr!("auth-action");
                    on_click: $(cx.listener(Self::on_workspace_auth_open));
                }
            },
            tr!("auth-action"),
        )
        .into_any_element()
    }

    pub(in crate::pages::workspace) fn authentication_hint(&self) -> Option<String> {
        self.selected.authentication.required.as_ref().map(|agent| {
            if self.backend.server.is_local() {
                tr!("auth-local-help", agent = (agent).to_string())
            } else {
                tr!(
                    "auth-remote-help",
                    agent = (agent).to_string(),
                    server = (self.backend.server).to_string()
                )
            }
        })
    }

    pub(in crate::pages::workspace) fn open_agent_auth(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(agent) = self.selected.authentication.required.clone() else {
            return;
        };
        self.close_reply_preview(window, cx);
        self.close_menu(window, cx);
        self.record_settings_target(cx);
        if cx
            .try_global::<crate::entities::agent::check::Service>()
            .is_none()
        {
            let owner = cx.entity().downgrade();
            let parent = self.window.handle;
            cx.defer(move |cx| {
                if !crate::features::navigation::target_page(parent, cx) {
                    return;
                }
                if let Err(error) = crate::widgets::settings::window::open_agent(&agent, cx) {
                    let _ = owner.update(cx, |this, cx| {
                        this.selected.error =
                            Some(tr!("auth-instructions-error", error = error.to_string()));
                        cx.notify();
                    });
                }
            });
        } else {
            self.load_authentication_panel(window, cx);
        }
        cx.notify();
    }

    pub(in crate::pages::workspace) fn load_authentication_panel(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(agent) = self.selected.authentication.required.clone() else {
            return;
        };
        if self.selected.authentication.panel.is_some()
            || self.selected.authentication.load.is_some()
        {
            return;
        }
        let Some(runtime) = cx
            .try_global::<crate::entities::agent::catalog::Service>()
            .map(|service| service.runtime().clone())
        else {
            return;
        };
        let catalog = crate::entities::agent::catalog::Service::new(
            self.backend.server.clone(),
            runtime.clone(),
        );
        let service =
            crate::entities::agent::check::Service::new(self.backend.server.clone(), runtime);
        let path = crate::shared::config::store(cx).path().to_owned();
        let epoch = self.selected.epoch;
        self.window.output_open = true;
        self.selected.authentication.load = Some(cx.spawn_in(window, async move |view, cx| {
            let result = async {
                let catalog = catalog.catalog(false).await?;
                let spec = catalog
                    .entries
                    .into_iter()
                    .find(|entry| entry.id == agent)
                    .and_then(|entry| entry.spec)
                    .ok_or_else(|| tr!("agent-catalog-missing").to_owned())?;
                let status = service.request(spec.clone(), true, path.clone()).await;
                Ok::<_, String>((spec, status))
            }
            .await;
            let _ = view.update_in(cx, |this, window, cx| {
                if this.selected.epoch != epoch
                    || this.selected.id.is_some()
                    || this.selected.authentication.required.as_ref() != Some(&agent)
                {
                    return;
                }
                this.selected.authentication.load = None;
                match result {
                    Err(error) => this.selected.error = Some(error),
                    Ok((_, status)) if status.connected() => {
                        this.refresh(cx);
                    }
                    Ok((spec, status)) => {
                        let panel = cx.new(|_| {
                            crate::features::agent_auth::Authentication::new(
                                spec,
                                status,
                                path,
                                window.window_handle(),
                                Some(service.clone()),
                            )
                        });
                        let expected = agent.clone();
                        let completed = cx.subscribe_in(
                            &panel,
                            window,
                            move |this,
                                  _,
                                  event: &crate::features::agent_auth::Completed,
                                  _,
                                  cx| {
                                if event.0.connected()
                                    && this.selected.epoch == epoch
                                    && this.selected.id.is_none()
                                    && this.selected.authentication.required.as_ref()
                                        == Some(&expected)
                                {
                                    this.selected.authentication.panel = None;
                                    this.refresh(cx);
                                }
                                cx.notify();
                            },
                        );
                        let changed = cx.observe(&panel, Self::on_workspace_auth_changed);
                        this.selected.authentication.panel =
                            Some(state::ObservedPanel::new(panel, vec![completed, changed]));
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
}
