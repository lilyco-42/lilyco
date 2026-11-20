# Lain42 Web2CLI 工作台 — shadcn/ui 设计规范

本规范定义 `lilyco-web2cli` 服务端工作台（`lain42.top` / `dev.lain42.top` / `hw.lain42.top`）的唯一界面标准，严格遵循 **shadcn/ui (New York / Zinc)** 设计语言。

## 1. 语义色板（Light 优先，支持 `.dark` 切换）

严禁使用霓虹发光背景、大面积渐变色块或装饰性 Emoji；所有界面色一律引用语义 CSS 变量：

| Token | Light（默认） | Dark（`.dark`） | 用途 |
|---|---|---|---|
| `--background` | `#ffffff` | `#09090b` | 页面底色 |
| `--foreground` | `#09090b` | `#fafafa` | 主正文与标题 |
| `--card` | `#ffffff` | `#09090b` | 卡片容器底色 |
| `--muted` | `#f4f4f5` | `#27272a` | 次级区域、Tab 底轨、代码块底色 |
| `--muted-foreground` | `#71717a` | `#a1a1aa` | 辅助说明、占位符、次级标签 |
| `--border` | `#e4e4e7` | `#27272a` | 1px 极细分割线与卡片描边 |
| `--primary` | `#18181b` | `#fafafa` | 主按钮与强调态 |
| `--primary-foreground` | `#fafafa` | `#18181b` | 主按钮文字 |
| `--destructive` | `#dc2626` | `#ef4444` | 驳回/错误提示 |
| `--success` | `#16a34a` | `#22c55e` | 已解锁/校对通过/已批准状态 |

## 2. 排版、圆角与图标
- **字体**：`Inter, -apple-system, BlinkMacSystemFont, "PingFang SC", "Noto Sans SC", sans-serif`；编号、令牌、JSON 预览与价格使用等宽字体 + `tabular-nums`。
- **圆角**：卡片 `8px`（`0.5rem`），按钮与输入框 `6px`（`0.375rem`），徽章 `9999px` 或 `6px`。
- **图标**：统一使用内联 **Lucide 风格 16px SVG**（`stroke-width="1.75"`），不使用 Emoji 充当 UI 图标。
- **阴影**：极轻克制阴影（`0 1px 2px rgba(0,0,0,0.05)`），靠 1px `--border` 与留白建立层级。

## 3. 信息架构与响应式
- **桌面端（≥960px）**：左侧固定 `248px` 导航侧栏（工作台 / 人类审核收件箱 / 插件与企业授权 + 底部企业授权状态卡片）+ 顶部 `52px` 面包屑与深浅色切换栏 + 右侧主内容区。
- **移动端（<960px）**：侧栏自动折叠为顶部紧凑导航，主区单列纵向排布。
- **可访问性**：所有交互按钮保留键盘焦点环（`outline: 2px solid var(--ring)`），实时反馈区使用 `aria-live="polite"`。
