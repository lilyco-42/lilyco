# lilyco-gui · Web 工作台参考

目标是让第一次打开页面的人能看懂命令、填完参数、运行并读懂结果。现有入口和技术边界见 [Web 适配器说明](web/README.md)，视觉组件使用 shadcn/ui，参数和执行事件来自 `lilyco-core`。

## 优先阅读

| 项目 | 适合借鉴 | 许可证与用途 |
| --- | --- | --- |
| [shadcn-ui/ui](https://github.com/shadcn-ui/ui) | 组件变体、表单错误、对话框焦点和主题令牌。优先从 [官方组件与 Registry](https://ui.shadcn.com/docs) 取组件。 | MIT。现有组件来源与升级记录见 [Registry 锁](web/registry-lock.json)。 |
| [chriskiehl/Gooey](https://github.com/chriskiehl/Gooey) | 从参数声明生成文件选择、枚举和运行页。研究常用参数、可选参数和运行反馈的层次。 | MIT。参考表单生成与交互。 |
| [gradio-app/gradio](https://github.com/gradio-app/gradio) | 输入与输出分区、上传后的文件状态、长任务进度和结果预览。 | Apache-2.0。参考单工具的完整运行流程。 |
| [scalar/scalar](https://github.com/scalar/scalar) | 从 OpenAPI 找到操作、查看约束、试运行、复制请求与读响应。适合完善 Lilyco 的交互式 API 文档。 | MIT。复用前核对具体发布包。 |
| [fastapi/full-stack-fastapi-template](https://github.com/fastapi/full-stack-fastapi-template) | FastAPI 与 React/Vite/shadcn/ui 的完整示例。重点看生成的 API 客户端和表单如何消费共享契约。 | MIT。参考 FastAPI 式易用性和项目组织。 |
| [MichalGniadek/klask](https://github.com/MichalGniadek/klask) | Rust 参数映射到文件选择、多值输入、枚举和输出。适合对照 CLI GUI 的参数覆盖。 | MIT。README 以 Clap 3 为基础，适合作为历史实现参考。 |

## 用法

- 先用 Gooey、Gradio 对照一条「选命令 → 填参数 → 执行 → 查看结果」流程，再用 shadcn/ui 整理视觉层次与组件状态。
- 文件框提供拖放、键盘可达的选择按钮和结果状态。上传中的反馈、运行错误、空输出与复制结果都应可辨认。
- 评审用现有同一条真实命令，记录 320、390、768、1440px 的明暗主题、键盘路径、表单错误和运行回显。
- Gooey 的成功路径建立在结构化参数声明上。社区讨论也记录了不同解析库之间的适配成本，给 Lilyco 的启发是坚持由 schema 映射控件。[上游使用讨论](https://news.ycombinator.com/item?id=38586767)
