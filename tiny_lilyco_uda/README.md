# tiny_lilyco_uda — UniDesktop/SDK 桥

把 [UniDesktop/SDK](https://github.com/UniDesktop/SDK) 的桌面能力接进 `tiny_lilyco`
的设计系统：**系统主题 → 主题选择**、**能力矩阵 → Header 徽章**。

## 已核对的 API（tag `v0.2.1`）

来源：`UniDesktop/SDK` 仓库 `crates/uda-core/src/{appearance,capability}.rs`、
`crates/uda-platform-{windows,linux}/src/appearance.rs`。

```rust
// uda-core
pub trait AppearanceManager {
    fn detect_theme(&self) -> Result<Theme, UdaError>;
    fn get_accent_color(&self) -> Result<RgbaColor, UdaError>;
    fn capabilities(&self) -> Result<Capability, UdaError>;
}
pub enum Theme { Light, Dark, Auto, Unknown }
pub struct RgbaColor { pub r: u8, pub g: u8, pub b: u8, pub a: u8 }
bitflags! { pub struct Capability: u32 { const DETECT_THEME = …; … } }

// 平台实现
uda_platform_windows::appearance::WindowsAppearanceManager::new()
uda_platform_linux::appearance::LinuxAppearanceManager::new()
```

## 映射规则

| UDA | 本项目 |
| :--- | :--- |
| `Theme::Dark` | `Theme::tokyo_night()`（产品默认） |
| `Theme::Light` | `Theme::neutral_light()` |
| `Theme::Auto` / `Unknown` | 保持 `Theme::default_theme()`，**不猜** |
| `Capability` | `DesktopFacts::capability_count()` → Header 徽章 `dark · 7 caps` |
| 探测失败 | `degraded = Some(reason)`，徽章转 warning 色，不 panic |

`Auto` 与 `Unknown` 故意不映射：`Auto` 表示"系统自己会切"，`Unknown` 表示"问不出来"，
两者都不该被当成 `Light`。

## 为什么是独立 crate

UDA 是 **git 依赖**（`tag = "v0.2.1"`），且 `uda-platform-windows` 整个 crate 被
`#![cfg(windows)]` 门控。把它塞进 `tiny_lilyco` 会让 headless CI 在解析依赖时就要拉 git。
所以这里用独立 `[workspace]`，只在需要桌面能力时构建。

## 验证

* 单元测试覆盖映射与降级（不依赖真机）。
* 真机探测由 `.github/workflows/tiny_lilyco_uda.yml`（手动触发）在
  ubuntu / windows 两个 runner 上各跑一次。
