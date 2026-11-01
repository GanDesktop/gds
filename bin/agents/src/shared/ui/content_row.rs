//! Composable content rows local to Agents. Supplied controls own interaction.

mod actions;
mod content;
mod description;
mod leading;
mod title;

pub(crate) use actions::ContentRowActions;
pub(crate) use content::ContentRowContent;
pub(crate) use description::ContentRowDescription;
pub(crate) use leading::ContentRowLeading;
pub(crate) use title::ContentRowTitle;

use gds_uim::el;
use gpui_kit::{prelude::*, *};

// Forward container behavior to the rendered div without adding a layout node.
macro_rules! row_traits {
    ($component:ident) => {
        impl $component {
            pub(crate) fn id(mut self, id: impl Into<gpui_kit::ElementId>) -> Self {
                self.base.interactivity().element_id = Some(id.into());
                self
            }
        }

        impl gpui_kit::Styled for $component {
            fn style(&mut self) -> &mut gpui_kit::StyleRefinement {
                self.base.style()
            }
        }

        impl gpui_kit::ParentElement for $component {
            fn extend(&mut self, elements: impl IntoIterator<Item = gpui_kit::AnyElement>) {
                self.base.extend(elements);
            }
        }

        impl gpui_kit::InteractiveElement for $component {
            fn interactivity(&mut self) -> &mut gpui_kit::Interactivity {
                self.base.interactivity()
            }
        }

        impl gpui_kit::StatefulInteractiveElement for $component {}
    };
}

pub(super) use row_traits;

/// A passive row containing leading content, a body and independent actions.
#[derive(IntoElement)]
pub(crate) struct ContentRow {
    base: Div,
}

impl Default for ContentRow {
    fn default() -> Self {
        Self {
            base: el! { .div { w_full; min_w_0; flex; items_center; gap_2; } },
        }
    }
}

row_traits!(ContentRow);

impl RenderOnce for ContentRow {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        self.base
    }
}
