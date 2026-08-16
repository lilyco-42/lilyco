use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput};

pub fn derive_value_enum_impl(input: TokenStream) -> TokenStream {
    let input: DeriveInput = syn::parse2(input).expect("ValueEnum: parse error");

    let name = &input.ident;

    let variants: Vec<_> = match &input.data {
        Data::Enum(e) => e
            .variants
            .iter()
            .map(|v| (v.ident.clone(), pascal_to_snake(&v.ident.to_string())))
            .collect(),
        _ => panic!("ValueEnum: only works on enums"),
    };

    let variant_idents: Vec<_> = variants.iter().map(|(i, _)| i).collect();
    let variant_strs: Vec<_> = variants.iter().map(|(_, s)| s).collect();

    let expanded = quote! {
        impl lilyco_core::schema::ValueEnum for #name {
            fn variants() -> Vec<&'static str> {
                vec![#(#variant_strs),*]
            }

            fn from_str(s: &str) -> Option<Self> {
                match s {
                    #(#variant_strs => Some(Self::#variant_idents),)*
                    _ => None,
                }
            }
        }
    };

    expanded
}

/// 将 PascalCase / camelCase 转为 snake_case
fn pascal_to_snake(name: &str) -> String {
    let mut result = String::new();
    let mut chars = name.chars().peekable();

    while let Some(c) = chars.next() {
        if c.is_uppercase() {
            if !result.is_empty() {
                result.push('_');
            }
            result.push(c.to_lowercase().next().unwrap_or(c));
            // 后续连续大写字母视为整体（如 H264 → h264, Av1 → av1）
            while let Some(&next) = chars.peek() {
                if next.is_lowercase()
                    && result.len() >= 2
                    && result.as_bytes()[result.len() - 2] != b'_'
                {
                    // hmm, complex case. Simple: just push lowercase for current
                    break;
                }
                if next.is_uppercase() {
                    chars.next();
                    result.push(next.to_lowercase().next().unwrap_or(next));
                } else {
                    break;
                }
            }
        } else {
            result.push(c);
        }
    }

    result
}

#[test]
fn test_pascal_to_snake() {
    assert_eq!(pascal_to_snake("H264"), "h264");
    assert_eq!(pascal_to_snake("H265"), "h265");
    assert_eq!(pascal_to_snake("Av1"), "av1");
    assert_eq!(pascal_to_snake("Codec"), "codec");
    assert_eq!(pascal_to_snake("MyEnum"), "my_enum");
}
