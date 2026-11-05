use super::*;
use bas::service::HistoryOverviewEntry;
use gds_uim::el;
use gpui_kit::component::menu::PopupMenuItem;

impl Workspace {
    pub(super) fn render_unloaded_markers(
        &self,
        session: SessionId,
        entries: &[HistoryOverviewEntry],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let first = entries.first().expect("nonempty history group").cursor;
        let last = entries.last().expect("nonempty history group").cursor;
        let range = first..=last;
        let active = self
            .active_marker()
            .is_some_and(|cursor| range.contains(&cursor));
        let bookmarks = self
            .navigation
            .marker_bookmarks
            .get(&session)
            .cloned()
            .unwrap_or_default();
        let marked = bookmarks.iter().any(|cursor| range.contains(cursor));
        let hint = gds::i18n::count("turn-marker-unloaded", entries.len(), cx);
        let owner = cx.entity().downgrade();
        let epoch = self.selected.epoch;
        let button = el! {
            (format!("turn-unloaded-{first}")).Button::new {
                ghost;
                small;
                w: RAIL_WIDTH.px;
                h: 12.0.px;
                p_0;
                flex_shrink_0;
                icon: $(Icon::new(IconName::Ellipsis).size(px(10.)));
                accessibility_label: $(hint.clone());
                tooltip: hint;
                text_color: $(if active || marked { cx.theme().primary } else { cx.theme().muted_foreground });
            }
        };
        self.menu_button(
            button,
            entries.len(),
            false,
            move |mut menu, _, cx| {
                let Some(view) = owner.upgrade() else {
                    return menu;
                };
                let view = view.read(cx);
                if view.selected.id != Some(session) || view.selected.epoch != epoch {
                    return menu;
                }
                let Some(overview) = &view.selected.timeline.navigation.overview else {
                    return menu;
                };
                for entry in overview
                    .entries
                    .iter()
                    .filter(|entry| range.contains(&entry.cursor))
                {
                    let cursor = entry.cursor;
                    let preview = entry.preview.clone();
                    let owner = owner.clone();
                    menu = menu.item(
                        PopupMenuItem::element(move |_, _| {
                            el! {
                                .div {
                                    # format!("turn-unloaded-choice-{cursor}");
                                    test_support;
                                    role: gpui_kit::Role::Label;
                                    aria_label: $(preview.clone());
                                    flex_1;
                                    min_w_0;
                                    truncate;
                                } [$(preview.clone())]
                            }
                        })
                        .checked(bookmarks.contains(&cursor))
                        .on_click(move |event, _, cx| {
                            let _ = owner.update(cx, |this, cx| {
                                if this.selected.id != Some(session) || this.selected.epoch != epoch
                                {
                                    return;
                                }
                                if event.modifiers().control {
                                    this.toggle_marker_bookmark(session, cursor, cx);
                                } else {
                                    this.navigate_to_marker(cursor, cx);
                                }
                            });
                        }),
                    );
                }
                menu
            },
            cx,
        )
    }
}
