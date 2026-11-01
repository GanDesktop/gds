/// Translate a message immediately with `t!(@cx, "message-id")`, or return a
/// closure accepting `&App` with `t!("message-id")`.
///
/// Arguments are Fluent variables. A bare identifier is shorthand for a
/// variable with the same name; `name = expression` supplies an explicit value.
/// Both forms can be mixed and accept a trailing comma.
///
/// ```ignore
/// use gds::i18n::t;
///
/// t!(@cx, "tabs");
/// t!(@cx, "items-count", count);
/// t!(@cx, "summary", count, name = user.name.as_str());
///
/// // UIM reborrows App or Context references through the factory's signature.
/// el! { cx.t!("tabs").SideBarSectionTitle::new }
/// ```
#[macro_export]
macro_rules! t {
    (@$cx:expr, $id:literal $(,)?) => {
        $crate::i18n::text($id, $cx)
    };
    (@$cx:expr, $id:literal, $($name:ident $(= $value:expr)?),+ $(,)?) => {{
        let cx: &_ = $cx;
        let mut arguments = $crate::i18n::FluentArgs::new();
        $(
            arguments.set(
                ::core::stringify!($name),
                $crate::t!(@argument $name $(= $value)?),
            );
        )+
        $crate::i18n::format($id, ::core::option::Option::Some(&arguments), cx)
    }};
    ($id:literal $(, $name:ident $(= $value:expr)?)* $(,)?) => {
        $crate::i18n::deferred(|cx| {
            $crate::t!(@cx, $id $(, $name $(= $value)?)* )
        })
    };
    (@argument $name:ident = $value:expr) => {
        $value
    };
    (@argument $name:ident) => {
        $name
    };
}
