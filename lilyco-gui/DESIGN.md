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
| DOM 钩子是契约 | `src/render.rs` 的测试 + 内嵌 JS | 见 §7，改样式不许动这些名字 |

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

实测（2026-09-23，`srcdoc` 同域 iframe 造真实视口，明暗两套各跑一遍）：
320 / 390 / 420 / 560 / 720 / 1024 / 1440 / 1920 八档**整页零横向溢出**；
输入框、下拉、按钮在每一档都是 40px 高（`@container ≤420` 竖排占满那一档曾经把按钮顶到 140px 高 ——
`flex: 1 1 140px` 的主轴在 `flex-direction: column` 下是高度，收在 `@container ≤420` 里改回 `0 0 auto`）。

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

**关掉动画不等于关掉状态。** `prefers-reduced-motion: reduce` 那条把动画全停之后，
两个「只靠动来说话」的组件会静悄悄失去状态，所以各留了一份静态替代：

- 进度条不确定态：动画一停，40% 的条会停在半路，读起来像「卡在 40%」→ 改成**铺满 + 压淡**（`opacity .5`），
  意思是「在跑，不知道到哪了」；确定态的百分比本来就在宽度里，不受影响。
- 主按钮 loading：`* { animation: none !important }` 打不到伪元素 —— 改之前实测
  `getComputedStyle(btn,'::after').animationName` 在 reduce 环境下仍然是 `spin`，也就是**根本没遵守
  用户的偏好**，转圈照转；同时标签被 `visibility:hidden` 藏着，读屏与低视力用户只剩一个会转的空壳。
  现在 reduce 下 `::after` 是 `display:none`（圈没了），标签 `visibility:visible` 并加后缀 `…`，
  `aria-busy="true"`、`disabled`、`cursor:progress` 一起到位 —— 既不动也不空。
  （`走 setBusy(true)` 这条真实代码路径量的，不是手动加 class。）

这两条由 `render.rs` 的 `reduced_motion_keeps_a_non_animated_state_signal` 盯着别被删。
顺带记一句教训：`*` 选不到 `::before` / `::after`，想真的「减少动效」得点名伪元素自己。
实测（2026-09-23，这台浏览器环境恰好开着「减少动态效果」，所以是能真量的那一档）：
不确定态读回 `width 394px / track 394px`、`opacity 0.5`、`animation-name none`；
同一元素给到确定值 `setProgress(0.42)` 时又回到 `165px`（= 42%）、`opacity 1`、`aria-valuenow=42`，
说明静态替代只接管不确定态，没有漏进确定进度。CSSOM 里两条同名规则各自的位置也对：
根作用域那条还是 `width:40% + animation:indet 1.2s`（正常动效环境用的），reduce 块里才是铺满那条。
`loading` 按钮在同一环境下读回 `visibility: visible` + `::after " …"`。

## 6. 分层：hairline 优先于阴影

层次只靠三件事：`hairline` 描边、`surface` 与 `canvas` 的色差、8px 节奏的留白。
**层次阴影全页只允许一处**：卡片（`0 1px 2px` + `0 4px 16px`，透明度 ≤ 0.06；深色主题下直接置 `none`）。
焦点环用的是 `box-shadow: 0 0 0 3px` 扩散环，那不是层次阴影，属允许的第二个 `box-shadow` 用法。
禁止：卡片套卡片、给按钮/输入框加投影、玻璃拟态叠加模糊（顶栏那条 `backdrop-filter` 是唯一的例外，因为它压在滚动内容上）。

## 7. 不许改的名字（样式可以重写，这些不行）

- 占位符：`__CSS__` `__CMD_NAV__` `__FIELDS__` `__ABOUT__` `__CMD_NAME__` `__CMD_JS__` `__META__` `__CANCEL_JS__` `__TOKEN__`
- id：`field-<arg>` `up-<arg>` `chip-<arg>` `hint-<arg>` `list-<arg>` `cmd-nav` `theme-toggle` `form` `run-btn` `cancel-btn` `copy-cli` `copy-result` `preview` `out` `log` `progress` `progress-bar` `result-wrap` `result`
- class：`dropzone` `dz-icon` `dz-hint` `dz-status` `dz-browse` `dz-pick` `file-chip` `visually-hidden` `list-rows` `list-row` `list-add` `row-del` `flag-row` `field` `field-flag` `field-hint` `req-mark` `mono` `btn` `btn-icon` `primary` `ghost` `danger` `copied` `copy-failed` `ok` `err` `busy` `drag-over` `uploaded` `loading` `card` `topbar` `page` `foot` `terminal` `cli-preview` `actions` `out-head` `result-head` `result-wrap` `brand` `brand-mark` `brand-sub` `icon-btn` `card-title` `card-about` `progress` `progress-bar`
- 属性 —— **谁读它**写清楚，不写清就是没人读，`no_data_attribute_goes_unread` 会红：
  - JS 读：`data-target`（**裸参数名**，不是 id）`data-max-upload`（**服务端注入的上传上限**，
    页面据此在读文件之前拦下超大文件）`data-pick` `data-list` `data-list-item` `data-list-add`
    `data-item`（List 容器：提交时按它换算类型）`data-item-kind`（每行一份：复制/清行按它挑分支）
    `data-indet` `data-state`（这两个由 JS 写、CSS 读，是状态选择器不是数据）
  - 只有测试与调试读：`data-component`（组件自报家门；JS 装配靠的是 `BOOT` 清单里的 `init*`，
    **不是**这个属性 —— 别再拿它当行为层的锚点）
  - 已删（画了半年没人读）：`data-browse`（参数名从祖先 `.dropzone[data-target]` 就拿得到）、
    `data-must-exist`（`must_exist` 的落点是拖拽区那句提示文案，不是属性）、
    `data-placeholder`（只有那段被删掉的兜底建行代码读过）
- `<meta name="lilyco-token">` 与请求头 `X-Lilyco-Token`
- **不许有内联事件**：`onclick=` / `onchange=` / `onsubmit=` / `oninput=` / `onload=` 一律不能出现在
  服务端吐的 HTML 里，行为只写在 `assets/index.html` 的 `init*` 中（`render.rs` 有一条测试逐名扫）。
  理由：markup 与行为各说各话是这套页面出过的每一类「点了没反应」的根因。

## 8. 不变量（真栽过的六类）

1. **平行表禁止**。同一事实只允许有一个来源。
   事故 A：`/upload` 的体积上限在 handler 里写 200 MB，而 axum 默认只放 2 MB，两条线各说各话 →
   超限上传只得到一句 `Failed to fetch`。现在 `MAX_JSON_BODY` 由 `MAX_B64_CHARS` 推导，并用
   `const _: () = assert!(...)` 在编译期钉住顺序。
   事故 B：`data-target` 发 `field-path`，JS 又拼 `field-` → `field-field-path` 取到 null，
   上传明明成功却报失败。现在 `data-target` 一律裸参数名，`src/render.rs` 里那条测试断言
   「JS 拼出来的 id 必须存在于页面」。
2. **改样式必须真在浏览器里量过，不是看过**。`cargo test` 看不见对比度，眼睛也看不见 360px 容器里的挤压。
   本表里每个数值都应有一次实测对应：控件高度那栏原来写 36px，实测 43.2px 才发现是没锁 `line-height`；
   对比度与 §10 的状态是从跑起来的页面 `getComputedStyle` 读回来核对的，不是手算的。
   视口层怎么量：把当前页面 `outerHTML` 塞进一个同域 `srcdoc` iframe，改 iframe 的宽，
   里面的 `@media` 与 container query 会按真实视口生效（320/390/420/560/720/1024/1440/1920 全档）。
   截图取不到时（in-app 浏览器表面 hidden）就读 DOM 几何与计算样式，别拿「应该没问题」交差。
3. **装配那一行必须在脚本最后**。组件函数读顶层 `const`，提前跑就撞 TDZ：
   `quoteIfSpaced` 声明在 `BOOT` 循环之后 → `initCliPreview` 抛 `ReferenceError` →
   拖拽区、List、运行、结果四个组件根本没装配，页面看着完全正常、点了不动，
   而 `cargo test` 与 `node --check` 全都是绿的（语法没错，是执行顺序错）。
   现在三样一起兜：`BOOT` 挪到文件末尾、每个 `init` 单独 try 且失败写进日志区、
   `render.rs` 里 `boot_is_the_last_thing_in_the_script` + `every_js_component_init_is_bootted`
   + `inline_script_is_syntactically_valid` 三条测试盯着。
   兜底本身也实测过：把页面 `outerHTML` 里 `.dz-browse` 那颗按钮抹掉再装进 srcdoc iframe，
   帧里读回 `#out` 已露出 + 一行 `line err`「组件装配失败，页面功能不完整：initDropzone:
   Cannot read properties of null…」，而 `#preview` 仍有内容 —— 也就是一个组件的毛病被关在自己的
   init 里，其余九个照常装配。对照组（没动过的 `outerHTML`）`#out` 保持 `hidden`、日志为空。
4. **`GET /` 必须 `no-store`**。整页自包含且随进程变（schema、令牌、重编译后的资产）。
   踩过：改了页面 JS 重编重跑，浏览器还在发上一版，半数组件是死的，第一反应是「新代码有 bug」。
5. **「一次只跑一个」的闸门必须同步生效**。`run()` 里那句 `if(es)return` 挡不住连点：`es` 要等
   `POST /run` 的 fetch 回来才赋值，而运行按钮虽然被 `setBusy(true)` 禁了，**表单自己不管**——
   在输入框里按回车照样提交。实测连发三次 `requestSubmit()` 就真出去三个 `/run`（对 T1/T2 命令
   就是三次副作用）。现在闸门是 `$("run-btn").disabled`：`setBusy(true)` 在任何 `await` 之前同步发生，
   所以第二次起必被挡；同一处实测跑完一轮后再提交仍能发出第二个请求（闸门会松开，不会锁死页面）。
6. **按钮只能许诺服务端真做得到的事**。`POST /cancel/{sid}` 只在「执行路登记了取消句柄」时才找得到会话，
   而句柄只有 `Registry` 那条路（`run::run_progress`）会登记。`serve_app` 以前走自定义 `RunnerFn`，
   于是**每一个** `#[app]` 单命令二进制的「取消」都只回一句 404 —— 页面看着给了这个能力，
   Rust 侧所有测试却全绿（没人去点那颗按钮）。现在两件事一起钉住：`serve_app` 内部就是单命令注册表
   （`run.rs` 的 `registry_run_registers_a_cancel_handle_and_clears_it` 盯着登记与清理），
   页面按 `__CANCEL_JS__` 决定画不画这颗按钮（`the_cancel_button_only_promises_what_the_server_can_do`
   与 `runner_fn_sessions_cannot_be_cancelled` 盯着两侧）。
   实测改前后 `lbrush --gui` 的页面形状没变（一命令页仍无命令下拉），只是 `CANCELABLE` 从假变真。
   同理，`setBusy` 除了 `hidden` 还要复原 `disabled` —— `cancel()` 按下后会自禁，
   不复原的话第二次运行时它是「看得见但点不动」。
   反过来也成立：`run()` 刚按下时 sid 还没回来，那时点「取消」真的什么都没发生，
   所以那颗按钮要等 sid 到手才解禁。实测：提交后 10 ms 时它是 `visible + disabled`，
   此时点它一个请求都不发（`fetch` 计数没动）；有 sid 之后点它，404 会原样说出
   「取消失败：no such running session」并把按钮解禁好让你再点一次，
   200 则说「已发送取消请求（命令需响应 ctx.is_cancelled）」并保持禁用（空点击比禁用的按钮更糟）。

## 9. 体积预算

| 文件 | 重设计前 | 现在（2026-09-23 实测） |
|------|---------|------|
| `assets/index.html`（含内联 JS） | 10,377 B | 20,751 B |
| `assets/app.css` | 10,314 B | 21,447 B |
| 一次 `GET /` 的响应 | 未量过旧版（两个资源文件之和 20,691 B） | 44,605 B（`lbin` 的 identify 表单实测字节数） |

**上限只写在测试里**：`src/render.rs` 的 `assets_stay_within_the_documented_budget` 是唯一闸门；
这里再抄一遍两个数字，就成了 §8.1 说的那种「需要人记着的第二张表」。它量的是**提交进仓库的字节**
（先把 `\r\n` 归一回 `\n`）—— Windows 按 `core.autocrlf` 检出会让同一份文件在磁盘上每行多一个 `\r`，
2026-09-23 闸门就因此在 CI 上红过一次（本地 20,751 B / CI 21,192 B）。预算管的是资源本身，
不是某台机器的检出方式；顺带记着：Windows 构建出来的 `.exe` 里嵌的就是那份带 `\r` 的字节。
涨过线编译期就红 —— 本次实测 HTML 离上限只剩几百字节，
下一个组件大概会撞上去，那就是该坐下来决定「删什么或凭什么涨」的时刻（2026-09-23 复核那批
修复真撞过一次：靠把新加的注释压回一行解决，理由留在本文件与 §8/§10，页面里不重复）。
响应比两个文件之和还大，是因为骨架里的 `__FIELDS__` / `__CMD_NAV__` / `__ABOUT__` / `__META__`
都会换成真内容；按字符数报会少算约 5 KB（CJK 一个字三个字节），所以这张表一律记字节。
涨的这 21 KB（资源文件从 20,691 B 到 41,994 B）买的是：两套主题 × 全令牌、§2.1 的组件级响应式、
§10 的七态与无障碍结构
（真按钮、焦点环、live region、`aria-describedby`），以及行为层从「一坨顺序脚本」拆成 10 个可装配组件。
这页要**内嵌进每个域二进制的 `.exe` 里**，所以预算是真的要守 —— 但它是本机回环上的单个响应，
不过网、不分片、无外部请求，涨的是磁盘不是等待。再要涨得先说清换来什么。

## 10. 组件目录

一个 `ArgKind` 一个组件函数（`src/render.rs` 的 `widget_*`），页面侧一个组件一个 `init*`
（`assets/index.html`）。**改组件要同时改这张表**：`data-component` 少了谁、
`BOOT` 漏装配了谁，测试会红；这张表负责对账「有没有七态、用了哪些令牌」。

下表的对比度全部是 2026-09-23 从跑起来的页面 `getComputedStyle` 读回来再算的比值
（明 / 暗，320–1920 全档无横向溢出）。
复核那批修复落地后，在**最终资产**上又整页跑了一遍同样的 16 档（明/暗 × 320/360/420/560/720/1024/1440/1920，
带一次真实运行的日志与结果）：所有可见组件（每个 `data-component` 节点 + 卡片 / 页脚 / 动作行 /
进度条 / 终端 / 结果区 / 拖拽区 / 提示行 / chip）零越界、零「宽度为 0」，进度条有宽度，
两套主题的 `body` 底色确实不同（`#f6f7f9` / `#0d1014`）—— 零违规。

### 10.1 六种参数控件

| 组件 | 构成 | 七态 | 令牌 | 无障碍 |
|------|------|------|------|--------|
| `flag` | `<label class="flag-row">` + 原生 checkbox | hover（原生）/ focus（`input:focus-visible` 环）/ disabled\* / checked（=success，原生勾选）/ loading·empty·error：n/a（开关没有中间态） | `--accent`（`accent-color`）`--s-2` | 标签即 `label for`，整行可点；不用 ARIA 重造 |
| `text` | 单个 `<input type=text>` | hover 描边压深 / focus accent + 3px 环 / disabled\* / error（`:user-invalid` 描边转红）/ empty（除占位文案外没有别的状态）/ loading·success：n/a（一行输入没有中间态，成没成由运行自己说） | `--control` 3.64·3.30 / `--ink` 17.63·14.50 / `--danger` 6.54·6.94 | `label for`；`required` 用原生属性；16px 字号（≤720 视口）防 iOS 聚焦缩放 |
| `number` | `<input type=number>` + `.field-hint` | 七态与 `text` **同名同法**（hover / focus / disabled / error / empty / loading / success 都走 `text` 那一行的定义，取值也不另立），此外的确有一条自己的：`:out-of-range`（实测填 999 进 `1–100` 的框，`matches(":out-of-range")` 为真，描边由这条规则转红） | 同上 + `--ink-muted` 5.43·7.07 | `min`/`max` 进属性 **且** 写成可见提示；`aria-describedby="hint-<arg>"` 让读屏念得出区间。**单边也要写**（只有 `min=0` 时以前一个字都不报，`number_bounds_are_spoken_whichever_side_they_come_from` 盯着；无区间就不画提示行）。真页面核对过：`lbin entries` 的 `limit` 只有 `min=1`，画出来就是 `取值 >= 1` 且 `aria-describedby` 指向的节点确实存在 |
| `enum` | `<select>` + `<option selected>` | hover / focus / disabled\* / empty：n/a（值域非空，选得出的就是合法值）/ error：n/a（选不出非法值）/ loading·success：n/a | `--control` / `--ink` | 原生键盘可用；40px 高与其它控件对齐（同一档 `--s-*` 节奏） |
| `path` | 手填 `<input>` + `.dropzone` 容器（三件套） | hover / focus（里面每个控件各自）/ disabled（`本机` 在 `POST /pick` 期间由 JS 置位）/ loading = busy（`.dz-status.busy`，上传与等系统框时）/ success（`.uploaded` + `ok`，实测拖一个 600 B 文件走完全程）/ error（`.dz-status.err` + 整区 `:has()` 转红）/ empty（`.dz-hint`） | `--accent` 5.17·6.83 / `--warn` / `--ok` / `--danger` / `--accent-soft` | 状态行 `id="up-<arg>"` `role=status` `aria-live=polite`，并挂在路径框的 `aria-describedby` 上；上传结果不必聚焦也能读到 |
| `list` | `.list-rows` + N 行（**行的控件按 item 类型**：Text→文本、Number→带区间的数字框、Enum→下拉、Path→等宽文本、Flag→复选框）+ `.list-add` | hover / focus（`.btn-icon` 环）/ disabled\* / loading：n/a（行是本地增删，没有任何等待）/ empty = 删到最后一行时按模板补一行（实测连删 5 次仍留 1 行，且补出来的仍是**数字框**）/ error·success：n/a（逐行无独立反馈，对错由服务端校验在运行时说） | `--s-2` 节奏 / `--danger`（删除 hover） | 行控件 `aria-label` 取参数说明；新增行 `focus()` 跟上；新行是**克隆 `initList` 存下的服务端模板行**（`ROW_TMPL`），清行按 `data-item-kind` 分支。以前删空后走的是一段手搓的兜底建行代码 —— 于是 `Number` / `Enum` 的行删完变成文本框（JS 里不许有第二份「item→控件」映射，同 §8.1） |

> **List 的 item 类型现在只到 `Text` 为止**：`#[derive(App)]` 取 List 的值时一律
> `arr.iter().filter_map(as_str).collect()`，于是 `Vec<u32>` / `Vec<Enum>` 直接编译不过
> （`Vec<u32>` 不能从 `String` 迭代器构造），而它生成的 schema 却把 item 声明成
> `Number{min:None,max:None}`。也就是说：Web 侧已经按 schema 会把数字行、下拉、复选框画对了
> （`list_rows_follow_their_item_kind` 盯着），但只有 `Vec<String>` 真能跑通端到端。
> 剩下那半在 `lilyco-macros` 的取值分支里，属于四端共同的问题，不是控制台单独的毛病 ——
> 别把它当成 Web 的 bug 修在这里。
> 验证到能验的那一层：在跑起来的页面里挂一个 `data-item="Number"` 的列表容器，
> `addListRow` 复制出来的行确实是 `type=number`，`listValues` 回 `[7,9]` 且 `typeof` 两个都是
> `number`（不是 `"7"`）；端到端那一档要等宏修好才有真参数可跑。
> 2026-09-23 改成模板克隆之后同一层再测一遍：把两行数字删空，补回来的那一行仍是
> `INPUT:number` + `data-item-kind="Number"`（删空退化成文本框就是这次的修复点），
> Enum 行克隆出来 `selectedIndex` 归 0、Flag 行归未勾 —— 清行只认 `data-item-kind`。

**同一批最终资产上的状态重测（2026-09-23）**：`flag` / `text` / `enum` / `number` / `path` 五块在
键盘模态下 `matches(":focus-visible")` 为真、环是 `box-shadow: 0 0 0 3px rgba(110,161,255,.16)`；
`disabled` 五块都读回 `opacity 0.55`（`text`/`enum`/`number`/`path` 配 `cursor:not-allowed`，
checkbox 由 UA 给 `pointer`）；`input[type=text]:hover, input[type=number]:hover, select:hover`、
`input[type=number]:out-of-range`、`input:user-invalid` 三条规则都读自 `document.styleSheets`（135 条规则里）。
`number` 的单边区间在真页面上读出：`lbin entries` 的 `limit` 只有 `min=1` → 提示 `取值 >= 1`。

\* `disabled` 的**样式**实测到位（`run` / `btn-icon` / `input` 三者都读回 `opacity:0.55` +
`cursor:not-allowed`），但参数控件这一层暂时没有**触发方**：schema 里还没有「只读参数」这种东西。
三颗按钮（`运行` / `取消` / `本机`）是真的会置灰 —— 前两颗由 `setBusy`/`cancel` 驱动，
`本机` 在 `POST /pick` 期间驱动（那一下会弹系统对话框，故意没在自动化里点）。
其中 `取消` 还要**先问服务端做得到做不到**：`__CANCEL_JS__` 为 false（自定义 RunnerFn）时它根本不出现，
见 §8.6。参数控件留着 `disabled` 这条规则是控件基线的一部分：哪天真出现只读参数，直接复用，别另起一行。

### 10.2 拖拽区（`path` 里最重的一个，单独说）

`.dropzone` 是**容器不是按钮**：里面有 `选择文件`、`本机`（开了 `pick` 特性才有）、`chip` 三个各管各的控件，
外层挂 `role="button"` 会让一次点击变成两个动作。点击分流归 `initDropzone`：
落在 `button` / `input` / `.dz-status` 上不触发文件框，落在空白处才触发。
实测（`HTMLInputElement.prototype.click` 计数）：提示语 1 次、`选择文件` 1 次、`本机` 0 次、
状态行 0 次、空白区 1 次。`chip` 现在是 `<button type=button>`（可 Tab、可回车清除），
不再是没有焦点的 `<span onclick>`。

三条取文件的路都只往同一个 `input` 回填，所以命令侧永远只看 `--path`：
拖拽/点击 → `/upload`（服务端副本，受 `data-max-upload` 限）；`本机` → `/pick`（磁盘原始路径，不复制不限大小）；
手填 → 用户自己的字符串。

删掉那两个没人读的属性之后重测过一轮（明暗 × 320–1920 十六档，带着 24.1 MB 的文件名与
`file-chip`）：整页零横向溢出，`选择文件` / `本机` / `chip` / 提示行 / 状态行各自的盒子都在
拖拽区框内、没有被裁 —— 布局与删属性前一致，因为 CSS 从没按那两个名字选过东西。

### 10.3 页面级组件

「用到的令牌」是从 `app.css` 里对着组件规则读出来的，不是凭印象写的（终端区那几个十六进制
字面量是 §1.3 的固定色板，故意不走主题令牌）。

| 组件 | 构成 | 七态 | 用到的令牌 | 无障碍 |
|------|------|------|-----------|--------|
| `topbar` | brand + `command-nav` + 主题按钮 | hover / focus / active / pressed（`aria-pressed=true`=深色，=success 语义）/ disabled·loading·empty·error：n/a（常驻骨架，brand 恒在） | `--surface`（82% 混合）`--hairline` `--s-2`；按钮另用 `--control` `--ink` `--accent` `--surface-3` `--r-control` | `position:sticky` + `backdrop-filter`；实测 320–420 下拉换到第二行（99px）、≥560 单行（57px），各档顶栏自身无横向溢出，主题按钮始终距右 16px |
| `command-nav` | `<select id=cmd-nav>`，可见命令 >1 才出现 | hover / focus / **disabled + `aria-busy="true"`**（实测 `opacity 0.55` + `cursor:progress`：那是「正在办」不是「不许办」）/ loading·empty·error·success：n/a（值域非空、选不出非法值，成不成功由整页重载自己回答） | `--control`（描边，3.64·3.30）`--ink` `--surface` `--r-control` | `aria-label="切换命令"`；跳转在 `initCommandNav`，不写内联 `onchange` |
| `run-bar` | `运行` + `取消` + `复制 CLI` | hover / focus / disabled（跑起来时，实测 `opacity 0.55`；它同时就是「一次只跑一个」的闸门，见 §8.5）/ loading（转圈 + `aria-busy`；reduce 下改文字 `…`，见 §5）/ error（取消失败按钮解禁）/ success（跑完 `setBusy(false)` 复原；`setBusy` 连 `取消` 的 `disabled` 一起复原，见 §8.6）/ empty：n/a（按钮没有空态） | `--accent` `--on-accent`（5.17·7.32）`--control` `--danger` `--hairline` `--s-2` `--r-control` | `type=submit` 语义保留；`aria-busy` 给读屏；窄容器 ≤420 竖排各占满；`取消` 只在服务端能取消时出现（`__CANCEL_JS__`） |
| `cli-preview` | `$ ` 前缀 + 一行命令文本 | empty = `display:none`（不留孤零零的 `$ `）/ 其余六态：n/a（纯展示，交互在 `复制 CLI` 那颗上） | `--surface-2` `--hairline` `--ink-muted`（4.88·6.56 对预览底）`--accent`（`$ ` 前缀）`--r-control` `--s-3` | 只读文本；`updatePreview` 每次表单 input/change 都重算 |
| `output` | `out-head` + `log` + `result-wrap` 的容器 | empty（`hidden`）/ 有内容（撤 `hidden`）/ error（装配失败时也强制露出来，见 §8.3）/ hover·focus·disabled·loading·success：n/a（它自己不接收交互） | `--s-5`；标题行 `--ink-muted` `--s-3` `--s-4` | `#log` 与每个 `.dz-status`（`role=status`）都是 live region，`#out` **不再叠 `aria-live`**，免得同一句话念两遍 |
| `progress` | `.progress` 槽 + `.progress-bar` 条 | loading=不确定（`data-indet=1` 跑动；reduce 下铺满 + 压淡）/ 确定（`aria-valuenow`）/ success（`data-state=done` 整条 `--ok`）/ error（`data-state=error` 整条 `--danger` 且走满）/ empty（宽 0% = 什么都没发生，所以收尾不给 0%）/ hover·focus·disabled：n/a（非交互） | `--surface-3`（槽）`--accent` `--ok` `--danger` `--r-pill` | `role=progressbar` + `aria-valuemin/max`；无比例时**不给** `valuenow`（别骗读屏） |
| `log` | `.terminal` 里的 `.line` 流 | empty（`等待输出…`，`#8b949e`）/ loading=running（持续追加）/ error（`.line.err` `#ff7b72`）/ warn（`#e3b341`）/ tele（`#56d364`）/ focus（`:focus-visible` 内描边）/ 满（500 行上限，实测灌 620 只留 500、滚动跟到底）/ hover·disabled·success：n/a | `--mono` `--hairline` `--r-panel` `--s-3` `--s-4` + §1.3 固定色板（`#0d1117` / `#e6edf3`，16.02:1） | `role=log` `aria-live=polite`；`tabindex=0` 让键盘用户能滚这段可滚动区域（WCAG 2.1.1）。焦点环分两层证据：**实测**到的是同一份 `:focus-visible` 规则在可聚焦控件上生效（`copy-cli` 读回 `outline: 1.6px solid #6ea1ff`）；`#log` / `#result` 这两块只核到规则级（`.terminal:focus-visible, #result:focus-visible` 在 CSSOM 里存在 + 两者 `tabIndex===0`）—— 这台自动化环境的 Tab 不总能移动焦点，别把规则级说成实测 |
| `result` | `结果` 标题 + `复制` + `<pre>` | empty = 整块 `hidden`（**每次运行开头就清**：`run()` 调 `clearResult()`，不然日志在滚、结果还写着上一次的 JSON）/ success（跑完露出 JSON）/ focus（`<pre>` 自己 `:focus-visible` 内描边）/ hover·disabled·loading：n/a / error：n/a（失败没有结果，也不在跑） | `--surface-2` `--hairline` `--ink`（17.63·14.50）`--mono` `--r-panel` `--s-3` `--s-4` | `JSON.stringify(…,2)` 纯文本、不走 innerHTML；≥1200 限高内滚；横向也溢出，所以和 `#log` 同一条规矩：`role=region` + `aria-label` + `tabindex=0`，键盘能滚（WCAG 2.1.1）；焦点环的证据层级同上一条 |
| `file-chip` | 已上传文件的一枚胶囊**按钮** | hover（转 `--danger`）/ focus / empty = `hidden` / 其余 n/a | `--surface-2` `--hairline` `--ink` `--danger` `--r-pill` `--s-1` `--s-3` | `aria-label` 由 JS 写成「清除已上传的 <文件名>」，读屏念得出清的是哪个；实测走完整条上传链路 |
| 复制反馈 | `copyText()` 改文字 + 一次性上色 | success（`--ok`，实测 `#3ddc97`）/ error（`--danger`，实测 `#ff7b72`）/ disabled（反馈期间连点被 `data-copying` 挡住）/ 平时·hover·empty·loading：n/a | `--ok` `--danger`（+ 宿主按钮自己的描边） | 剪贴板被拒（这台自动化环境实测就是被拒）不能静默 —— 静默的复制等于没复制。`writeText` **同步抛**也要接住（非安全上下文里 `navigator.clipboard` 是 undefined）：不接住 `data-copying` 永远留着 `1`，之后每次复制都被自己那道闸门吞掉 |

**状态色实测**（深色主题下读回；浅色用同一批令牌，值见 §1.1/§1.2）：
`disabled` = `opacity 0.55` + `cursor not-allowed`（`run` / `btn-icon` / `input` 三处一致）；
`loading` = `cursor progress` + `spin` 动画；`progress done` `#3ddc97`、`error` `#ff7b72`；
`dropzone.uploaded` 边框 `#6ea1ff` 实线且提示语让位（`.dz-hint` 隐藏）、
错误态靠 `.dropzone:has(.dz-status.err)` 从 `#5f6d7c` 翻成 `#ff7b72`；
`copied` `#3ddc97`、`copy-failed` `#ff7b72`；日志空态占位 `#8b949e`（终端固定色板，见 §1.3）；
主题按钮按下（`aria-pressed=true`）描边与字色转 `#6ea1ff`。

### 10.4 加一个组件的清单

1. `render.rs`：写 `widget_xxx`，进 `render_field` 的 `match`，`data-component="xxx"` 自报家门。
2. `assets/index.html`：写 `initXxx`，加进 `BOOT`（漏了会被 `every_js_component_init_is_bootted` 拦下）。
3. `app.css`：只用 §1 的令牌与 §2 的 `--s-*`；状态用伪类，不新增颜色字面量（终端区除外，见 §1.3）。
4. 要在 DOM 上带信息就加 `data-*` —— **同时**让 JS 或 CSS 读它，否则 `no_data_attribute_goes_unread` 会红；
   要加按钮/动作，先确认服务端真做得到（拿不准就看 `run.rs` 有没有登记对应的句柄/会话，见 §8.6）。
5. 本表加一行：构成 / 哪些态真的存在 / 用到的令牌 / 无障碍要求。**没有的态就写 n/a 并说明为什么**，
   不许为了凑格子留一条永远命中不到的 CSS 规则。
6. 浏览器实测：`srcdoc` iframe 走 320→1920，明暗两套各一遍。


---

### 来源与致谢（MIT）

- 令牌文件结构与 DESIGN.md 格式：[VoltAgent/awesome-design-md](https://github.com/VoltAgent/awesome-design-md)（MIT）；
  其中 `design-md/linear.app/DESIGN.md` 的「单强调色 + hairline 分层 + 不用氛围色造节奏」三条纪律被本表采纳。
- 中性色阶与控件边界对比度的取法参考 [primer/css](https://github.com/primer/css)（MIT）。
- 「设计系统即数字 / 只此一张表 / 注释记下旧版本为什么错」的写法来自
  [cclank/lanshu-html2video-skill](https://github.com/cclank/lanshu-html2video-skill) 的
  `assets/template/src/lib/design.ts`。
- 本表**不引入**上述任何仓库的代码或依赖：CSP 与零依赖约束在前，数值与格式在后。
