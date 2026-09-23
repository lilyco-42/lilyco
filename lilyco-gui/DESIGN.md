# lilyco Web 控制台设计系统

Reading this as: 一台**本地开发者控制台**（一命令一表单 + 进度流 + JSON 结果），
给写工具的人和调工具的 agent 看，视觉语言要「软件工艺文档」而不是营销页。

本文件是**唯一一张表**。所有颜色/尺寸/节奏的取值以这里为准；`assets/app.css` 只允许
引用本表中的值，不允许出现第二处来源。改设计 = 先改本表。

---

## 0. 硬约束（决定下面每个选择）

| 约束 | 出处 | 后果 |
|---|---|---|
| 零外部依赖：无 CDN、无 webfont、无图标库 | `docs/WEBUI_EVALUATION.md` 的 Layui 退役决策 | 图标只能是 Unicode 字符；字体只能走系统栈 |
| CSP `default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; connect-src 'self'; img-src data:; base-uri 'none'` | `src/render.rs::index` | 任何外链（含字体、图片）都会被拦；`--gui` 是回环单机页 |
| 只监听 `127.0.0.1`，令牌每次启动随机 | `src/security.rs` | 页脚要如实写明「本地回环会话 · 令牌随进程轮换」 |
| 中英混排 | 仓库文档语言约定 | 正文行高按 CJK 走（1.7），UI 标签可紧（1.4） |
| DOM 钩子是契约 | `src/tests.rs` + 内嵌 JS | 见 §7，改样式不许动这些名字 |

## 1. 色彩令牌

品牌色取自仓库门面，不新造：`docs/banner.svg` 主蓝 `#2563eb`，`docs/logo.png` 近黑 `#181828` / 近白 `#f8f8f8`。
**单一强调色锁**：蓝只用于 brand mark、当前命令、焦点环、主按钮、进度条、链接。
绿色**不再是强调色**，只表示「就绪 / 成功」这一种语义。

### 1.1 浅色

| 语义 | 值 | 用在哪 |
|---|---|---|
| canvas | `#f6f7f9` | 页面底 |
| surface | `#ffffff` | 卡片、输入框底 |
| surface-2 | `#f1f3f5` | 次级面板、chip、CLI 预览底 |
| surface-3 | `#e7eaee` | 进度条槽、分隔块 |
| hairline | `#e0e3e8` | 卡片/表格装饰性分隔线（不受 1.4.11 约束） |
| control-border | `#7d8794` | **输入框、按钮描边、拖拽区虚线**（对 surface 3.64:1，对 canvas 3.4:1） |
| ink | `#16191d` | 主文本（对 surface 17.6:1） |
| ink-muted | `#5f6b7a` | 次文本、说明（对 surface 5.43:1） |
| accent | `#2563eb` | 强调（对 surface 5.17:1） |
| on-accent | `#ffffff` | 强调底上的字 |
| ok | `#0e7a55` | 就绪/成功（5.34:1） |
| warn | `#8a5a06` | 进行中/提醒（5.92:1） |
| danger | `#b3261e` | 错误、必填星号、取消（6.54:1） |

### 1.2 深色

| 语义 | 值 | 用在哪 |
|---|---|---|
| canvas | `#0d1014` | 页面底（不用纯黑，避免与纯白对撞刺眼） |
| surface | `#151a20` | 卡片 |
| surface-2 | `#1b2128` | 次级面板 |
| surface-3 | `#232b34` | 进度条槽 |
| hairline | `#28313b` | 装饰分隔 |
| control-border | `#5f6d7c` | 控件描边（对 surface 3.3:1） |
| ink | `#e7eaee` | 主文本（14.5:1） |
| ink-muted | `#9aa6b4` | 次文本（7.07:1） |
| accent | `#6ea1ff` | 强调（对 surface 6.83:1；浅底蓝在深底上不够亮，故提亮） |
| on-accent | `#0b1220` | 强调底上的字（深字浅底，7.32:1） |
| ok | `#3ddc97` | 就绪 |
| warn | `#f0b23a` | 进行中 |
| danger | `#ff7b72` | 错误 |

### 1.3 终端区（两套主题共用同一深底）

`#0d1117` 底 / `#e6edf3` 字；日志语义色 `err #ff7b72`、`warn #e3b341`、`tele #56d364`。
理由：日志是「机器的输出」，不随主题变，避免浅色主题下日志区像一块贴歪的纸。

## 2. 空间与响应式

`--s-1: 4px` `--s-2: 8px` `--s-3: 12px` `--s-4: 16px` `--s-5: 24px` `--s-6: 32px` `--s-7: 48px`。
所有 gap / padding / margin 只能取这些值；出现 10px、14px、18px 就是漂移，审计时按 §6 的机械检查扫掉。
阅读宽度 `max-width: 72ch`（约 720px 正文），整页 `860px`；≥1200px 时不放大字号，只放宽结果区。

### 2.1 响应式分两层，组件层不看视口

- **视口层**只管页面级留白与 iOS 输入字号（`max-width: 720px` / `420px`、`min-width: 721px` / `1200px`）。
- **组件层一律 container query**：`.card` `.field` `.actions` `.out-head` `.topbar` 声明
  `container-type: inline-size`，按钮组、拖拽区、chip、命令下拉按**自己拿到的宽度**决策。
  理由：这页会被塞进不同宽度的容器（窄侧栏、分屏、别的工具里），只看视口的话，
  一个 360px 宽的卡片里的按钮组照样挤成一团 —— 而视口可能是 1440px。
  需要 Chrome 105+ / Safari 16+ / Firefox 110+；每条组件规则另有视口兜底，老浏览器不会退化成坏布局。
- **flex 子项一律 `min-width: 0`**（输入框、chip、状态行、下拉）。默认值是 `auto`，
  一条 150 字符的 Windows 长路径就能把整行撑破 —— 这是本机控制台最常见的溢出源。

实测（同源页面里逐档改容器宽，量 `scrollWidth - clientWidth`）：320 / 360 / 420 / 560 / 860 / 1200
六档**卡片内零横向溢出**；按钮组 ≤420 竖排占满、≥560 回横排；CLI 预览处处换行不溢出。

## 3. 排版

字体栈（系统优先，CJK 就绪，无 webfont）：
- UI：`system-ui, -apple-system, "Segoe UI", "PingFang SC", "Hiragino Sans GB", "Microsoft YaHei", sans-serif`
- 等宽：`ui-monospace, "SF Mono", "Cascadia Code", Consolas, "Noto Sans Mono CJK SC", monospace`

| 角色 | px | weight | line-height | tracking | 用在哪 |
|---|---|---|---|---|---|
| title | 22 | 650 | 1.25 | -0.2px | 命令名（`card-title`） |
| about | 14 | 400 | **1.7** | 0 | 命令说明（长英文描述，CJK 行高兜底） |
| label | 13 | 600 | 1.4 | 0 | 字段标签 |
| body | 15 | 400 | 1.6 | 0 | 页面基准字号（`body`） |
| meta | 12.5 | 400 | 1.5 | 0 | 状态行、chip、页脚 |
| mono-data | 13 | 400 | 1.55 | 0 | CLI 预览、结果 JSON、日志 |
| overline | 11.5 | 600 | 1.4 | 0.8px | 输出区小标题（大写） |

规则：输入控件在 `max-width: 720px` 时字号提到 16px（iOS 聚焦会因小字号缩放整页），其余按本表。
另外 `color-scheme` 必须随主题声明（light / dark），否则深色主题下原生下拉箭头与滚动条仍是白底。

## 4. 圆角与高度（一套锁死）

`--r-control: 8px`（输入框、按钮、下拉）`--r-panel: 12px`（卡片、终端、结果）`--r-pill: 999px`（chip、进度条）。
控件高度：常规 **40px**（输入框与 `.btn`），紧凑 28px（`btn-icon`），图标按钮 34px。
输入框必须显式 `line-height: 1.4`：不锁的话它会继承 body 的 1.6，15/16px 字加 padding 把控件顶到 43px 以上，
「40px」就成了假数（这条是浏览器实测打脸后写进来的，36px 那版就是错在没量）。
**只有这三档圆角**；出现 10px/16px 即为漂移。例外只有两类非界面元素：滚动条滑块 4px、spinner 的 50% 正圆。

## 5. 动效

时长：微交互 150ms、状态切换 200ms、进场 300ms。只用 `transform` + `opacity`。
`@keyframes` 上限 3 个（rise / spin / indet）。
`prefers-reduced-motion: reduce` 时全部关闭（现有那条保留）。

## 6. 分层：hairline 优先于阴影

层次只靠三件事：`hairline` 描边、`surface` 与 `canvas` 的色差、8px 节奏的留白。
**层次阴影全页只允许一处**：卡片（`0 1px 2px` + `0 4px 16px`，透明度 ≤ 0.06；深色主题下直接置 `none`）。
焦点环用的是 `box-shadow: 0 0 0 3px` 扩散环，那不是层次阴影，属允许的第二个 `box-shadow` 用法。
禁止：卡片套卡片、给按钮/输入框加投影、玻璃拟态叠加模糊（顶栏那条 `backdrop-filter` 是唯一的例外，因为它压在滚动内容上）。

## 7. 不许改的名字（样式可以重写，这些不行）

- 占位符：`__CSS__` `__CMD_NAV__` `__FIELDS__` `__ABOUT__` `__CMD_NAME__` `__CMD_JS__` `__META__` `__TOKEN__`
- id：`field-<arg>` `up-<arg>` `chip-<arg>` `list-<arg>` `cmd-nav` `theme-toggle` `form` `run-btn` `cancel-btn` `copy-cli` `copy-result` `preview` `out` `log` `progress` `progress-bar` `result-wrap` `result`
- class：`dropzone` `dz-icon` `dz-hint` `dz-status` `dz-pick` `file-chip` `visually-hidden` `list-rows` `list-row` `list-add` `row-del` `flag-row` `field` `field-flag` `req-mark` `mono` `btn` `btn-icon` `primary` `ghost` `danger` `ok` `err` `busy` `drag-over` `uploaded` `loading` `card` `topbar` `page` `foot` `terminal` `cli-preview` `actions` `out-head` `result-head` `result-wrap` `brand` `brand-mark` `brand-sub` `icon-btn` `card-title` `card-about` `progress` `progress-bar`
- 属性：`data-target`（**裸参数名**）`data-must-exist` `data-pick` `data-file-for`（裸参数名）
  `data-max-upload`（**服务端注入的上传上限**，页面据此在读文件之前拦下超大文件）
  `data-list` `data-list-item` `data-list-add` `data-indet`
- `<meta name="lilyco-token">` 与请求头 `X-Lilyco-Token`

## 8. 不变量（今天真栽过的两类）

1. **平行表禁止**。同一事实只允许有一个来源。
   事故 A：`/upload` 的体积上限在 handler 里写 200 MB，而 axum 默认只放 2 MB，两条线各说各话 →
   超限上传只得到一句 `Failed to fetch`。现在 `MAX_JSON_BODY` 由 `MAX_B64_CHARS` 推导，并用
   `const _: () = assert!(...)` 在编译期钉住顺序。
   事故 B：`data-target` 发 `field-path`，JS 又拼 `field-` → `field-field-path` 取到 null，
   上传明明成功却报失败。现在 `data-target` 一律裸参数名，`src/render.rs` 里那条测试断言
   「JS 拼出来的 id 必须存在于页面」。
2. **改样式必须真在浏览器里量过，不是看过**。`cargo test` 看不见对比度，眼睛也看不见 360px 容器里的挤压。
   本表里每个数值都应有一次实测对应：控件高度那栏原来写 36px，实测 43.2px 才发现是没锁 `line-height`；
   对比度四组数（15.8 / 6.83 / 7.32 / 3.3）是从跑起来的页面 `getComputedStyle` 读回来核对的，不是手算的。
   验收清单：明/暗两套 → 逐档改容器宽（320/360/420/560/860/1200）量 `scrollWidth` →
   拖一个 >2 MiB 真文件走完「上传→回填→运行→进度→结果」→ `本机` 按钮渲染 → 取消 → 复制 → 命令切换。
   截图取不到时（in-app 浏览器表面 hidden）就读 DOM 几何与计算样式，别拿「应该没问题」交差。

## 9. 体积预算

`assets/index.html` ≤ 12 KB（现 10.4 KB），`assets/app.css` ≤ 16 KB（现 15.1 KB）。
CSS 从 10.7 涨到 15.1 的差额买的是 §2.1 那套组件级响应式与两套主题的完整令牌，
不是装饰。超预算要先说明换来了什么 —— 这页要**内嵌进每个域二进制的二进制里**，
每个 `--gui` 都带一份，所以预算是真的要守。

---

### 来源与致谢（MIT）

- 令牌文件结构与 DESIGN.md 格式：[VoltAgent/awesome-design-md](https://github.com/VoltAgent/awesome-design-md)（MIT）；
  其中 `design-md/linear.app/DESIGN.md` 的「单强调色 + hairline 分层 + 不用氛围色造节奏」三条纪律被本表采纳。
- 中性色阶与控件边界对比度的取法参考 [primer/css](https://github.com/primer/css)（MIT）。
- 「设计系统即数字 / 只此一张表 / 注释记下旧版本为什么错」的写法来自
  [cclank/lanshu-html2video-skill](https://github.com/cclank/lanshu-html2video-skill) 的
  `assets/template/src/lib/design.ts`。
- 本表**不引入**上述任何仓库的代码或依赖：CSP 与零依赖约束在前，数值与格式在后。
