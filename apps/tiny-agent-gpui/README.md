# tiny-agent-gpui — 最小 GPUI 外壳

shadcn 风格 + Tokyo Night 默认主题的 agent 窗口骨架，用来验证
[`docs/UI-DESIGN.md`](../../docs/UI-DESIGN.md) 里的决策与令牌。

## 它展示了什么

* 三段式布局：**Sidebar（240px）/ Trace / Composer**
* 三套主题循环切换（`Tokyo Night → Neutral Light → Neutral Dark`），
  点击右上角按钮，颜色全部来自 `tiny_lilyco::ui::Theme`，**无硬编码色值**
* OODA trace 行：`step` + `phase` 徽章 + 工具名，卡片式浮起（`card` + `border`）
* 桌面能力徽章：默认显示 `desktop degraded`，因为 UDA 桥还没接线

## 跑起来

```bash
cd apps/tiny-agent-gpui
cargo run
```

依赖 GPUI 的 git 主干，首次构建较慢。建议先把 `Cargo.toml` 里的 `gpui`
钉到具体 `rev`，避免 API 漂移。

## 已知边界（诚实清单）

1. **UDA 已接线，但未真机验证**：启动时调用 `tiny_lilyco_uda::probe()`，用系统主题选
   默认主题，并在 Header 显示 `dark · N caps`。`UniDesktop/SDK` 的 API 已按 tag `v0.2.1`
   核对（见 `tiny_lilyco_uda/README.md`），真机行为由手动 CI 在 Windows/Linux 各跑一次。
2. **未使用 `gpui-kit`**：当前只用 GPUI 原语，便于先跑通。切到成套组件时把
   `Cargo.toml` 换成 `gpui-kit = "…"`（它 re-export `gpui`，组件 API 更全）。
3. **不是 workspace 成员**：靠 `[workspace]` 表与主 workspace 隔离，避免
   headless CI 被 GPU 依赖拖累。
4. **未在本地编译验证**：按项目规则禁止本机编译，此 crate 只由
   `.github/workflows/tiny-agent-gpui.yml`（手动触发）构建。

## 下一步

* 接 UDA：主题/通知/WakeLock → Header 徽章显示真实能力
* 接 `tiny_lilyco::adapters::browser_control`：EmbeddedWebview 面板
* Trace 行展开为 `Observation`（text / diff / console / image）
