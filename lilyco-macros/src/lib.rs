//! # Lilyco Macros（发布契约：随 facade 同步）
//!
//! ## ⚠️ 与 lilyco facade 强耦合
//!
//! 本 crate 展开的代码引用 `::lilyco::__core::…`（facade 的 doc-hidden 再导出），
//! 因此**必须与 lilyco facade 同版本发布**：facade 侧以 `version = "=x.y.z"` 精确
//! 锁定本 crate（semver 不可见的耦合边 → 显式版本锁定）。单独升级本 crate 会
//! 导致用户编译失败——版本号永远跟 facade 走。

mod app_derive;
mod value_enum;

/// 为 struct 自动实现 `lilyco_core::App` trait
///
/// 从字段类型推断 `ArgKind`，从 `#[arg(...)]` 属性读取元数据。
///
/// # 属性
///
/// ## Struct level
/// - `#[app(about = "...")]` — 命令描述
/// - `#[app(run = "fn_name")]` — 指定 run() 调用的业务逻辑函数
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

/// 为 enum 自动实现 `lilyco_core::ValueEnum` trait
///
/// 自动将 PascalCase 变体名转为 snake_case 字符串。
#[proc_macro_derive(ValueEnum)]
pub fn derive_value_enum(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    value_enum::derive_value_enum_impl(input.into()).into()
}
