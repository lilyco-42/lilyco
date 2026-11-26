//! 10 年期 UI 主题与多框架适配器契约（零前端/渲染依赖）。
//!
//! 设计原则：
//! 1. **语义核与渲染壳彻底分离**：业务代码只声明 `Component` 与 `ThemePreset` / `ThemeSpec`，
//!    绝不绑定任何具体前端框架（React/Vue/NSH/TUI）。
//! 2. **向前兼容（Additive-Only）**：`UI_PROTOCOL_VERSION` 遵循 SemVer；新增令牌与预设只增不改；
//!    未知组件或主题字段由适配器按 `FallbackStrategy` 优雅降级，永不崩溃。
//! 3. **一套 Token 驱动全部主流框架**：覆盖 shadcn/ui、Ant Design、Material 3、daisyUI、
//!    Chakra UI、Mantine、HeroUI、Element Plus、Radix Headless、PrimeReact 10 套官方预设。

use serde::{Deserialize, Serialize};

/// Lilyco UI 协议版本（向后兼容承诺：主版本 `1` 内永不删除或破坏已有 `Component` kind 与核心 Token）。
pub const UI_PROTOCOL_VERSION: &str = "1.0";

/// 内置 10 大主流 UI 框架主题预设 + 自定义扩展槽位。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ThemePreset {
    #[default]
    ShadcnZinc,
    AntDesign,
    Material3,
    DaisyUi,
    ChakraUi,
    Mantine,
    HeroUi,
    ElementPlus,
    RadixHeadless,
    PrimeAura,
    Custom(String),
}

impl ThemePreset {
    /// 全部 10 套内置预设列表（用于展厅与配置枚举）。
    pub fn all_builtins() -> &'static [ThemePreset] {
        &[
            Self::ShadcnZinc,
            Self::AntDesign,
            Self::Material3,
            Self::DaisyUi,
            Self::ChakraUi,
            Self::Mantine,
            Self::HeroUi,
            Self::ElementPlus,
            Self::RadixHeadless,
            Self::PrimeAura,
        ]
    }

    pub fn id(&self) -> &str {
        match self {
            Self::ShadcnZinc => "shadcn_zinc",
            Self::AntDesign => "ant_design",
            Self::Material3 => "material_3",
            Self::DaisyUi => "daisy_ui",
            Self::ChakraUi => "chakra_ui",
            Self::Mantine => "mantine",
            Self::HeroUi => "hero_ui",
            Self::ElementPlus => "element_plus",
            Self::RadixHeadless => "radix_headless",
            Self::PrimeAura => "prime_aura",
            Self::Custom(name) => name.as_str(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ColorMode {
    #[default]
    Light,
    Dark,
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Density {
    Compact,
    #[default]
    Default,
    Comfortable,
}

/// 语义调色板（单套明或暗色），同时映射 `lilyco-gui` 与 shadcn/W3C Design Tokens。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticPalette {
    pub background: String,
    pub foreground: String,
    pub card: String,
    pub muted: String,
    pub muted_foreground: String,
    pub border: String,
    pub primary: String,
    pub primary_foreground: String,
    pub success: String,
    pub warning: String,
    pub danger: String,
}

/// 形状与节奏令牌（圆角、控件高度、阴影）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShapeTokens {
    pub radius_sm: String,
    pub radius_md: String,
    pub radius_lg: String,
    pub control_height: String,
    pub card_shadow: String,
}

/// 完整的跨端主题规范（可序列化为 JSON 供 React/Vue/NSH/TUI 适配器消费，或直接输出 CSS 变量）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThemeSpec {
    pub protocol_version: String,
    pub preset: ThemePreset,
    pub display_name: String,
    pub mode: ColorMode,
    pub density: Density,
    pub light: SemanticPalette,
    pub dark: SemanticPalette,
    pub shape: ShapeTokens,
}

impl ThemeSpec {
    /// 生成指定主流框架预设的完整 `ThemeSpec`。
    pub fn from_preset(preset: ThemePreset) -> Self {
        let (display_name, primary_light, primary_fg_light, primary_dark, primary_fg_dark, shape) =
            match &preset {
                ThemePreset::ShadcnZinc => (
                    "shadcn/ui (Zinc)",
                    "#18181b",
                    "#fafafa",
                    "#fafafa",
                    "#18181b",
                    ShapeTokens {
                        radius_sm: "4px".into(),
                        radius_md: "6px".into(),
                        radius_lg: "8px".into(),
                        control_height: "36px".into(),
                        card_shadow: "0 1px 2px rgba(0,0,0,0.05)".into(),
                    },
                ),
                ThemePreset::AntDesign => (
                    "Ant Design",
                    "#1677ff",
                    "#ffffff",
                    "#1668dc",
                    "#ffffff",
                    ShapeTokens {
                        radius_sm: "4px".into(),
                        radius_md: "6px".into(),
                        radius_lg: "8px".into(),
                        control_height: "32px".into(),
                        card_shadow: "0 1px 2px rgba(0,0,0,0.04)".into(),
                    },
                ),
                ThemePreset::Material3 => (
                    "Material UI (M3)",
                    "#1976d2",
                    "#ffffff",
                    "#90caf9",
                    "#0b1220",
                    ShapeTokens {
                        radius_sm: "4px".into(),
                        radius_md: "4px".into(),
                        radius_lg: "12px".into(),
                        control_height: "36px".into(),
                        card_shadow: "0 2px 8px rgba(0,0,0,0.08)".into(),
                    },
                ),
                ThemePreset::DaisyUi => (
                    "daisyUI",
                    "#570df8",
                    "#ffffff",
                    "#7c3aed",
                    "#ffffff",
                    ShapeTokens {
                        radius_sm: "8px".into(),
                        radius_md: "10px".into(),
                        radius_lg: "16px".into(),
                        control_height: "38px".into(),
                        card_shadow: "0 4px 14px rgba(0,0,0,0.06)".into(),
                    },
                ),
                ThemePreset::ChakraUi => (
                    "Chakra UI",
                    "#319795",
                    "#ffffff",
                    "#4fd1c5",
                    "#0b1220",
                    ShapeTokens {
                        radius_sm: "4px".into(),
                        radius_md: "6px".into(),
                        radius_lg: "10px".into(),
                        control_height: "36px".into(),
                        card_shadow: "0 1px 3px rgba(0,0,0,0.07)".into(),
                    },
                ),
                ThemePreset::Mantine => (
                    "Mantine",
                    "#228be6",
                    "#ffffff",
                    "#339af0",
                    "#ffffff",
                    ShapeTokens {
                        radius_sm: "4px".into(),
                        radius_md: "6px".into(),
                        radius_lg: "8px".into(),
                        control_height: "34px".into(),
                        card_shadow: "0 1px 3px rgba(0,0,0,0.06)".into(),
                    },
                ),
                ThemePreset::HeroUi => (
                    "HeroUI",
                    "#006fee",
                    "#ffffff",
                    "#338ef7",
                    "#ffffff",
                    ShapeTokens {
                        radius_sm: "8px".into(),
                        radius_md: "12px".into(),
                        radius_lg: "14px".into(),
                        control_height: "38px".into(),
                        card_shadow: "0 4px 16px rgba(0,0,0,0.06)".into(),
                    },
                ),
                ThemePreset::ElementPlus => (
                    "Element Plus",
                    "#409eff",
                    "#ffffff",
                    "#409eff",
                    "#ffffff",
                    ShapeTokens {
                        radius_sm: "2px".into(),
                        radius_md: "4px".into(),
                        radius_lg: "4px".into(),
                        control_height: "32px".into(),
                        card_shadow: "0 2px 12px rgba(0,0,0,0.06)".into(),
                    },
                ),
                ThemePreset::RadixHeadless => (
                    "Radix Primitives (Headless)",
                    "#0f172a",
                    "#ffffff",
                    "#f8fafc",
                    "#0f172a",
                    ShapeTokens {
                        radius_sm: "2px".into(),
                        radius_md: "4px".into(),
                        radius_lg: "6px".into(),
                        control_height: "32px".into(),
                        card_shadow: "none".into(),
                    },
                ),
                ThemePreset::PrimeAura => (
                    "PrimeReact (Aura)",
                    "#10b981",
                    "#ffffff",
                    "#34d399",
                    "#06281c",
                    ShapeTokens {
                        radius_sm: "4px".into(),
                        radius_md: "6px".into(),
                        radius_lg: "10px".into(),
                        control_height: "34px".into(),
                        card_shadow: "0 1px 3px rgba(0,0,0,0.06)".into(),
                    },
                ),
                ThemePreset::Custom(name) => (
                    name.as_str(),
                    "#18181b",
                    "#fafafa",
                    "#fafafa",
                    "#18181b",
                    ShapeTokens {
                        radius_sm: "4px".into(),
                        radius_md: "6px".into(),
                        radius_lg: "8px".into(),
                        control_height: "36px".into(),
                        card_shadow: "0 1px 2px rgba(0,0,0,0.05)".into(),
                    },
                ),
            };

        Self {
            protocol_version: UI_PROTOCOL_VERSION.to_string(),
            preset: preset.clone(),
            display_name: display_name.to_string(),
            mode: ColorMode::Light,
            density: Density::Default,
            light: SemanticPalette {
                background: "#ffffff".into(),
                foreground: "#09090b".into(),
                card: "#ffffff".into(),
                muted: "#f4f4f5".into(),
                muted_foreground: "#71717a".into(),
                border: "#e4e4e7".into(),
                primary: primary_light.into(),
                primary_foreground: primary_fg_light.into(),
                success: "#16a34a".into(),
                warning: "#b45309".into(),
                danger: "#dc2626".into(),
            },
            dark: SemanticPalette {
                background: "#09090b".into(),
                foreground: "#fafafa".into(),
                card: "#111113".into(),
                muted: "#27272a".into(),
                muted_foreground: "#a1a1aa".into(),
                border: "#27272a".into(),
                primary: primary_dark.into(),
                primary_foreground: primary_fg_dark.into(),
                success: "#4ade80".into(),
                warning: "#fbbf24".into(),
                danger: "#ef4444".into(),
            },
            shape,
        }
    }

    /// 导出为标准 CSS 自定义属性块（`:root` + `.dark`），任何 Web 渲染器可直接挂载。
    pub fn to_css_vars(&self) -> String {
        format!(
            ":root {{\n  --background: {};\n  --foreground: {};\n  --card: {};\n  --muted: {};\n  --muted-foreground: {};\n  --border: {};\n  --primary: {};\n  --primary-foreground: {};\n  --success: {};\n  --warning: {};\n  --destructive: {};\n  --radius-sm: {};\n  --radius: {};\n  --radius-lg: {};\n  --control-height: {};\n  --card-shadow: {};\n}}\n.dark {{\n  --background: {};\n  --foreground: {};\n  --card: {};\n  --muted: {};\n  --muted-foreground: {};\n  --border: {};\n  --primary: {};\n  --primary-foreground: {};\n  --success: {};\n  --warning: {};\n  --destructive: {};\n}}\n",
            self.light.background,
            self.light.foreground,
            self.light.card,
            self.light.muted,
            self.light.muted_foreground,
            self.light.border,
            self.light.primary,
            self.light.primary_foreground,
            self.light.success,
            self.light.warning,
            self.light.danger,
            self.shape.radius_sm,
            self.shape.radius_md,
            self.shape.radius_lg,
            self.shape.control_height,
            self.shape.card_shadow,
            self.dark.background,
            self.dark.foreground,
            self.dark.card,
            self.dark.muted,
            self.dark.muted_foreground,
            self.dark.border,
            self.dark.primary,
            self.dark.primary_foreground,
            self.dark.success,
            self.dark.warning,
            self.dark.danger,
        )
    }
}

/// 未知未来组件时的降级策略（10 年期向前兼容保证）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum FallbackStrategy {
    /// 渲染为通用卡片（展示 title/description 并递归渲染 children）
    #[default]
    DegradeToCard,
    /// 渲染为可读纯文本/Markdown（适用于 TUI/CLI）
    DegradeToText,
    /// 渲染为原始 JSON 代码块（适用于调试器）
    DegradeToJson,
}

/// 任意外部/未来 UI 框架适配器必须声明的能力清单（Adapter Contract）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdapterCapabilities {
    pub adapter_id: String,
    pub framework_name: String,
    pub protocol_version: String,
    pub supports_dark_mode: bool,
    pub supports_density: bool,
    pub fallback: FallbackStrategy,
    /// 已实现原生渲染的 `Component` kind 列表（未列出的自动走 `fallback`）
    pub supported_kinds: Vec<String>,
}

impl AdapterCapabilities {
    /// 校验该适配器是否兼容当前 `lilyco-core` UI 协议主版本。
    pub fn is_protocol_compatible(&self) -> bool {
        let cur_major = UI_PROTOCOL_VERSION.split('.').next().unwrap_or("1");
        let adp_major = self.protocol_version.split('.').next().unwrap_or("");
        cur_major == adp_major
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_ten_builtin_presets_roundtrip_and_emit_css() {
        assert_eq!(ThemePreset::all_builtins().len(), 10);
        for preset in ThemePreset::all_builtins() {
            let spec = ThemeSpec::from_preset(preset.clone());
            let json = serde_json::to_string(&spec).unwrap();
            let decoded: ThemeSpec = serde_json::from_str(&json).unwrap();
            assert_eq!(decoded, spec);
            let css = spec.to_css_vars();
            assert!(css.contains("--primary:"));
            assert!(css.contains(".dark {"));
        }
    }

    #[test]
    fn adapter_protocol_compatibility_checks_major_version() {
        let ok = AdapterCapabilities {
            adapter_id: "react-shadcn".into(),
            framework_name: "shadcn/ui".into(),
            protocol_version: "1.2".into(),
            supports_dark_mode: true,
            supports_density: true,
            fallback: FallbackStrategy::DegradeToCard,
            supported_kinds: vec!["card".into(), "table".into()],
        };
        assert!(ok.is_protocol_compatible());

        let breaking = AdapterCapabilities {
            protocol_version: "2.0".into(),
            ..ok
        };
        assert!(!breaking.is_protocol_compatible());
    }
}
