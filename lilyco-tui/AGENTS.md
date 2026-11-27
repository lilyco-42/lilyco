# lilyco-tui · Ratatui 交互参考

以现有 Ratatui 表单和运行视图为起点，让用户能看出当前焦点、当前任务状态与可用按键。

| 项目 | 适合借鉴 | 许可证与用途 |
| --- | --- | --- |
| [akirco/pigma](https://github.com/akirco/pigma) | 底部状态栏、不同面板的焦点、`?` 快捷键面板、TOML 主题令牌和运行时切换。看 [预览图](https://github.com/akirco/pigma#preview)。 | Apache-2.0（按仓库 LICENSE）。原始推荐。示例终端依赖 Nerd Fonts，借鉴时验证普通字体。 |
| [sxyazi/yazi](https://github.com/sxyazi/yazi) | 三栏导航、选中项高亮、异步预览。适合改善文件参数的选择体验。 | MIT。图片预览按终端能力提供，文字信息应可读。 |
| [jesseduffield/lazygit](https://github.com/jesseduffield/lazygit) | 面板切换、上下文动作、确认窗口和详细结果预览。 | MIT。参考复杂操作如何分步呈现。 |
| [gitui-org/gitui](https://github.com/gitui-org/gitui) | Rust TUI 的列表搜索、差异预览、异步操作及多平台终端。 | MIT。参考状态与键盘交互。 |
| [ratatui/ratatui](https://github.com/ratatui/ratatui) | 现有框架的列表、滚动、布局和状态组件。 | MIT。优先对照官方示例。 |

先做焦点和可发现的按键，再调整紧凑布局与主题颜色。用窄终端、中英混排、长路径、运行成功、运行失败和取消走一遍主流程。社区讨论记录了同一文件预览在不同终端的表现差异，预览验收要覆盖所用终端能力。[Yazi 社区发布讨论](https://www.reddit.com/r/rust/comments/16fxr58)
