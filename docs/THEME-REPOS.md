# Lilyco 主题仓库索引（Hub-and-Spoke 多仓架构）

为降低外部贡献者门槛、隔离 UI/应用重依赖并支持独立发版，Lilyco 生态采用「**框架底座 Monorepo + 主题应用独立仓**」架构，全部仓库统一标记 GitHub Topic **`lilyco`**：

- **框架底座仓**：[`lilyco-42/lilyco`](https://github.com/lilyco-42/lilyco)（根目录精简为 `crates/`、`docs/`、`scripts/`、`site/`，仅保留 10 个核心 SDK crate）

## 独立主题仓（共 7 个，均带 `lilyco` Topic）

| 仓库 | 主题定位 | 包含模块 / 工具 |
|---|---|---|
| [`Lyco-team/lilyco-env`](https://github.com/Lyco-team/lilyco-env) | 跨平台 AI 原生环境控制平面（`no_std` 核心 + 体检/配置/换源/kiri 复现环境） | `lenv` (`lilyco-env-core`, `lilyco-env-runtime`, `lilyco-env-cli`) |
| [`Lyco-team/lilyco-tools`](https://github.com/Lyco-team/lilyco-tools) | 系统工具与领域命令行套件 | `lbin` (`lilyco-binfmt`), `lgrep` (`lilyco-grep`), `lfiles` (`lilyco-files`), `lplc` (`lilyco-plc`) |
| [`Lyco-team/lilyco-media`](https://github.com/Lyco-team/lilyco-media) | 音视频、图像与创作域套件 | `lbrush` (`lilyco-brush`), `lffmpeg` (`lilyco-ffmpeg`), `lvision` (`lilyco-vision`), `lsrt` (`lilyco-srt`), `lpet` (`lilyco-pet`), `lilyco-spine`, `lilyco-letsgal` |
| [`Lyco-team/lilyco-ai`](https://github.com/Lyco-team/lilyco-ai) | AI 工具、记忆包与协议转换套件 | `laic` (`lilyco-aic`), `lmpkg` (`lilyco-mpkg`), `lilyco-bitnet`, `lilyco-web2cli` |
| [`Lyco-team/lilyco-ui`](https://github.com/Lyco-team/lilyco-ui) | 重 UI 与桌面壳套件 | `lilyco-tauri`, `lilyco-graphite`, `lilyco-float`, `lilyco-native-ui`, `lilyco-ultra-ui-example` |
| [`Lyco-team/lilyco-studios`](https://github.com/Lyco-team/lilyco-studios) | 桌面 Studio 应用套件 | `asset-studio`, `vision-studio`, `media-studio`, `desktop-common` |
| [`Lyco-team/lilyco-agent`](https://github.com/Lyco-team/lilyco-agent) | Agent、工具中枢与管理器 | `lilyco-manager`, `lilyco-toolhub`, `tiny-agent-*`, `tiny_lilyco`, `tiny_lilyco_uda` |
