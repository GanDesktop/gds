//! Container behavior delegates to the anatomical root without adding nodes.
macro_rules! container_traits {
    ($component:ident) => {
        impl $component {
            pub fn child(mut self, child: impl anti_webui::prelude::IntoView) -> Self {
                self.base = self.base.child(child);
                self
            }
            pub fn children<V: anti_webui::prelude::IntoView>(
                mut self,
                children: impl IntoIterator<Item = V>,
            ) -> Self {
                self.base = self.base.children(children);
                self
            }
            pub fn on_click(
                mut self,
                handler: impl Fn(&anti_webui::prelude::PointerEvent) + 'static,
            ) -> Self {
                self.base = self.base.on_click(handler);
                self
            }
            pub fn on_aux_click(
                mut self,
                handler: impl Fn(&anti_webui::prelude::PointerEvent) + 'static,
            ) -> Self {
                self.base = self.base.on_aux_click(handler);
                self
            }
            pub fn on_resized(mut self, handler: impl Fn(f32, f32) + 'static) -> Self {
                self.base = self.base.on_resized(handler);
                self
            }
            pub fn on_key_down(
                mut self,
                handler: impl Fn(&anti_webui::prelude::KeyboardEvent) + 'static,
            ) -> Self {
                self.base = self.base.on_key_down(handler);
                self
            }
            pub fn node_ref(mut self, node: anti_webui::prelude::NodeRef) -> Self {
                self.base = self.base.node_ref(node);
                self
            }
            pub fn dyn_style(
                mut self,
                style: impl Fn(anti_webui::prelude::Style) -> anti_webui::prelude::Style + 'static,
            ) -> Self {
                self.base = self.base.dyn_style(style);
                self
            }
            pub fn focusable(mut self) -> Self {
                self.base = self.base.focusable();
                self
            }
        }
        impl anti_webui::prelude::Styled for $component {
            fn style_mut(&mut self) -> &mut anti_webui::prelude::Style {
                self.base.style_mut()
            }
        }
        impl anti_webui::prelude::IntoView for $component {
            fn into_view(self) -> anti_webui::prelude::View {
                self.base.into_view()
            }
        }
    };
}
pub(crate) use container_traits;
