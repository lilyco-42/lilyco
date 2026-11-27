# lilyco-tauri · 桌面工具参考

这是 Lilyco 的桌面壳。研究窗口、系统对话框、拖放、任务反馈和桌面交互。Web 参数渲染参考 [lilyco-gui](../lilyco-gui/AGENTS.md)，原生 GTK 参考 [lilyco-native-ui](../lilyco-native-ui/AGENTS.md)。

| 项目 | 适合借鉴 | 许可证与用途 |
| --- | --- | --- |
| [mountain-loop/yaak](https://github.com/mountain-loop/yaak) | Tauri、Rust 和 React 的真实应用。重点看标签页、操作区与响应区、长请求取消和主题切换。 | MIT 源码。官方预构建二进制使用另行发布的条款。 |
| [farion1231/cc-switch](https://github.com/farion1231/cc-switch) | 同一个桌面窗口管理多个工具时的切换入口、设置分组和状态呈现。 | MIT。参考桌面信息架构。 |
| [LocalSend/LocalSend](https://github.com/LocalSend/LocalSend#screenshots) | 文件从选择到发送的流程、单个文件与批量状态、桌面和手机的空间适配。 | Apache-2.0。参考布局与文件工作流。 |
| [mifi/lossless-cut](https://github.com/mifi/lossless-cut) | 本地媒体工具的预览区、批量片段、导出和快捷操作。 | GPL-2.0。用于界面与流程观察。 |
| [tauri-apps/tauri](https://github.com/tauri-apps/tauri) | 现用桌面壳的窗口、WebView 和系统交互文档入口。 | MIT 或 Apache-2.0。优先对照现有 Tauri 版本。 |

评审先走文件选择或拖入、运行、取消、查看结果。验证文件名很长时的布局、一次错误恢复和键盘主流程。把系统窗口行为与 WebView 内组件样式分别观察，便于定位具体改进。
