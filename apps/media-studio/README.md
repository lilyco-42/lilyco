# Lain42 视频工坊（开发中）

用可视化方式调用本仓库原有 `lffmpeg`：压缩、转成 MP4/WebM、缩放和截取片段。
前端是 React/shadcn，后端 FastAPI 只管理本机工作区与任务；原 Rust MCP/schema/executor 负责实际媒体处理。
Windows 入口使用 WebView2；同一工作台也可在本机浏览器打开，不需要云端上传。

每次处理生成独立输出，支持取消、预览、视频下载和包含原始引擎结果的 ZIP。输入限 256 MB / 2 小时 / 4K 像素总量，
输出限 512 MB。默认 H.264/AAC MP4，WebM 使用 VP9/Opus；浏览器不能播放原格式时仍可转成 MP4。

## 当前状态

当前是原仓库内的新产品开发分支，尚未形成可销售的 Windows 完整包。
Linux 上的真实原引擎、编码/解码、取消、HTTP 边界与浏览器流程由专用 Actions 验证；
EXE/MSI、Windows 原生/安装卸载、FFmpeg 运行时来源与对应完整源码/许可载荷仍须补齐并验收。
本机不安装依赖、编译或运行产品测试，也不会把未过门的构建当成正式下载。

## 原始来源

- `lilyco-ffmpeg/src/main.rs`：原有命令，未复制编码算法或执行宿主。
- `apps/vision-studio/`（主线 `0c0245f1e5de133b4e33aec6902d043bf58bad29`）：本机壳、工作区边界、MCP 传输、Windows 进程 Job、锁定 UI 组件的复用来源；保留 `SHARED-SOURCE-LICENSE`。
- Noto Sans SC 5.3.1 字体依赖锁来自 Office UI；所有前端依赖仍由 lockfile 的 npm integrity 在 Actions 验证。
- `resources/icon-source.json`：按 lyco-ip 规范生成的原创源图、哈希、提示词与独立 IP 许可。

FFmpeg 的外部二进制与许可不能由 Python/Rust 包的 MIT 许可替代。在完整发行前，必须固定构建来源，
检查实际编码器和许可证，并随包提供适用原文及对应源码，不假定所有 FFmpeg 构建都是同一种许可。
