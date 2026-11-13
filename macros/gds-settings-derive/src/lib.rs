//! Derive typed field bindings for the shared native settings editor.

use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as Tokens};
use quote::{format_ident, quote};
use syn::{
    Attribute, Data, DeriveInput, Expr, Fields, LitStr, Type, parse_macro_input, spanned::Spanned,
};

#[proc_macro_derive(ConfigEditor, attributes(config, setting))]
pub fn derive_editor(input: TokenStream) -> TokenStream {
    expand(parse_macro_input!(input as DeriveInput))
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

#[derive(Default)]
struct Options {
    label: Option<(LitStr, Span)>,
    help: Option<(LitStr, Span)>,
    group: Option<(LitStr, Span)>,
    id: Option<(LitStr, Span)>,
    kind: Option<(LitStr, Span)>,
    skip: Option<Span>,
    min: Option<(Expr, Span)>,
    max: Option<(Expr, Span)>,
    step: Option<(Expr, Span)>,
    scale: Option<(Expr, Span)>,
    default: Option<(Expr, Span)>,
    unit: Option<(LitStr, Span)>,
}

fn set_option<T>(slot: &mut Option<(T, Span)>, value: T, span: Span) -> syn::Result<()> {
    if slot.is_some() {
        return Err(syn::Error::new(span, "duplicate settings UI annotation"));
    }
    *slot = Some((value, span));
    Ok(())
}

impl Options {
    fn parse(attrs: &[Attribute]) -> syn::Result<Self> {
        let mut result = Self::default();
        for attr in attrs.iter().filter(|a| a.path().is_ident("setting")) {
            attr.parse_nested_meta(|meta| {
                let span = meta.path.span();
                let key = meta
                    .path
                    .get_ident()
                    .map(ToString::to_string)
                    .unwrap_or_default();
                match key.as_str() {
                    "skip" => {
                        if result.skip.is_some() {
                            return Err(syn::Error::new(span, "duplicate settings UI annotation"));
                        }
                        result.skip = Some(span);
                    }
                    "label" => set_option(&mut result.label, meta.value()?.parse()?, span)?,
                    "help" => set_option(&mut result.help, meta.value()?.parse()?, span)?,
                    "group" => set_option(&mut result.group, meta.value()?.parse()?, span)?,
                    "id" => set_option(&mut result.id, meta.value()?.parse()?, span)?,
                    "kind" => set_option(&mut result.kind, meta.value()?.parse()?, span)?,
                    "min" => set_option(&mut result.min, meta.value()?.parse()?, span)?,
                    "max" => set_option(&mut result.max, meta.value()?.parse()?, span)?,
                    "step" => set_option(&mut result.step, meta.value()?.parse()?, span)?,
                    "scale" => set_option(&mut result.scale, meta.value()?.parse()?, span)?,
                    "default" => set_option(&mut result.default, meta.value()?.parse()?, span)?,
                    "unit" => set_option(&mut result.unit, meta.value()?.parse()?, span)?,
                    _ => return Err(meta.error("unknown settings UI annotation")),
                }
                Ok(())
            })?;
        }
        Ok(result)
    }

    fn first_numeric_span(&self) -> Option<Span> {
        [&self.min, &self.max, &self.step, &self.scale]
            .into_iter()
            .flatten()
            .map(|(_, span)| *span)
            .next()
            .or_else(|| self.unit.as_ref().map(|(_, span)| *span))
    }

    fn first_variant_disallowed_span(&self) -> Option<Span> {
        [&self.help, &self.group, &self.id, &self.kind]
            .into_iter()
            .flatten()
            .map(|(_, span)| *span)
            .next()
            .or_else(|| self.default.as_ref().map(|(_, span)| *span))
            .or_else(|| self.first_numeric_span())
    }

    fn first_non_skip_span(&self) -> Option<Span> {
        self.label
            .as_ref()
            .map(|(_, span)| *span)
            .or_else(|| self.first_variant_disallowed_span())
    }
}

fn inner_type(ty: &Type) -> syn::Result<(&Type, bool, String)> {
    let Type::Path(path) = ty else {
        return Err(syn::Error::new(
            ty.span(),
            "expected an owned configuration field type",
        ));
    };
    let segment = path.path.segments.last().expect("type path has a segment");
    if segment.ident == "Option" {
        if let syn::PathArguments::AngleBracketed(arguments) = &segment.arguments
            && let Some(syn::GenericArgument::Type(inner)) = arguments.args.first()
        {
            let (_, _, name) = inner_type(inner)?;
            return Ok((inner, true, name));
        }
        return Err(syn::Error::new(ty.span(), "expected Option<T>"));
    }
    Ok((ty, false, segment.ident.to_string()))
}

fn expand(input: DeriveInput) -> syn::Result<Tokens> {
    if !input.generics.params.is_empty() {
        return Err(syn::Error::new(
            input.generics.span(),
            "derive ConfigEditor on a concrete configuration type",
        ));
    }
    let name = &input.ident;
    let visibility = &input.vis;
    let mut root: syn::Path = syn::parse_quote!(::gds);
    let mut validator: Option<syn::Path> = None;
    for attr in input.attrs.iter().filter(|a| a.path().is_ident("config")) {
        attr.parse_nested_meta(|meta| {
            let value: LitStr = meta.value()?.parse()?;
            if meta.path.is_ident("validate") {
                validator = Some(value.parse()?);
            } else if meta.path.is_ident("crate") {
                root = value.parse()?;
            } else {
                return Err(meta.error("expected validate or crate"));
            }
            Ok(())
        })?;
    }
    let api = quote!(#root::ui::settings::form);
    match &input.data {
        Data::Struct(data) => {
            let Fields::Named(fields) = &data.fields else {
                return Err(syn::Error::new(
                    data.fields.span(),
                    "ConfigEditor requires named configuration fields",
                ));
            };
            let mut descriptors = Vec::new();
            let mut identities = std::collections::HashSet::new();
            for field in &fields.named {
                if !field.attrs.iter().any(|a| a.path().is_ident("setting")) {
                    continue;
                }
                let options = Options::parse(&field.attrs)?;
                if options.skip.is_some() {
                    if let Some(other) = options.first_non_skip_span() {
                        return Err(syn::Error::new(
                            other,
                            "#[setting(skip)] cannot be combined with other settings UI annotations",
                        ));
                    }
                    continue;
                }
                let member = field.ident.as_ref().expect("named field");
                let ty = &field.ty;
                let (inner, nullable, type_name) = inner_type(ty)?;
                let (label, _) = options.label.as_ref().ok_or_else(|| {
                    syn::Error::new(
                        field.span(),
                        "editable fields need #[setting(label = \"message-id\")]",
                    )
                })?;
                let id = options
                    .id
                    .as_ref()
                    .map(|(lit, _)| lit.clone())
                    .unwrap_or_else(|| LitStr::new(&member.to_string(), member.span()));
                if !identities.insert(id.value()) {
                    return Err(syn::Error::new(id.span(), "duplicate settings field id"));
                }
                let help = options
                    .help
                    .as_ref()
                    .map(|(lit, _)| lit.clone())
                    .unwrap_or_else(|| LitStr::new("", member.span()));
                let group = options
                    .group
                    .as_ref()
                    .map(|(lit, _)| lit.clone())
                    .unwrap_or_else(|| LitStr::new("", member.span()));
                let (is_number, kind) = match options
                    .kind
                    .as_ref()
                    .map(|(lit, span)| (lit.value(), *span))
                {
                    Some((kind, span)) => match kind.as_str() {
                        "font" => (false, quote!(#api::Kind::Font)),
                        "language" => (false, quote!(#api::Kind::Language)),
                        "text" => (false, quote!(#api::Kind::Text)),
                        "string_list" => (false, quote!(#api::Kind::StringList)),
                        _ => {
                            return Err(syn::Error::new(
                                span,
                                "kind must be font, language, text or string_list; primitive types and enums are inferred",
                            ));
                        }
                    },
                    None => match type_name.as_str() {
                        "bool" => (false, quote!(#api::Kind::Toggle)),
                        "String" | "PathBuf" => (false, quote!(#api::Kind::Text)),
                        "Language" => (false, quote!(#api::Kind::Language)),
                        "f32" | "f64" | "i8" | "i16" | "i32" | "i64" | "isize" | "u8" | "u16"
                        | "u32" | "u64" | "usize" => {
                            let min = options
                                .min
                                .as_ref()
                                .map(|(expr, _)| expr.clone())
                                .unwrap_or_else(|| syn::parse_quote!(0.0));
                            let max = options
                                .max
                                .as_ref()
                                .map(|(expr, _)| expr.clone())
                                .unwrap_or_else(|| syn::parse_quote!(100.0));
                            let step = options
                                .step
                                .as_ref()
                                .map(|(expr, _)| expr.clone())
                                .unwrap_or_else(|| syn::parse_quote!(1.0));
                            let scale = options
                                .scale
                                .as_ref()
                                .map(|(expr, _)| expr.clone())
                                .unwrap_or_else(|| syn::parse_quote!(1.0));
                            let unit = options
                                .unit
                                .as_ref()
                                .map(|(lit, _)| lit.clone())
                                .unwrap_or_else(|| LitStr::new("", member.span()));
                            let integer = match type_name.as_str() {
                                "i8" => quote!(#api::Integer::I8),
                                "i16" => quote!(#api::Integer::I16),
                                "i32" => quote!(#api::Integer::I32),
                                "i64" => quote!(#api::Integer::I64),
                                "isize" => quote!(#api::Integer::Isize),
                                "u8" => quote!(#api::Integer::U8),
                                "u16" => quote!(#api::Integer::U16),
                                "u32" => quote!(#api::Integer::U32),
                                "u64" => quote!(#api::Integer::U64),
                                "usize" => quote!(#api::Integer::Usize),
                                _ => quote!(#api::Integer::Float),
                            };
                            (
                                true,
                                quote!(#api::Kind::Number(#api::Number {
                                    min: (#min) as f64, max: (#max) as f64, step: (#step) as f64,
                                    scale: (#scale) as f64, unit: #unit, integer: #integer,
                                })),
                            )
                        }
                        "Vec" | "HashMap" | "BTreeMap" | "i128" | "u128" => {
                            return Err(syn::Error::new(
                                field.span(),
                                "this field needs a custom editor; leave it unannotated or use #[setting(skip)]",
                            ));
                        }
                        _ => (
                            false,
                            quote!(#api::Kind::Choice(<#inner as #api::Choices>::choices)),
                        ),
                    },
                };
                if !is_number && let Some(numeric) = options.first_numeric_span() {
                    return Err(syn::Error::new(
                        numeric,
                        "numeric settings annotations (min, max, step, scale, unit) only apply to numeric fields",
                    ));
                }
                let default = match options.default.as_ref() {
                    Some((value, _)) => quote!(Some(|| #api::encode(&(#value)))),
                    None => quote!(None),
                };
                descriptors.push(quote! {
                    #api::Field {
                        id: #id, label: #label, help: #help, group: #group, nullable: #nullable,
                        kind: #kind, default: #default,
                        get: |model: &Self| #api::encode(&model.#member),
                        set: |model: &mut Self, value| { model.#member = #api::decode::<#ty>(value)?; Ok(()) },
                    }
                });
            }
            if descriptors.is_empty() {
                return Err(syn::Error::new(
                    name.span(),
                    "mark at least one editable field with #[setting(label = \"message-id\")]",
                ));
            }
            let validation = validator.map(|validator| quote! {
                fn validate(&self) -> Result<(), String> { #validator(self).map_err(|error| error.to_string()) }
            });
            let editor = format_ident!("{}Editor", name);
            Ok(quote! {
                impl #api::Model for #name {
                    fn fields() -> Vec<#api::Field<Self>> { vec![#(#descriptors),*] }
                    #validation
                }
                #visibility type #editor = #api::Editor<#name>;
            })
        }
        Data::Enum(data) => {
            let mut choices = Vec::new();
            for variant in &data.variants {
                let options = Options::parse(&variant.attrs)?;
                if options.skip.is_some() {
                    if let Some(other) = options.first_non_skip_span() {
                        return Err(syn::Error::new(
                            other,
                            "#[setting(skip)] cannot be combined with other settings UI annotations",
                        ));
                    }
                    continue;
                }
                if let Some(disallowed) = options.first_variant_disallowed_span() {
                    return Err(syn::Error::new(
                        disallowed,
                        "enum choice variants only support #[setting(label = \"message-id\")] or #[setting(skip)]",
                    ));
                }
                if !matches!(variant.fields, Fields::Unit) {
                    return Err(syn::Error::new(
                        variant.span(),
                        "configuration choices require unit enum variants",
                    ));
                }
                let (label, _) = options.label.ok_or_else(|| {
                    syn::Error::new(
                        variant.span(),
                        "enum choices need #[setting(label = \"message-id\")]",
                    )
                })?;
                let variant = &variant.ident;
                choices.push(quote!(#api::Choice::new(#api::encode(&Self::#variant), #root::i18n::text(#label, cx))));
            }
            Ok(quote! { impl #api::Choices for #name {
                fn choices(cx: &#root::ui::settings::form::App) -> Vec<#api::Choice> { vec![#(#choices),*] }
            } })
        }
        Data::Union(data) => Err(syn::Error::new(
            data.union_token.span(),
            "unions cannot be configuration editors",
        )),
    }
}

#[cfg(test)]
mod tests;
