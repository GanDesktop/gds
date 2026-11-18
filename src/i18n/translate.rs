/// Translate with the current catalogue; explicit `@context` selects an application context.
#[macro_export]
macro_rules! t {
    (@$cx:expr, $id:literal $(,)?) => { $cx.i18n().text($id) };
    (@$cx:expr, $id:literal, $($name:ident $(= $value:expr)?),+ $(,)?) => {{
        let mut arguments = $crate::i18n::FluentArgs::new();
        $(arguments.set(::core::stringify!($name), $crate::t!(@argument $name $(= $value)?));)+
        $cx.i18n().format($id, Some(&arguments))
    }};
    ($id:literal $(, $name:ident $(= $value:expr)?)* $(,)?) => {
        $crate::t!(@$crate::app::Gds::current(), $id $(, $name $(= $value)?)* )
    };
    (@argument $name:ident = $value:expr) => { $value };
    (@argument $name:ident) => { $name };
}
