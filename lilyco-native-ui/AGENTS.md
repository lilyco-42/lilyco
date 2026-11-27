# lilyco-native-ui · 原生界面参考

原生渲染以现有 [NSH / GTK4 项目说明](README.md) 为起点。参考成熟应用的视觉层次和窗口交互，把控件落到当前 `UiSpec` 和渲染器。

| 项目 | 适合借鉴 | 许可证与用途 |
| --- | --- | --- |
| [longbridge/gpui-kit](https://github.com/longbridge/gpui-kit) | Rust 桌面组件的密度、面板层次、焦点、表格和表单状态。 | 软件源码和代码示例 Apache-2.0。文档文字、插图另有授权，见仓库 License 段落。 |
| [lapce/lapce](https://github.com/lapce/lapce) | 原生工具的编辑区、侧边栏与状态栏组合，以及小窗口下的信息层次。 | Apache-2.0。参考桌面工作区布局。 |
| [LocalSend/LocalSend](https://github.com/LocalSend/LocalSend#screenshots) | 同时看桌面和手机的文件操作入口、状态与低密度页面。 | Apache-2.0。参考跨屏工作流。 |
| [MichalGniadek/klask](https://github.com/MichalGniadek/klask) | Rust CLI 参数转成原生窗口组件的例子，含路径选择和执行输出。 | MIT。历史参考，基于 Clap 3 和 egui。 |

每次选一个真实 `UiSpec` 对照，记录首帧、键盘焦点、明暗主题和数据更新。对同一控件先比较 `UiSpec` 表达和现有平台能力，再评估复用和维护成本。
