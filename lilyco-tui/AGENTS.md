# lilyco-tui · Ratatui 交互参考

以现有 Ratatui 表单和运行视图为起点，让用户能看出当前焦点、当前任务状态与可用按键。

| 项目 | 适合借鉴 | 许可证与用途 |
| --- | --- | --- |
| [akirco/pigma](https://github.com/akirco/pigma) | 底部状态栏、不同面板的焦点、`?` 快捷键面板、TOML 主题令牌和运行时切换。看 [预览图](https://github.com/akirco/pigma#preview)。 | Apache-2.0（按仓库 LICENSE）。原始推荐。示例终端依赖 Nerd Fonts，借鉴时验证普通字体。 |
| [sxyazi/yazi](https://github.com/sxyazi/yazi) | 三栏导航、选中项高亮、异步预览。适合改善文件参数的选择体验。 | MIT。图片预览按终端能力提供，文字信息应可读。 |
| [alexpasmantier/television](https://github.com/alexpasmantier/television) | 搜索、列表、预览和快捷键在同一视图中配合；对照配置示例和窄终端的预览布局。 | MIT。参考搜索与键盘工作流。 |
| [jesseduffield/lazygit](https://github.com/jesseduffield/lazygit) | 面板切换、上下文动作、确认窗口和详细结果预览。 | MIT。参考复杂操作如何分步呈现。 |
| [gitui-org/gitui](https://github.com/gitui-org/gitui) | Rust TUI 的列表搜索、差异预览、异步操作及多平台终端。 | MIT。参考状态与键盘交互。 |
| [ratatui/ratatui](https://github.com/ratatui/ratatui) | 现有框架的列表、滚动、布局和状态组件。 | MIT。优先对照官方示例。 |

## 聊天 TUI 参考（2026-11-29 实测）

| 项目 | 语言 / Star | 可借鉴 | 许可注意 |
| --- | --- | --- | --- |
| [chojs23/concord](https://github.com/chojs23/concord) | Rust + ratatui / 1.7k | 聊天主模型：`1-4` 切 pane、`Tab` 循环焦点、`j/k/gg/G/Ctrl+d/u`、消息键 `y/r/R/d/e/o/v`、composer `i` + `Ctrl+o` 附件 + `$EDITOR`、`/` 搜索、`Space Space` fuzzy 切频道、Markdown 子集 + 代码高亮、`ratatui-image` 协议探测、`config/keymap/theme.toml` 三分、Nerd Font→Unicode 回退 | 许可非标准（NOASSERTION），只借鉴交互与结构，不复制代码 |
| [ayn2op/discordo](https://github.com/ayn2op/discordo) | Go / 5.9k | 密码 / QR / token 三登录流、`config.toml` 键位与主题、代码块语法高亮 | GPL-3.0；仅借鉴交互，禁止复制代码 |
| [FedericoBruzzone/tgt](https://github.com/FedericoBruzzone/tgt) | Rust / 1.1k | TDlib 封装、配置版本化并在升级时补键位、图片/语音作为可选 feature | Apache-2.0 / MIT，可参考实现 |
| [d99kris/nchat](https://github.com/d99kris/nchat) | C++ / 2.0k | Telegram / WhatsApp / Signal 三协议共用一套终端壳 | MIT，参考信息架构 |
| [paul-nameless/tg](https://github.com/paul-nameless/tg) | Python / 1.2k | TUI + CLI + agent skill 三种入口复用同一能力 | Unlicense |
| [sorokin-vladimir/tele](https://github.com/sorokin-vladimir/tele) | Go / 262 | keyboard-first + 后台 daemon + CLI 分层 | GPL-3.0；只借鉴分层 |
| [ryota-ka/twterm](https://github.com/ryota-ka/twterm) | Ruby / 244 | 时间线 / 会话 / 输入框的经典三区布局（X/Twitter，2023 停更） | MIT；X 侧无活跃 Rust TUI |

结论：聊天模板以 **concord 的交互模型** 为主参考，discordo 补登录与配置，tgt 补配置版本化。禁止复制 GPL 代码。

## 聊天 TUI 模板规范（lilyco 自有实现）

- **布局**：左会话列表 / 中消息流 / 下 composer / 底状态栏。窄终端（80×24）优先保证消息流与 composer，可折叠侧栏。
- **焦点**：`1-4` 直达 pane，`Tab` / `Shift+Tab` 循环，`h/l` 左右，`j/k` 上下；焦点 pane 边框高亮，状态栏显示当前 pane。
- **消息选择**：`j/k` 移动选中、`gg/G` 顶/底、`Ctrl+d/u` 半页；选中行反色，状态栏显示序号与总数。
- **输入**：`i` 进入插入模式，`Esc` 退出，`Enter` 发送必须经 App 的权限门，`Ctrl+J` 换行；草稿与待发送附件在 composer 上方显示。
- **消息动作**：`y` 复制、`r` 回应、`R` 回复、`e` 编辑、`d` 删除、`o` 打开链接、`v` 查看附件；TUI 只发出 action 意图，权限/确认由 App 决定。
- **渲染**：作者、时间、已编辑、回复引用、未读徽标；一期纯文本，Markdown/代码高亮/图片放二期。
- **配置**：键位与主题外置 `keymap.toml` / `theme.toml`，内置默认值；图标优先 Unicode，Nerd Font 仅作可选增强。
- **可访问性**：中文与 emoji 宽度按 `Line::width()` 计算；不用仅靠颜色传达状态。
- **安全**：删除/发送/撤回等写操作不得在 TUI 内绕过 App 权限门；默认不发请求。

验收至少覆盖：80×24 窄终端、中英混排、超长消息、空会话、未读态、发送被拒绝、取消。

先做焦点和可发现的按键，再调整紧凑布局与主题颜色。用窄终端、中英混排、长路径、运行成功、运行失败和取消走一遍主流程。社区讨论记录了同一文件预览在不同终端的表现差异，预览验收要覆盖所用终端能力。[Yazi 社区发布讨论](https://www.reddit.com/r/rust/comments/16fxr58)
