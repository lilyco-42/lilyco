# lilyco-cli · 命令行呈现参考

`lilyco-cli` 从 `CommandSchema` 生成 Clap 命令并呈现输出。交互式全屏体验见 [lilyco-tui](../lilyco-tui/AGENTS.md)，窗口化参数表单见 [lilyco-gui](../lilyco-gui/AGENTS.md)。

| 项目 | 适合借鉴 | 许可证与用途 |
| --- | --- | --- |
| [qwerprog/bili-tools](https://github.com/qwerprog/bili-tools) | Rust CLI 的中文帮助、分组示例、主题色和 Unicode 宽度处理。重点看 [终端 UI 模块](https://github.com/qwerprog/bili-tools/tree/main/src/ui) 的主题和降级策略。 | MIT。原始推荐，优先对照终端输出。 |
| [cli/cli](https://github.com/cli/cli) | 用留白、缩进和分组建立终端层次，颜色辅助状态，帮助里放可执行示例。参见 [GitHub CLI 的排版规范](https://github.com/cli/cli/blob/trunk/docs/primer/foundations/README.md)。 | MIT。参考输出约定和帮助组织。 |
| [charmbracelet/gum](https://github.com/charmbracelet/gum) | 选择列表、确认、输入和状态提示的密度与视觉一致性。 | MIT。参考交互样式。 |
| [console-rs/indicatif](https://github.com/console-rs/indicatif) | Rust 进度条、spinner 和多任务进度。接入前先核对现有 `Progress` 事件能否直接驱动呈现。 | MIT。可评估局部复用。 |
| [Nukesor/pueue](https://github.com/Nukesor/pueue) | 把任务状态、日志和 JSON 输出做成可分别调用的命令。 | Apache-2.0。适合查看长任务回顾。 |
| [cli-guidelines/cli-guidelines](https://github.com/cli-guidelines/cli-guidelines) | 终端能力检测、色彩、提示、错误恢复和结构化输出的评审清单。 | CC BY-SA 4.0。参考文字指南。 |

优先改进 `--help` 的示例、错误后的修复路径、进度结束摘要。验证交互终端、窄终端、重定向与 `NO_COLOR`。保持 `--json` 和 `--json-stream` 可直接解析。GitHub CLI 的上游问题记录了在强制终端模式下 JSON 可能带颜色，适合作为输出兼容性的回归例子。[CLI 上游问题](https://github.com/cli/cli/issues/13121)
