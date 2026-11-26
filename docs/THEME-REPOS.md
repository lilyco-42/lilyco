# Lilyco 主题仓库索引（Hub-and-Spoke 多仓架构）

为降低外部贡献者门槛、隔离 UI/应用重依赖并支持独立发版，Lilyco 生态采用「**框架底座 Monorepo + 主题应用独立仓**」架构：

- **框架底座仓**：[`lilyco-42/lilyco`](https://github.com/lilyco-42/lilyco)（保留 `lilyco-core`、`lilyco-macros`、`lilyco-cli`、`lilyco-tui`、`lilyco-mcp`、`lilyco-dev`、`lilyco`）

## 已拆分的主题独立仓

| 仓库 | 主题定位 | 包含模块 |
|---|---|---|
| [`lilyco-42/lilyco-env`](https://github.com/lilyco-42/lilyco-env) | 跨平台 AI 原生环境控制平面（`no_std` 核心 + 体检/配置/换源/kiri 复现环境） | `lilyco-env-core`, `lilyco-env-runtime`, `lilyco-env-cli` (`lenv`) |
| [`lilyco-42/lilyco-ui`](https://github.com/lilyco-42/lilyco-ui) | 重 UI 与桌面壳套件（隔离 486 包桌面依赖与 `unicode-width` 冲突） | `lilyco-gui`, `lilyco-tauri`, `lilyco-ultra-ui`, `lilyco-ultra-ui-example`, `lilyco-native-ui`, `lilyco-graphite`, `lilyco-float` |
| [`lilyco-42/lilyco-media`](https://github.com/lilyco-42/lilyco-media) | 音视频、图像与创作域套件 | `lilyco-ffmpeg`, `lilyco-vision`, `lilyco-spine`, `lilyco-srt`, `lilyco-brush`, `lilyco-letsgal`, `lilyco-pet` |
| [`lilyco-42/lilyco-ai`](https://github.com/lilyco-42/lilyco-ai) | AI 工具、记忆包与协议转换套件 | `lilyco-aic`, `lilyco-bitnet`, `lilyco-mpkg`, `lilyco-web2cli` |
| [`lilyco-42/lilyco-tools`](https://github.com/lilyco-42/lilyco-tools) | 系统工具与领域命令行套件 | `lilyco-files`, `lilyco-grep`, `lilyco-binfmt`, `lilyco-plc` |

## 过渡期说明

- 本次拆仓前已合入主仓 Open PR（#75、#44、#43）。
- 过渡期内主仓源码暂时保留，待各主题仓独立 CI 稳定运行后再经单独 PR 精简主仓 `Cargo.toml`。
