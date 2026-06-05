use proc_macro2::TokenStream;
use quote::quote;
use syn::{Attribute, Data, DeriveInput, Expr, Fields, Lit, Type};

// ── 属性解析 ──────────────────────────────────────────────

struct ArgAttrs {
    about: Option<String>,
    default: Option<Expr>,
    min: Option<Expr>,
    max: Option<Expr>,
    must_exist: Option<bool>,
}

fn parse_app_about(attrs: &[Attribute]) -> Option<String> {
    for attr in attrs {
        if attr.path().is_ident("app") {
            let mut about = None;
            let _ = attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("about") {
                    let s: Lit = meta.value()?.parse()?;
                    if let Lit::Str(s) = s {
                        about = Some(s.value());
                    }
                }
                Ok(())
            });
            if about.is_some() {
                return about;
            }
        }
    }
    None
}

fn parse_arg_attrs(attrs: &[Attribute]) -> ArgAttrs {
    let mut result = ArgAttrs {
        about: None,
        default: None,
        min: None,
        max: None,
        must_exist: None,
    };

    for attr in attrs {
        if !attr.path().is_ident("arg") {
            continue;
        }

        let _ = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("about") {
                let s: Lit = meta.value()?.parse()?;
                if let Lit::Str(s) = s {
                    result.about = Some(s.value());
                }
            } else if meta.path.is_ident("default") {
                let v: Expr = meta.value()?.parse()?;
                result.default = Some(v);
            } else if meta.path.is_ident("range") {
                // range = 0..=51 → extract lo/hi from string
                let ts: TokenStream = meta.value()?.parse()?;
                let range_str = ts.to_string();
                let parts: Vec<&str> = range_str.split("..=").collect();
                if parts.len() == 2 {
                    if let Ok(lo) = parts[0].trim().parse::<f64>() {
                        result.min = Some(syn::parse_quote!(#lo));
                    }
                    if let Ok(hi) = parts[1].trim().parse::<f64>() {
                        result.max = Some(syn::parse_quote!(#hi));
                    }
                }
            } else if meta.path.is_ident("min") {
                let v: Expr = meta.value()?.parse()?;
                result.min = Some(v);
            } else if meta.path.is_ident("max") {
                let v: Expr = meta.value()?.parse()?;
                result.max = Some(v);
            } else if meta.path.is_ident("must_exist") {
                let v: Lit = meta.value()?.parse()?;
                if let Lit::Bool(b) = v {
                    result.must_exist = Some(b.value);
                }
            }
            Ok(())
        });
    }
    result
}

// ── 类型推断 ──────────────────────────────────────────────

enum InferredKind {
    Flag,
    Text,
    Number,
    Path { must_exist: bool },
    Enum,
    List { item: Box<InferredKind> },
}

struct FieldInfo {
    ident: syn::Ident,
    attrs: ArgAttrs,
    kind: InferredKind,
    required: bool,
    ty: Type,
}

fn infer_kind(ty: &Type) -> (InferredKind, bool) {
    match ty {
        Type::Path(tp) => {
            let last = tp.path.segments.last().unwrap();
            let name = last.ident.to_string();

            if name == "Option" {
                if let syn::PathArguments::AngleBracketed(ref args) = last.arguments {
                    if let Some(syn::GenericArgument::Type(inner)) = args.args.first() {
                        let (kind, _) = infer_kind(inner);
                        return (kind, false);
                    }
                }
            }

            if name == "Vec" {
                if let syn::PathArguments::AngleBracketed(ref args) = last.arguments {
                    if let Some(syn::GenericArgument::Type(inner)) = args.args.first() {
                        let (inner_kind, _) = infer_kind(inner);
                        return (InferredKind::List { item: Box::new(inner_kind) }, true);
                    }
                }
            }

            match name.as_str() {
                "bool" => (InferredKind::Flag, true),
                "String" => (InferredKind::Text, true),
                "PathBuf" => (InferredKind::Path { must_exist: false }, true),
                "u8" | "u16" | "u32" | "u64" | "i8" | "i16" | "i32" | "i64"
                | "f32" | "f64" | "usize" | "isize" => (InferredKind::Number, true),
                _ => (InferredKind::Enum, true),
            }
        }
        _ => (InferredKind::Text, true),
    }
}

// ── 代码生成 ──────────────────────────────────────────────

pub fn derive_app_impl(input: TokenStream) -> TokenStream {
    let input: DeriveInput = syn::parse2(input).expect("App: parse error");

    let struct_name = &input.ident;
    let about_str = parse_app_about(&input.attrs)
        .unwrap_or_else(|| struct_name.to_string());

    let fields = match &input.data {
        Data::Struct(s) => match &s.fields {
            Fields::Named(fields) => &fields.named,
            _ => panic!("App: only named fields are supported"),
        },
        _ => panic!("App: only works on structs"),
    };

    let mut field_infos = Vec::new();
    for field in fields.iter() {
        let ident = field.ident.clone().unwrap();
        let arg_attrs = parse_arg_attrs(&field.attrs);
        let (mut kind, required) = infer_kind(&field.ty);

        if let InferredKind::Path { ref mut must_exist } = kind {
            if let Some(me) = arg_attrs.must_exist {
                *must_exist = me;
            }
        }

        field_infos.push(FieldInfo {
            ident,
            attrs: arg_attrs,
            kind,
            required,
            ty: field.ty.clone(),
        });
    }

    // ── generate schema() body ──
    let schema_args = field_infos.iter().map(|f| {
        let name = f.ident.to_string();
        let about = f.attrs.about.clone().unwrap_or_else(|| f.ident.to_string());
        let required = f.required;
        let default_expr = match &f.attrs.default {
            Some(d) => quote! { Some(serde_json::to_value(#d).unwrap()) },
            None => quote! { None },
        };
        let kind_expr = kind_to_tokens(&f.kind);

        quote! {
            triforge_core::schema::ArgSchema {
                name: #name.into(),
                about: #about.into(),
                kind: #kind_expr,
                required: #required,
                default: #default_expr,
            }
        }
    });

    // ── generate from_args() body ──
    let from_args_bindings = field_infos.iter().map(|f| {
        let name = f.ident.to_string();
        let ident = &f.ident;
        let ty = &f.ty;

        match &f.kind {
            InferredKind::Flag => quote! {
                #ident: args.get(#name).and_then(|v| v.as_bool()).unwrap_or(false)
            },
            InferredKind::Text => quote! {
                #ident: args.get(#name).and_then(|v| v.as_str()).unwrap_or("").to_string()
            },
            InferredKind::Number => quote! {
                #ident: args.get(#name).and_then(|v| v.as_f64()).unwrap_or(0.0) as #ty
            },
            InferredKind::Path { .. } => quote! {
                #ident: std::path::PathBuf::from(
                    args.get(#name).and_then(|v| v.as_str()).unwrap_or("")
                )
            },
            InferredKind::Enum => quote! {
                #ident: {
                    let s = args.get(#name).and_then(|v| v.as_str()).unwrap_or("");
                    <#ty as triforge_core::schema::ValueEnum>::from_str(s)
                        .ok_or_else(|| triforge_core::AppError::InvalidArg(
                            format!("invalid value for {}: {}", #name, s)
                        ))?
                }
            },
            InferredKind::List { .. } => quote! {
                #ident: args.get(#name)
                    .and_then(|v| v.as_array())
                    .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                    .unwrap_or_default()
            },
        }
    });

    let expanded = quote! {
        impl triforge_core::App for #struct_name {
            fn schema() -> triforge_core::schema::CommandSchema {
                triforge_core::schema::CommandSchema {
                    name: stringify!(#struct_name).into(),
                    about: #about_str.into(),
                    args: vec![#(#schema_args),*],
                    subcommands: vec![],
                }
            }

            fn from_args(
                args: &std::collections::HashMap<String, serde_json::Value>,
            ) -> Result<Self, triforge_core::AppError> {
                Ok(Self {
                    #(#from_args_bindings)*
                })
            }

            fn run(&self, _ctx: &triforge_core::Context) -> Result<serde_json::Value, triforge_core::AppError> {
                unimplemented!("run() not implemented for {}", stringify!(#struct_name))
            }
        }
    };

    expanded
}

fn kind_to_tokens(kind: &InferredKind) -> TokenStream {
    match kind {
        InferredKind::Flag => quote! { triforge_core::schema::ArgKind::Flag },
        InferredKind::Text => quote! { triforge_core::schema::ArgKind::Text },
        InferredKind::Number => quote! { triforge_core::schema::ArgKind::Number { min: None, max: None } },
        InferredKind::Path { must_exist } => {
            quote! { triforge_core::schema::ArgKind::Path { must_exist: #must_exist } }
        }
        InferredKind::Enum => quote! {
            triforge_core::schema::ArgKind::Enum {
                values: vec![] // placeholder — user should override via #[arg(enum_values = ...)]
            }
        },
        InferredKind::List { item } => {
            let inner = match item.as_ref() {
                InferredKind::Text => quote! { triforge_core::schema::ArgKind::Text },
                InferredKind::Number => quote! { triforge_core::schema::ArgKind::Number { min: None, max: None } },
                InferredKind::Flag => quote! { triforge_core::schema::ArgKind::Flag },
                _ => quote! { triforge_core::schema::ArgKind::Text },
            };
            quote! { triforge_core::schema::ArgKind::List { item: Box::new(#inner) } }
        }
    }
}
