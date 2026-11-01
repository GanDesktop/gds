// Forward container behavior to the rendered div without adding a layout node.
macro_rules! container_traits {
    ($component:ident) => {
        impl $component {
            pub fn id(mut self, id: impl Into<gpui_kit::ElementId>) -> Self {
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

pub(crate) use container_traits;
