# Lain42 素材工坊 · 早期预览版

复用 `lilyco-aic` 的 `laic`，让用户不用输入命令即可查看 AIC 角色、预览动画、导出部件。
架构沿用 Rembg：静态 Web 界面 → 本机 FastAPI → 已有 CLI。解析器不复制，不改 Lilyco 执行核心。

## 用户怎么用

1. 下载对应系统 ZIP，**完整解压**后双击 `Lain42-Asset-Studio.exe`（Windows）或同名程序（Linux）。
2. 浏览器自动打开本机窗口。选择 `.pxls` / `.pxls.dat` / `.pxls.bytes` 和同目录的配套 `.texture_0.dat` / PNG；也可选择素材文件夹。
3. 点「读取动画列表」，选姿势和朝向，点「预览并导出动画」。GIF、PNG、精灵表可直接查看，ZIP 一次保存全部结果。
4. 可导出部件 PNG 或 AIC 配套 Unity 贴图，删除工作区、取消任务。关闭启动窗口退出并清理临时文件。

所有文件只写入本机临时目录，不上传服务器。一次最多 2000 文件、512 MB；单文件 128 MB。
10 个工作区、50 个任务记录、单任务 120 秒；一次仅运行一个引擎进程。
本版本不是云服务，不应绑定到公网，也不提供多用户服务器模式。

## 支持范围

- AIC：角色列表、姿势列表、PNG / GIF / 精灵表、部件 PNG、配套 Unity Texture2D 解码。
- 不修改原文件，不自动写回游戏，不携带游戏本体或角色素材，不是通用 Unity 编辑器。
- Spine：已接好 `lyco-spine-render` 的动画列表和 GIF 播放 / 导出适配器，目标 **3.8**。
  **默认包不包含该播放器**；有权使用运行时的用户可把自己的二进制放进 PATH，或由操作者设置
  `LAIN42_LYCO_SPINE_RENDER`。缺少运行时时入口清楚说明，不冒充完整可售的 Spine 播放器。
  运行时许可见 [Spine Runtimes License](https://esotericsoftware.com/spine-runtimes-license)。
- 目前 Windows、Linux ZIP；未提供 macOS 签名包或手机安装包。

## 开发与验证

Python 3.12，依赖在 `requirements.txt`。开发入口 `launcher.py`。
`LAIN42_LAIC` 仅供本机操作者指定可信引擎位置；网站请求不能选择可执行程序或输入 shell 命令。
新增 CLI 只添加 `studio/adapters.py` 中的适配器，不改 UI 的任务、文件和取消逻辑。

**构建和测试仅在 GitHub Actions**，`.github/workflows/asset-studio.yml`：
原生 `laic` 单测 + 构建 → 自制 PXLS/PNG 真解析/渲染/导出 → API 隔离/路径/取消测试 →
Playwright 桌面/手机尺寸预览 → PyInstaller 完整包 → 完整包再次真实解析/渲染冒烟。
样例为 CI 自制色块角色，不能拿历史游戏资源截图冒充本版实测。

源码沿用仓库 MIT 许可；收费对象可为完整打包、加速交付与支持。开源源码仍可免费获取。
售卖工具不代表可以转售第三方游戏或 Spine 运行时。
