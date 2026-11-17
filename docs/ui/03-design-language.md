# 03 · 设计语言

> 这一篇回答「为什么是这个样子」，以及「什么可以改、什么一改就散」。
> 具体色值查 [04 · 令牌参考](04-tokens.md)；组件结构查 [组件参考](components/README.md)。

---

## 1. 三条主线

整套视觉只靠三件事立住，其余都是从它们推导出来的：

1. **灰阶承担 90% 的视觉，颜色只做信号**
2. **描边优先于阴影**
3. **8px 节奏 + 三档圆角 + 40px 控件**

如果一次改动用上了第四种手段（渐变、玻璃拟态、投影、装饰性插画），
先问它是否在替代上面三条里的某一条 —— 通常是。

---

## 2. 颜色：单一强调色锁

### 2.1 蓝色只出现在六个地方

| 位置 | 用法 |
|---|---|
| 品牌标记 `◆` | 顶栏左侧，唯一的装饰 |
| 当前命令 | 命令标题、下拉选中态 |
| 焦点环 | 2px outline + 2px offset |
| 主按钮 | 「▶ 运行」的底色 |
| 进度条 | 确定态填充 |
| 链接 / `$` 前缀 | CLI 预览提示符 |

除此之外，蓝不用于：标题、表头、边框、图标、状态点、图表。
**一旦蓝色出现第七个用途，强调就不再是强调。**

### 2.2 状态色是语义，不是配色

| 令牌 | 含义 | 允许的场景 |
|---|---|---|
| `--ok` | 就绪 / 成功 | 上传完成、进度收尾、复制成功 |
| `--warn` | 进行中 / 提醒 | 上传中、取消请求已发送 |
| `--danger` | 错误 / 危险动作 | 校验失败、取消失败、删除行 hover、必填星号 |

绿色**不是**强调色。不要用 `--ok` 做「主要按钮」或「选中态」；
不要用 `--danger` 做「品牌红」。两者都是单一语义。

### 2.3 主题是换令牌，不是写反色

深色主题不是把浅色反相：

- 浅色靠 `canvas < surface`（`#f6f7f9 → #ffffff`）抬升；
- 深色靠 `canvas < surface < surface-2`（`#0d1014 → #151a20 → #1b2128`）分层；
- 深色下强调色要**提亮**（`#2563eb → #6ea1ff`），否则在深底上对比度不够；
- 深色下卡片阴影置 `none`。

切换机制有三个入口，优先级从高到低：

```text
html[data-theme="dark"|"light"]   ← 用户点过 ◐，存 localStorage["lilyco-theme"]
@media (prefers-color-scheme)     ← 跟随系统
:root                             ← 兜底浅色
```

`color-scheme` 必须随主题一起声明（`light` / `dark`），
否则深色主题下原生下拉箭头与滚动条仍是白底。

---

## 3. 层次：hairline 与 control 分工不同

这是最容易改错的一处。

| 令牌 | 用在哪 | 对比度要求 | 实测 |
|---|---|---|---|
| `--hairline` | 卡片边、面板边、装饰分隔 | **不承担** WCAG 1.4.11「认出组件所需的边界」，故意不过 3:1 | 对卡片底约 1.3 |
| `--control` | 输入框 / 按钮 / 下拉的描边、拖拽区虚线 | 必须过 3:1 | 浅 3.64 / 深 3.30 |

所以：**不要因为审计工具说 hairline 对比度不足就把它调深**。
调深之后卡片边界会开始抢内容，层次反而变乱。需要强边界的是控件，不是容器。

层次只允许三种手段：

1. hairline 描边；
2. `surface` / `canvas` 的色阶差；
3. 8px 节奏的留白。

阴影只有一处例外：卡片（`0 1px 2px` + `0 4px 16px`，透明度 ≤ 0.06，深色下 `none`）。
焦点环的 `box-shadow: 0 0 0 3px` 是状态扩散环，不算层次阴影。

禁止：卡片套卡片、按钮投影、输入框内阴影、多图层毛玻璃。
顶栏的 `backdrop-filter` 是唯一例外 —— 它压在滚动内容之上，有功能理由。

---

## 4. 空间与尺寸

### 4.1 间距只有七个值

```text
--s-1: 4px   --s-2: 8px   --s-3: 12px  --s-4: 16px
--s-5: 24px  --s-6: 32px  --s-7: 48px
```

出现 10px、14px、18px、20px 就是漂移。例外是非布局用途（滚动条滑块 4px）。

### 4.2 版心

| 宽度 | 正文 | 卡片 | 结果区 |
|---|---|---|---|
| ≥1200px | 不放大字号 | `max-width: 1040px` | 限高内滚（`min(560px, 70vh)`） |
| 721–1199px | `72ch` | `860px` | 正常 |
| ≤720px | 输入框字号提到 16px | 满宽 | 正常 |
| ≤420px | 页面留白收到 `--s-3` | 满宽、`--s-4` 内边距 | 正常 |

**宽屏只放宽结果，不放大字号。** 这是控制台不是营销页，字大不等于好用。

### 4.3 控件高度锁死

| 控件 | 高度 | 备注 |
|---|---|---|
| 输入框 / 下拉 / `.btn` | 40px | 必须显式 `line-height: 1.4` |
| `.icon-btn`（主题按钮） | 34px | 正方形 |
| `.btn-icon`（选择文件、添加、复制） | 28px | 紧凑动作 |

输入框不锁 `line-height` 会继承 body 的 1.6，15/16px 字加 padding 直接顶到 43px，
「40px」就成了假数。这条是实测打脸后写进规范的。

### 4.4 圆角只有三档

```text
--r-control: 8px    输入框、按钮、下拉、CLI 预览
--r-panel: 12px     卡片、终端、结果区、拖拽区
--r-pill: 999px     chip、进度条
```

10px / 16px 一律算漂移。非界面元素例外：滚动条滑块 4px、spinner 正圆。

---

## 5. 排版

字体栈只用系统字体（无 webfont）：

```text
UI:   system-ui, -apple-system, "Segoe UI", "PingFang SC", "Hiragino Sans GB",
      "Microsoft YaHei", sans-serif
等宽:  ui-monospace, "SF Mono", "Cascadia Code", Consolas,
      "Noto Sans Mono CJK SC", monospace
```

| 角色 | px | weight | line-height | 用在哪 |
|---|---|---|---|---|
| title | 22 | 650 | 1.25 | 命令名 |
| about | 14 | 400 | **1.7** | 命令说明（中英混排按 CJK 行高） |
| label | 13 | 600 | 1.4 | 字段标签 |
| body | 15 | 400 | 1.6 | 页面正文基准 |
| meta | 12.5 | 400 | 1.5 | 状态行、chip、页脚 |
| mono-data | 13 | 400 | 1.55 | CLI 预览、结果 JSON、日志 |
| overline | 11.5 | 600 | 1.4 + 0.8px 字距 | 「输出」「结果」小标题（大写） |

两条附加规则：

- 视口 ≤720px 时输入框字号提到 **16px** —— iOS 上小于 16px 会在聚焦时缩放整页；
- 中英混排时正文按 1.7，UI 标签可以紧到 1.4，二者不要互相借。

---

## 6. 响应式：两层，组件不看视口

```text
视口层（@media）      只管页面留白、字号、iOS 输入缩放
组件层（@container）  按钮组、拖拽区、chip、命令下拉按「自己拿到的宽度」决策
```

为什么要 container query：这个页面会被塞进不同宽度的容器（OBS Dock、分屏、iframe）。
视口可能是 1920px，但卡片只有 360px —— 只看视口的话，按钮组照样挤成一团。

已声明的容器：`.card` `.field` `.actions` `.out-head` `.topbar`。

断点与行为：

| 容器宽度 | 行为 |
|---|---|
| ≤560px | 动作行允许换行，按钮 `flex: 1 1 140px`、居中 |
| ≤420px | 动作行改竖排、每个占满；输出头换行、进度条独占一行；命令下拉换到第二行 |
| ≥721px | 动作行不换行；chip 限制 45% 宽并靠右 |

两个必须记住的实现细节：

1. **flex 子项一律 `min-width: 0`**。默认 `auto` 时，一条 150 字符的 Windows 路径就能把整行撑破。
2. **竖排时别让基准值变成高度**。`flex: 1 1 140px` 在 `column` 下会把 140px 解释成高度，
   实测按钮被顶到 140px 高 —— 竖排那一档必须改回 `flex: 0 0 auto`。

---

## 7. 动效

| 类型 | 时长 | 手段 |
|---|---|---|
| 微交互（hover / active） | 150ms | `border-color`、`background`、`transform: scale(.98)` |
| 状态切换 | 200ms | `background`、`color`、进度条 `width` |
| 进场 | 300ms | `opacity` + `translateY(6px)` |

`@keyframes` 全页最多 3 个：`rise` / `spin` / `indet`。
不用装饰性动画：没有视差、没有持续脉冲、没有循环渐变。

### reduce 动效的三条硬要求

`prefers-reduced-motion: reduce` 下，动画停掉之后**状态不能跟着消失**：

- 不确定态进度条：原本 40% 宽来回跑，停掉会停在半路读成「卡在 40%」
  → 改为**铺满 + `opacity: .5`**，意思是「在跑，不知道到哪了」；
- 主按钮 loading：原本转圈 + 标签 `visibility:hidden`，停掉动画只剩空壳
  → 圆环 `display:none`，标签恢复可见并追加 `…`，同时保留 `aria-busy="true"`、`disabled`、`cursor: progress`；
- 注意 `*` 选不中伪元素：想真的停 `::after` 的动画，得点名它自己。

---

## 8. 无障碍

这套页面的无障碍要求是**结构性的**，不是补 ARIA：

| 要求 | 做法 |
|---|---|
| 焦点永远可见 | 控件 `outline: 2px solid var(--accent); outline-offset: 2px`；面板内元素用 `-2px` 内描边 |
| 标签与控件绑定 | `<label for="field-<arg>">`；Flag 用整行 `<label class="flag-row">`，不重造 ARIA |
| 状态可被读到 | `role="status"` + `aria-live="polite"`（上传状态行）、`role="log"` + `aria-live="polite"`（终端） |
| 区间要被念出来 | Number 的 `min`/`max` 既进属性，也渲染成 `.field-hint` 并用 `aria-describedby` 挂上；单边区间也要说 |
| 可滚动区可键盘滚 | 终端与结果区 `tabindex="0"`（WCAG 2.1.1），并给 `:focus-visible` 内描边 |
| 进度不骗读屏 | 没有比例时不写 `aria-valuenow` |
| 复制必有反馈 | 成功/失败都改文字与颜色；失败要能重试 |
| 按钮不承诺做不到的事 | 取消按钮只在服务端真能取消时才出现 |

对比度目标：正文 ≥ 7:1，次要文本 ≥ 4.5:1，控件描边 ≥ 3:1。
实测值见 [04 · 令牌参考](04-tokens.md)。

---

## 9. 改之前先看闸门

仓库里有 10 条测试盯着这套设计。它们不是形式主义，每条都对应一次真实事故。

| 测试 | 守住什么 | 违反后的真实症状 |
|---|---|---|
| `every_css_color_is_in_the_design_table` | CSS 里每个色值都能在 DESIGN.md 找到 | 出现第二份颜色，暗色主题下某块发白 |
| `assets_stay_within_the_documented_budget` | 资源字节上限 | 二进制体积失控 |
| `no_data_attribute_goes_unread` | 每个 `data-*` 都有人读 | 加了属性没人用，后来者以为它是契约 |
| `boot_is_the_last_thing_in_the_script` | 装配在脚本最后 | TDZ 报错，四个组件静默失效 |
| `every_js_component_init_is_bootted` | 每个 `initXxx` 进了 BOOT | 写了组件但没装配 |
| `inline_script_is_syntactically_valid` | 内联脚本能解析 | 语法错但页面照常打开 |
| `reduced_motion_keeps_a_non_animated_state_signal` | reduce 下状态仍在 | 关掉动画后用户看不到「在跑」 |
| `the_cancel_button_only_promises_what_the_server_can_do` | 取消按钮与服务端能力一致 | 按钮点了只回 404 |
| `number_bounds_are_spoken_whichever_side_they_come_from` | 单边区间也提示 | 只设 `min` 的参数读屏一个字都不报 |
| `list_rows_follow_their_item_kind` | 列表行按 item 类型画 | `Vec<u32>` 删空后变回文本框，提交被拒 |

---

## 10. 反模式清单

改 UI 之前对照一遍，命中任何一条就先停下：

- 用阴影做层次（除卡片）；
- 第二处出现主色；
- `10px` / `16px` 间距或圆角；
- 在组件规则里写十六进制颜色；
- 给输入框或按钮加投影；
- 用渐变、玻璃拟态、发光边框；
- 用 webfont 或图标库；
- 把按钮组的高度基准当宽度用（竖排时）；
- 内联 `onclick=` / `onchange=`；
- 新增 `data-*` 但没有读者；
- 只在 1440px 下看效果，不测 320px；
- 用 `<div role=button>` 包住多个控件（拖拽区就是反例，见组件页）。

---

想动手改一遍：跟着 [第 5 章 · 让页面像产品](tutorial/05-make-it-product.md) 走。