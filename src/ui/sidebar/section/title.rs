use gds_uim::{component, el};
use gpui_kit::component::{ActiveTheme, StyledExt};
use gpui_kit::{prelude::*, *};

/// Shared typography for sidebar section labels, following the rem base.
#[component]
#[derive(IntoElement)]
pub struct SideBarSectionTitle {
    #[attr(into)]
    label: SharedString,
    base: Div,
    style: StyleRefinement,
}

impl SideBarSectionTitle {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            base: div(),
            style: StyleRefinement::default(),
        }
    }

    /// Assign identity to the inner div while preserving the component's type.
    pub fn id(mut self, id: impl Into<ElementId>) -> Self {
        self.base.interactivity().element_id = Some(id.into());
        self
    }
}

impl Styled for SideBarSectionTitle {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl InteractiveElement for SideBarSectionTitle {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.base.interactivity()
    }
}

impl StatefulInteractiveElement for SideBarSectionTitle {}

impl RenderOnce for SideBarSectionTitle {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        el! {
            $(self.base) {
                min_w_0;
                text_xs;
                font_weight: FontWeight::NORMAL;
                text_color: $(cx.theme().muted_foreground);
                truncate;
                refine_style: $&self.style;
            } [$self.label]
        }
    }
}
