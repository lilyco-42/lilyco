# 原始来源与设计判断

- 原始引擎：本 repo `lilyco-vision/src/main.rs` 的八工具注册，实际处理源分列于 crop / resize / dominant / diff / foreground / trace / info / screenshot；MCP 必须委托现有 core 执行器和 schema 校验。
- 共用桌面壳：`lilyco-42/mirage-tank` `59f4142602c5f188bcd8db0eeb5b2cc20c849d55` 的 desktop 目录；其祖先素材工坊保留 Triforge 原 MIT 声明。FastAPI / WebView2 生命周期和安装清单复用，不复制图像算法。
- 原 shadcn 源和固定摘要：`ui/src/shadcn-sources.json`，https://github.com/shadcn-ui/ui 。界面改在 wrapper，不改原组件。
- 矢量化引擎说明：https://github.com/visioncortex/vtracer/blob/master/README.md 。本 repo 使用 Cargo.lock 中固定 `1.0.0-alpha.3`；不能以 README 的其他版本功能宣称实现。
- Chromium 独立 profile：https://www.chromium.org/developers/creating-and-using-profiles/ 。截图显式 `--user-data-dir` 临时目录，不复用个人 Chrome。
- Ubuntu CI Chromium 沙箱：https://chromium.googlesource.com/chromium/src/+/main/docs/security/apparmor-userns-restrictions.md 。只为 CI 下载的精确可执行路径建立 AppArmor userns 许可，不关闭 Chromium 沙箱、不改用户本机配置。
- Windows 进程树：https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects 。在发送工具请求前将引擎放入带 KILL_ON_JOB_CLOSE 的 nested Job，取消后检查 ActiveProcesses=0，不按 Chrome 名称杀进程。
- Chromium 自动化进程归属参数：https://github.com/microsoft/playwright/blob/v1.56.1/packages/playwright-core/src/server/chromium/chromiumSwitches.ts 。Windows 使用 AutoDeElevate 与 Edge relaunch 的自动化参数，避免交给 Explorer 重启后离开宿主 Job，保留浏览器沙箱。

首批投入判断：现有算法可直接复用，参数、文件选择与导出能改善非技术用户操作，且无 AI 推理成本。仍没有此新产品销量证据；这是交付成本判断，不是市场验证。官方同源完整包可免费分发，加速交付/支持收费需明确区分。
