pub(crate) trait FilesExt {
    type Context;
    fn files(self) -> Files<Self::Context>;
}

impl<C> FilesExt for gds::app::Gds<C> {
    type Context = C;
    fn files(self) -> Files<C> {
        Files {
            cx: self.into_inner(),
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Files<C> {
    cx: C,
}

impl<C> Files<C> {
    pub(crate) fn into_inner(self) -> C {
        self.cx
    }
}

impl Files<&mut gpui_kit::App> {
    pub(crate) fn app(&mut self) -> &mut gpui_kit::App {
        self.cx
    }
}
