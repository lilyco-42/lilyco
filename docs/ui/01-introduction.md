# 01 · 入门介绍

> 本章你会知道：`--gui` 到底是什么、为什么它长得不像一般后台、
> 它与 shadcn/ui 哪些地方同源、哪些地方故意不同。

---

## 1. 一个 struct，四个界面

`lilyco` 的起点是一个带 `#[derive(App)]` 的 struct：

```rust
#[derive(App)]
#[app(about = "压缩图片", run = "run_compress")]
struct ImgCompress {
    /// 输入图片
    #[arg(must_exist = true)]
    input: PathBuf,

    /// 质量 1–100
    #[arg(default = 75, range = 1..=100)]
    quality: u8,
}
```

框架把它派生到四个端：

```text
        ┌──────────── struct + schema ────────────┐
        │  name / about / args / kind / default   │
        └───────┬─────────┬─────────┬─────────┬───┘
                │         │         │         │
             CLI        TUI       Web        MCP
          clap 参数   表单+进度   本页      tools/list
```

`--gui` 打开的就是 **Web** 这一端。它不是另一套产品，只是同一份 schema 的第四个渲染器：

- 字段 → 控件（`ArgKind` 决定用哪个）；
- `about` → 标签与占位文案；
- `default` / `min` / `max` / `required` → 控件属性；
- 运行 → `POST /run`，进度 → SSE，收尾 → `done` / `error`。

所以：**你在 Rust 侧改 schema，页面自动跟着变**。改 UI 通常不是改 HTML，而是改 struct 上的注解。

---

## 2. 它是什么：本地开发者控制台

一句话定位：

> 给写工具的人和调工具的 agent 看的一张**软件工艺文档**，
> 不是给客户看的落地页。

这个定位直接推出页面结构：

| 区域 | 作用 | 不允许出现的东西 |
|---|---|---|
| 顶栏 | 品牌、命令切换、主题开关 | 导航菜单、用户头像、搜索框 |
| 卡片头 | 命令名 + 一句说明 | 插图、标语、渐变标题 |
| 表单 | 参数，一字段一行 | 分步向导、折叠面板 |
| 动作行 | 运行 / 取消 / 复制 CLI | 次要按钮抢主按钮位置 |
| 输出区 | 进度条 + 日志 + 结果 JSON | 图表、仪表盘、装饰性动画 |

判断一个改动该不该做，问一句：**它帮人更快确认「参数对不对、跑没跑完、结果是什么」吗？**
答案是否，就不属于这套系统。

---

## 3. 为什么长这样：shadcn 借了什么

shadcn/ui 最值钱的不是 React 组件，而是**把「高级感」拆成了可执行的规则**：
语义令牌、灰阶承担大部分视觉、1px 描边而不是阴影、克制的动效。
这部分 lilyco 全盘接受。

| 维度 | shadcn/ui | lilyco Web 控制台 |
|---|---|---|
| 交付形态 | 复制组件源码进你的项目 | 服务端渲染单页，内嵌进二进制 |
| 样式载体 | Tailwind 原子类 | CSS 变量 + 少量语义 class |
| 运行依赖 | React + Radix + Tailwind | **零**（原生 HTML/CSS/JS） |
| 组件来源 | 你手动挑、手动贴 | 由 `schema` 自动派生（`widget_*`） |
| 主题切换 | `.dark` class + 变量 | `[data-theme]` + `prefers-color-scheme`，存 `localStorage` |
| 令牌命名 | `background / foreground / card / primary …` | `canvas / surface / ink / control / hairline / accent …` |
| 图标 | lucide-react | Unicode 字符（`▶ ■ ⇪ ◐ ◆ ＋`） |
| 字体 | 可引 webfont | 只能用系统字体栈 |

**为什么不用 React**：页面要内嵌进每一个域二进制的 `.exe` 里，CSP 又禁止外链脚本与字体。
引入 React 意味着要么打破零依赖，要么把整个运行时塞进每个二进制。两者都不划算。

**那为什么还叫「shadcn 风格」**：因为 shadcn 的本质是**令牌 + 规则**，不是 React。
把同一套令牌投到 `lilyco-gui` 的 `app.css` 里，视觉语言是连续的；
要 React 时，两者用 [05 · React 映射](05-react-shadcn-mapping.md) 对齐。

---

## 4. 心智模型：四层

改任何东西之前，先确认自己在哪一层。跨层改动是这套页面出 bug 的主要来源。

```text
┌─ 1. 语义（Rust）────────────────────────────┐
│  #[derive(App)] struct / #[arg(...)]         │
│  产出：CommandSchema { name, about, args }   │
└───────────────┬──────────────────────────────┘
                │ render.rs 按 ArgKind 选 widget
┌─ 2. 结构（HTML）────────────────────────────┐
│  .card > .field > label + input              │
│  占位符：__FIELDS__ __ABOUT__ __CMD_NAV__ …  │
└───────────────┬──────────────────────────────┘
                │ app.css 只用令牌
┌─ 3. 表现（CSS）─────────────────────────────┐
│  :root 令牌 → [data-theme="dark"] 覆盖        │
│  状态一律用伪类：:hover :focus-visible …      │
└───────────────┬──────────────────────────────┘
                │ index.html 的 init* 组件
┌─ 4. 行为（JS）──────────────────────────────┐
│  一个组件一个 initXxx，全部登记进 BOOT        │
│  通信只走 updatePreview / readForm /         │
│  logLine / setProgress                       │
└──────────────────────────────────────────────┘
```

四条推论，每一条都对应一条硬规矩：

1. **语义层能表达的，不要写进 HTML**。参数说明写 `///` 文档注释，不要在页面里手写文案。
2. **结构层的名字是契约**。`#field-<arg>`、`data-target`、`.dz-status` 被测试与 JS 同时引用。
3. **表现层不许出现第二份颜色**。`app.css` 里每个十六进制值都必须能在 `DESIGN.md` 找到。
4. **行为层不许有内联事件**。`onclick=` 一律禁止，行为只写在 `initXxx` 里。

---

## 5. 六条设计公理

这六条是从 shadcn 与 Primer 里挑出来、并且在这套页面上被实测验证过的。
后面的组件页都以它们为前提。

1. **灰阶承担 90% 的视觉，颜色只做信号。**
   蓝只出现在：品牌标记、当前命令、焦点环、主按钮、进度条、链接。
   绿只表示「就绪 / 成功」这一种语义，不再是强调色。
2. **描边优先于阴影。**
   全页唯一允许的层次阴影是卡片（透明度 ≤ 0.06）；深色主题下直接 `none`，靠
   `canvas < surface < surface-2` 的色阶分层。
3. **8px 节奏，三档圆角。**
   间距只取 4/8/12/16/24/32/48；圆角只有控件 8px、面板 12px、胶囊 999px。
   出现 10px 或 16px 就是漂移。
4. **控件是 40px 高的。**
   输入框、下拉、主按钮统一 40px；图标按钮 34px；`btn-icon` 28px。
   输入框必须显式 `line-height: 1.4`，否则行高会把控件顶到 43px —— 「40px」就成了假数。
5. **组件层不看视口，看自己拿到的宽度。**
   `.card/.field/.actions/.out-head/.topbar` 都是 container，按 **自身宽度** 换行。
   页面可能被塞进 360px 的 OBS Dock，而视口是 1920px。
6. **关掉动画不等于关掉状态。**
   `prefers-reduced-motion` 下，不确定进度条改成「铺满 + 压淡」，
   主按钮 loading 改成文字后缀 `…` 并保持可读名。状态必须留下静态信号。

---

## 6. 三分钟验证

先确认你说的是同一套东西。

```bash
# 1) 看 schema（示例 crate 默认 headless，没有 --gui）
cargo run -p lilyco-example -- --input photo.jpg --schema

# 2) 起页面：模板已开 tui + web 特性
cd ../lilyco-template && cargo run -- --gui
```

页面打开后依次确认：

| 看哪里 | 应该是什么 |
|---|---|
| 顶栏 | `◆ lilyco` + 命令下拉（多命令才有）+ 右上 `◐` |
| 卡片 | 命令名 22px、说明 14px/1.7、字段一行一个 |
| 表单 | 输入框 40px 高；必填项标签带红星；数字框下方有「取值 1 – 100」 |
| 动作行 | 一颗强调色「▶ 运行」，其余是描边按钮；窄容器（≤420px）竖排占满 |
| 输出区 | 运行后出现；进度条 8px 胶囊；终端永远深底；结果区是等宽 JSON |
| 页脚 | `lilyco · 本地回环会话 · 令牌随进程轮换` |

按 `Tab` 走一遍：焦点环必须始终可见。切换系统深浅色：页面应跟随（若没手动设过主题）。

---

## 7. 接下来

- 想动手：读 [章节教程](tutorial/README.md)，从空 crate 写到四端可跑。
- 只想抄骨架：[速览版](02-tutorial.md)。
- 想改样式：读 [03 · 设计语言](03-design-language.md)，先弄清哪些值不能随便动。
- 想加控件：读 [组件参考](components/README.md) 的「加一个组件」清单。
- 想用 React：读 [05 · React 映射](05-react-shadcn-mapping.md)。