mod app_derive;
mod value_enum;

/// 为 struct 自动实现 `triforge_core::App` trait
///
/// 从字段类型推断 `ArgKind`，从 `#[arg(...)]` 属性读取元数据。
///
/// # 属性
///
/// ## Struct level
/// - `#[app(about = "...")]` — 命令描述
///
/// ## Field level
/// - `#[arg(about = "...")]` — 参数描述
/// - `#[arg(default = value)]` — 默认值
/// - `#[arg(range = lo..=hi)]` — 数字范围（仅数字类型）
/// - `#[arg(must_exist = true)]` — Path 必须存在
#[proc_macro_derive(App, attributes(app, arg))]
pub fn derive_app(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    app_derive::derive_app_impl(input.into()).into()
}

/// 为 enum 自动实现 `triforge_core::ValueEnum` trait
///
/// 自动将 PascalCase 变体名转为 snake_case 字符串。
#[proc_macro_derive(ValueEnum)]
pub fn derive_value_enum(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    value_enum::derive_value_enum_impl(input.into()).into()
}
