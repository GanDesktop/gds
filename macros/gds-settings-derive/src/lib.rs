//! Derive typed field bindings for the shared native settings editor.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as Tokens;
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
    label: Option<LitStr>,
    help: Option<LitStr>,
    group: Option<LitStr>,
    id: Option<LitStr>,
    kind: Option<LitStr>,
    skip: bool,
    min: Option<Expr>,
    max: Option<Expr>,
    step: Option<Expr>,
    scale: Option<Expr>,
    default: Option<Expr>,
    unit: Option<LitStr>,
}

impl Options {
    fn parse(attrs: &[Attribute]) -> syn::Result<Self> {
        let mut result = Self::default();
        for attr in attrs.iter().filter(|a| a.path().is_ident("setting")) {
            attr.parse_nested_meta(|meta| {
                let key = meta
                    .path
                    .get_ident()
                    .map(ToString::to_string)
                    .unwrap_or_default();
                match key.as_str() {
                    "skip" => result.skip = true,
                    "label" => result.label = Some(meta.value()?.parse()?),
                    "help" => result.help = Some(meta.value()?.parse()?),
                    "group" => result.group = Some(meta.value()?.parse()?),
                    "id" => result.id = Some(meta.value()?.parse()?),
                    "kind" => result.kind = Some(meta.value()?.parse()?),
                    "min" => result.min = Some(meta.value()?.parse()?),
                    "max" => result.max = Some(meta.value()?.parse()?),
                    "step" => result.step = Some(meta.value()?.parse()?),
                    "scale" => result.scale = Some(meta.value()?.parse()?),
                    "default" => result.default = Some(meta.value()?.parse()?),
                    "unit" => result.unit = Some(meta.value()?.parse()?),
                    _ => return Err(meta.error("unknown settings UI annotation")),
                }
                Ok(())
            })?;
        }
        Ok(result)
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
                if options.skip {
                    continue;
                }
                let member = field.ident.as_ref().expect("named field");
                let ty = &field.ty;
                let (inner, nullable, type_name) = inner_type(ty)?;
                let label = options.label.ok_or_else(|| {
                    syn::Error::new(
                        field.span(),
                        "editable fields need #[setting(label = \"message-id\")]",
                    )
                })?;
                let id = options
                    .id
                    .unwrap_or_else(|| LitStr::new(&member.to_string(), member.span()));
                if !identities.insert(id.value()) {
                    return Err(syn::Error::new(id.span(), "duplicate settings field id"));
                }
                let help = options
                    .help
                    .unwrap_or_else(|| LitStr::new("", member.span()));
                let group = options
                    .group
                    .unwrap_or_else(|| LitStr::new("", member.span()));
                let kind = match options.kind.as_ref().map(LitStr::value).as_deref() {
                    Some("font") => quote!(#api::Kind::Font),
                    Some("language") => quote!(#api::Kind::Language),
                    Some("text") => quote!(#api::Kind::Text),
                    Some(_) => {
                        return Err(syn::Error::new(
                            field.span(),
                            "kind must be font, language or text; primitive types and enums are inferred",
                        ));
                    }
                    None => match type_name.as_str() {
                        "bool" => quote!(#api::Kind::Toggle),
                        "String" | "PathBuf" => quote!(#api::Kind::Text),
                        "Language" => quote!(#api::Kind::Language),
                        "f32" | "f64" | "i8" | "i16" | "i32" | "i64" | "isize" | "u8" | "u16"
                        | "u32" | "u64" | "usize" => {
                            let min = options.min.unwrap_or_else(|| syn::parse_quote!(0.0));
                            let max = options.max.unwrap_or_else(|| syn::parse_quote!(100.0));
                            let step = options.step.unwrap_or_else(|| syn::parse_quote!(1.0));
                            let scale = options.scale.unwrap_or_else(|| syn::parse_quote!(1.0));
                            let unit = options
                                .unit
                                .unwrap_or_else(|| LitStr::new("", member.span()));
                            let integer = if type_name.starts_with('u') {
                                quote!(#api::Integer::Unsigned)
                            } else if type_name.starts_with('i') {
                                quote!(#api::Integer::Signed)
                            } else {
                                quote!(#api::Integer::Float)
                            };
                            quote!(#api::Kind::Number(#api::Number {
                                min: (#min) as f64, max: (#max) as f64, step: (#step) as f64,
                                scale: (#scale) as f64, unit: #unit, integer: #integer,
                            }))
                        }
                        "Vec" | "HashMap" | "BTreeMap" | "i128" | "u128" => {
                            return Err(syn::Error::new(
                                field.span(),
                                "this field needs a custom editor; leave it unannotated or use #[setting(skip)]",
                            ));
                        }
                        _ => quote!(#api::Kind::Choice(<#inner as #api::Choices>::choices)),
                    },
                };
                let default = match options.default {
                    Some(value) => quote!(Some(|| #api::encode(&(#value)))),
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
                if options.skip {
                    continue;
                }
                if !matches!(variant.fields, Fields::Unit) {
                    return Err(syn::Error::new(
                        variant.span(),
                        "configuration choices require unit enum variants",
                    ));
                }
                let label = options.label.ok_or_else(|| {
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
