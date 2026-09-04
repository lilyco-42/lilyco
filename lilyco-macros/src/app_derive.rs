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

struct AppAttrs {
    about: Option<String>,
    run: Option<String>,
    name: Option<String>,
}

fn parse_app_attrs(attrs: &[Attribute]) -> AppAttrs {
    // Convention: struct-level doc comment → app about (if no explicit #[app(about = "...")])
    let doc_about = doc_comment(attrs);
    let mut result = AppAttrs {
        about: None,
        run: None,
        name: None,
    };

    for attr in attrs {
        if attr.path().is_ident("app") {
            let _ = attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("about") {
                    let s: Lit = meta.value()?.parse()?;
                    if let Lit::Str(s) = s {
                        result.about = Some(s.value());
                    }
                } else if meta.path.is_ident("run") {
                    let s: Lit = meta.value()?.parse()?;
                    if let Lit::Str(s) = s {
                        result.run = Some(s.value());
                    }
                } else if meta.path.is_ident("name") {
                    let s: Lit = meta.value()?.parse()?;
                    if let Lit::Str(s) = s {
                        result.name = Some(s.value());
                    }
                }
                Ok(())
            });
        }
    }

    if result.about.is_none() {
        result.about = doc_about;
    }

    result
}

/// Extract `/// doc comment` from field attributes (becomes about text)
fn doc_comment(attrs: &[Attribute]) -> Option<String> {
    for attr in attrs {
        if attr.path().is_ident("doc") {
            if let syn::Meta::NameValue(m) = &attr.meta {
                if let syn::Expr::Lit(syn::ExprLit {
                    lit: Lit::Str(s), ..
                }) = &m.value
                {
                    let txt = s.value().trim().to_string();
                    if !txt.is_empty() {
                        return Some(txt);
                    }
                }
            }
        }
    }
    None
}

/// snake_case → kebab-case
fn snake_to_kebab(s: &str) -> String {
    s.replace('_', "-")
}

fn parse_arg_attrs(attrs: &[Attribute]) -> ArgAttrs {
    let mut result = ArgAttrs {
        about: None,
        default: None,
        min: None,
        max: None,
        must_exist: None,
    };

    // Convention: doc comment → about
    result.about = doc_comment(attrs);

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
    is_option: bool,
}

fn is_option_type(ty: &Type) -> bool {
    match ty {
        Type::Path(tp) => tp
            .path
            .segments
            .last()
            .map_or(false, |s| s.ident == "Option"),
        _ => false,
    }
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
                        return (
                            InferredKind::List {
                                item: Box::new(inner_kind),
                            },
                            true,
                        );
                    }
                }
            }

            match name.as_str() {
                "bool" => (InferredKind::Flag, false), // flag 省略即 false
                "String" => (InferredKind::Text, true),
                "PathBuf" => (InferredKind::Path { must_exist: false }, true),
                "u8" | "u16" | "u32" | "u64" | "i8" | "i16" | "i32" | "i64" | "f32" | "f64"
                | "usize" | "isize" => (InferredKind::Number, true),
                _ => (InferredKind::Enum, true),
            }
        }
        _ => (InferredKind::Text, true),
    }
}

// ── 代码生成 ──────────────────────────────────────────────

/// 数值字段在 `as #ty` 转换时使用实际数值类型。
/// 对 `Option<u32>` / `Option<f64>` 等可选数值字段，取 `Option` 里的内层类型，
/// 否则转型会变成 `f64 as Option<u32>`（编译错误）。
fn number_cast_type(ty: &Type) -> TokenStream {
    if let Type::Path(tp) = ty {
        if let Some(last) = tp.path.segments.last() {
            if last.ident == "Option" {
                if let syn::PathArguments::AngleBracketed(args) = &last.arguments {
                    if let Some(syn::GenericArgument::Type(inner)) = args.args.first() {
                        return quote! { #inner };
                    }
                }
            }
        }
    }
    quote! { #ty }
}

pub fn derive_app_impl(input: TokenStream) -> TokenStream {
    let input: DeriveInput = syn::parse2(input).expect("App: parse error");

    let struct_name = &input.ident;
    let app_attrs = parse_app_attrs(&input.attrs);
    let about_str = app_attrs.about.unwrap_or_else(|| struct_name.to_string());
    // 命令名默认取结构体名；多命令场景建议 #[app(name = "kebab-name")] 覆盖
    let name_str = app_attrs.name.unwrap_or_else(|| struct_name.to_string());

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
        let is_option = is_option_type(&field.ty);

        // Convention: fields with a default are not required
        let required = required && arg_attrs.default.is_none();

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
            is_option,
        });
    }

    // ── generate schema() body ──
    let schema_args = field_infos.iter().map(|f| {
        let name = snake_to_kebab(&f.ident.to_string());
        let about = f.attrs.about.clone().unwrap_or_else(|| name.clone());
        let required = f.required;
        let default_expr = match &f.attrs.default {
            Some(d) => quote! { Some(serde_json::to_value(#d).unwrap()) },
            None => quote! { None },
        };
        let kind_expr = kind_to_tokens(f);

        quote! {
            lilyco_core::schema::ArgSchema {
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
        let name = snake_to_kebab(&f.ident.to_string());
        let ident = &f.ident;
        let ty = &f.ty;
        let is_opt = f.is_option;

        let num_ty = number_cast_type(&f.ty);

        let inner = match &f.kind {
            InferredKind::Flag => {
                quote! { args.get(#name).and_then(|v| v.as_bool()).unwrap_or(false) }
            }
            InferredKind::Text => {
                quote! { args.get(#name).and_then(|v| v.as_str()).unwrap_or("").to_string() }
            }
            InferredKind::Number => {
                quote! { args.get(#name).and_then(|v| v.as_f64()).unwrap_or(0.0) as #num_ty }
            }
            InferredKind::Path { .. } => quote! {
                std::path::PathBuf::from(args.get(#name).and_then(|v| v.as_str()).unwrap_or(""))
            },
            InferredKind::Enum => quote! {{
                let s = args.get(#name).and_then(|v| v.as_str()).unwrap_or("");
                <#ty as lilyco_core::schema::ValueEnum>::from_str(s)
                    .ok_or_else(|| lilyco_core::AppError::InvalidArg(
                        format!("invalid value for {}: {}", #name, s)
                    ))?
            }},
            InferredKind::List { .. } => quote! {
                args.get(#name)
                    .and_then(|v| v.as_array())
                    .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                    .unwrap_or_default()
            },
        };

        if is_opt {
            quote! {
                #ident: if args.contains_key(#name) { Some(#inner) } else { None }
            }
        } else {
            quote! {
                #ident: #inner
            }
        }
    });

    // ── generate run() body ──
    // If #[app(run = "fn_name")] is specified, call that function;
    // otherwise fall back to unimplemented!() (backward compatible).
    let run_impl = match &app_attrs.run {
        Some(fn_name) => {
            let fn_ident = syn::Ident::new(fn_name, proc_macro2::Span::call_site());
            quote! {
                fn run(&self, ctx: &lilyco_core::Context) -> Result<serde_json::Value, lilyco_core::AppError> {
                    #fn_ident(self, ctx)
                }
            }
        }
        None => {
            quote! {
                fn run(&self, _ctx: &lilyco_core::Context) -> Result<serde_json::Value, lilyco_core::AppError> {
                    unimplemented!("run() not implemented for {} — add #[app(run = \"your_fn\")] to wire up business logic", stringify!(#struct_name))
                }
            }
        }
    };

    let expanded = quote! {
        impl lilyco_core::App for #struct_name {
            fn schema() -> lilyco_core::schema::CommandSchema {
                lilyco_core::schema::CommandSchema {
                    name: #name_str.into(),
                    about: #about_str.into(),
                    args: vec![#(#schema_args),*],
                    subcommands: vec![],
                }
            }

            fn from_args(
                args: &std::collections::HashMap<String, serde_json::Value>,
            ) -> Result<Self, lilyco_core::AppError> {
                Ok(Self {
                    #(#from_args_bindings),*
                })
            }

            #run_impl
        }
    };

    expanded
}

fn kind_to_tokens(f: &FieldInfo) -> TokenStream {
    match &f.kind {
        InferredKind::Flag => quote! { lilyco_core::schema::ArgKind::Flag },
        InferredKind::Text => quote! { lilyco_core::schema::ArgKind::Text },
        InferredKind::Number => {
            let min = f
                .attrs
                .min
                .as_ref()
                .map(|m| quote! { Some(#m as f64) })
                .unwrap_or(quote! { None });
            let max = f
                .attrs
                .max
                .as_ref()
                .map(|m| quote! { Some(#m as f64) })
                .unwrap_or(quote! { None });
            quote! { lilyco_core::schema::ArgKind::Number { min: #min, max: #max } }
        }
        InferredKind::Path { must_exist } => {
            quote! { lilyco_core::schema::ArgKind::Path { must_exist: #must_exist } }
        }
        InferredKind::Enum => {
            let ty = &f.ty;
            quote! {
                lilyco_core::schema::ArgKind::Enum {
                    values: <#ty as lilyco_core::schema::ValueEnum>::variants().into_iter().map(|s| s.to_string()).collect()
                }
            }
        }
        InferredKind::List { item } => {
            let inner = match item.as_ref() {
                InferredKind::Text => quote! { lilyco_core::schema::ArgKind::Text },
                InferredKind::Number => {
                    quote! { lilyco_core::schema::ArgKind::Number { min: None, max: None } }
                }
                InferredKind::Flag => quote! { lilyco_core::schema::ArgKind::Flag },
                _ => quote! { lilyco_core::schema::ArgKind::Text },
            };
            quote! { lilyco_core::schema::ArgKind::List { item: Box::new(#inner) } }
        }
    }
}
