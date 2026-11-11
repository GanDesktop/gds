//! Tab drags are local to one workspace and never carry filesystem operations.
use crate::pages::workspace::ui::*;
use gds_uim::el;
use gpui_kit::component::{ActiveTheme, Icon};
use std::{cell::Cell, rc::Rc};

#[derive(Clone)]
struct TabDrag {
    owner: EntityId,
    tab: u64,
    pinned: bool,
    title: SharedString,
    icon: gpui_kit::assets::IconName,
}

impl TabDrag {
    fn accepts(&self, owner: EntityId, tab: u64, pinned: bool) -> bool {
        self.owner == owner && self.tab != tab && self.pinned == pinned
    }
}

impl Browser {
    pub(in crate::pages::workspace) fn draggable_tab(
        &self,
        element: Stateful<Div>,
        slot: &OpenTab,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let owner = cx.entity_id();
        let id = slot.id;
        let pinned = slot.pinned;
        let drag = TabDrag {
            owner,
            tab: id,
            pinned,
            title: self.tab_title(slot, cx).into(),
            icon: slot.location().icon(),
        };
        let bounds = Rc::new(Cell::new(Bounds::default()));
        let measured = bounds.clone();
        let target = bounds.clone();
        let measure = canvas(move |bounds, _, _| measured.set(bounds), |_, (), _, _| {})
            .absolute()
            .inset_0();
        el! { element [measure] }
            .on_drag(drag, |drag, _, _, cx| cx.new(|_| TabPreview(drag.clone())))
            .drag_over::<TabDrag>(move |style, drag, window, cx| {
                if !drag.accepts(owner, id, pinned) {
                    return style;
                }
                let style = style.border_color(cx.theme().drag_border);
                if window.mouse_position().y < target.get().center().y {
                    style.border_t_2()
                } else {
                    style.border_b_2()
                }
            })
            .on_drop(cx.listener(move |this, drag: &TabDrag, window, cx| {
                if !drag.accepts(owner, id, pinned) {
                    return;
                }
                let after = window.mouse_position().y >= bounds.get().center().y;
                if this.tabs.move_tab(drag.tab, id, after) {
                    if pinned {
                        let paths = this
                            .tabs
                            .items
                            .iter()
                            .filter(|tab| tab.pinned)
                            .map(OpenTab::location)
                            .collect();
                        this.locations.places.reorder_pinned(paths);
                        this.sync_pinned();
                        this.save_places(cx);
                    }
                    cx.notify();
                }
            }))
    }
}

struct TabPreview(TabDrag);
impl Render for TabPreview {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        el! {
            .div { px_3; py_2; max_w: 240.0.px; rounded_md; flex; items_center; gap_2;
                bg: $(cx.theme().popover); text_color: $(cx.theme().popover_foreground);
                font_family: $(cx.theme().font_family.clone()); text_sm;
                border_1; border_color: $(cx.theme().border);
            } [
                $(self.0.icon).Icon::new { size: 16.0.px; },
                .div { truncate } [$(self.0.title.clone())],
            ]
        }
    }
}
