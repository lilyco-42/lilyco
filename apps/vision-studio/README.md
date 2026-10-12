# Lain42 图片工坊

把原有 `lvision` 八个视觉工具做成本机桌面工作台。FastAPI 管理临时文件与任务，React 19 + shadcn Base Vega 负责界面，Windows WebView2 负责窗口。原 Rust 引擎通过 MCP stdio 执行，算法与参数语义仍由 Lilyco schema / core executor 管理。连接器先查询 tools/list：原工具名为 ImageInfo / Crop / Resize / DominantColors / PixelDiff / ExtractForeground / Trace / HtmlScreenshot，参数采用 kebab-case；界面名不替换原接口。

## 使用

Windows 安装包安装到当前用户目录，无需 Python / Rust / Node。便携 ZIP 解压后双击 `Lain42-Vision-Studio.exe`；桌面需要 Microsoft WebView2，未安装时提示并回退默认浏览器。Linux 解压后运行 `Lain42-Vision-Studio`，打开本机浏览器；完成后关闭启动窗口。

选择工具 → 选择文件 → 设置参数 → 开始处理 → 预览 → 下载成果。可以重复调整参数；清空会移除这组临时文件和任务。关闭应用会移除所有临时工作区，先下载需要保留的结果。

| 工具 | 实际输出 |
| --- | --- |
| 裁剪与放大 | 像素坐标框、整数放大、PNG |
| 调整尺寸 | Lanczos3 缩放；单边填 0 保持比例 |
| 主色提取 | 配色和占比、可选区域 / 候选色评分 |
| 图片对比 | 两张图的差异热图、网格评分 |
| 纯色背景提取 | 边界连通背景透明化，不是 AI 人像抠图 |
| 位图转 SVG | vtracer 颜色 / 单色、曲线 / 多边形 |
| 图片信息 | 格式、尺寸、字节大小 |
| HTML 截图 | 本地 HTML 截图，需本机 Chrome / Edge |

所有工具生成原引擎 JSON 报告，可下载 PNG / SVG / ZIP。HTML 必须是用户自己的可信文件；脚本和外部资源由独立浏览器执行，可能联网，但不读取个人浏览器登录状态。不提供任意命令、URL 抓取、上传路径或输出路径 API。

图片仅在本机临时目录处理，不发往 Lain42 / 模型 API。单文件 16 MB、总量 32 MB、图片和输出最多 1600 万像素、HTML 最多 2 MB、单任务 2 分钟、最多 10 个临时工作区。裁剪/矢量化的倍率预算按完整输入面积保守限制。原始图片不会覆盖。

## 交付与许可

Windows 安装 EXE / 便携 ZIP 和 Linux ZIP 只由 GitHub Actions 构建。正式包必须通过八工具真实 MCP 输出、HTTP 边界、桌面/手机交互、冻结包、WebView2 正常关闭和安装卸载门禁后发布。本目录出现不代表已经发布；正式下载以仓库 Release 的当前通过版本为准。

原 repo 许可保留；`SHARED-SOURCE-LICENSE` 保留共用壳的原始 MIT 声明，shadcn 源组件独立保留 MIT。正式包携带依赖完整许可文本、Cargo / npm 锁文件和 SHA-256 manifest。不附带 Chrome / Edge，不读取其他产品授权，不包含 AI 服务额度。可以销售整理交付和支持，收入须以真实订单为证，不承诺销量。

开发者的运行、构建、测试均在 `.github/workflows/vision-studio.yml`。不得在本机执行编译、冻结或产品测试。
