use gpui_kit::{
    App, Context, Div, Empty, IntoElement, Render, Stateful, Styled as _, Window,
    base::{Root, RootPlugin},
    transparent_black,
};

/// Window transparency belongs to the root surface, not the theme's background role.
pub(crate) struct Material;

impl Material {
    pub(crate) fn install(cx: &mut App) {
        Root::register_plugin::<Self>(cx, |_, _| Self);
    }
}

impl RootPlugin for Material {
    fn style(&self, surface: &mut Stateful<Div>, _: &mut Window, _: &mut App) {
        surface.style().background = Some(transparent_black().into());
    }
}

impl Render for Material {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}
