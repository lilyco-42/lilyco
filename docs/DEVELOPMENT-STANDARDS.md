# 开发规范（Development Standards）

适用于 lilyco 主 workspace（`crates/`、`apps/`）与独立子项目（如 `apps/lilyco-manager`）。
目标是让“新增一个功能”有固定套路，减少返工与跨会话失忆。

## 1. 端口

所有本机监听端口必须遵守 [PORT-ALLOCATION.md](PORT-ALLOCATION.md)：

- 只绑定 `127.0.0.1`；
- 在所属段内选 `preferred_port`，通过 manager 注入（`port_env`），不在代码里假设端口不被占用；
- 新应用在规范第 5 节登记；
- 冲突时**改默认值 + 同步文档/脚本**，不要让用户带环境变量绕开。

## 2. 模块化（前端与服务共用同一套心智）

一个功能 = 五件套：

```
modules/<feature>/
  config.ts        # 纯类型 + 纯函数：默认值 / 归一化 / 持久化 / URL 生成
  state.ts         # 数据流（SSE / 轮询 / 事件），不依赖具体视图
  View.tsx         # 渲染核心，只接 props
  SettingsCard.tsx # 模块自带设置 UI，宿主一行引入
  index.ts         # 统一出口
```

规则：

1. `config` 不 import 框架运行时，`normalize*` 对任意输入必须返回合法值（clamp 而非抛错）；
2. 数据流与视图分离，hook 只产出数据（`items`/`seq`/`running`/`error`）；
3. 设置 UI 跟着模块走，不要散落在宿主页面；
4. 需要独立打开的能力（OBS 浏览器源、新窗口、iframe）提供 `XxxView`，在入口按 pathname 分流；
5. 只用同源静态资源，不加载远端脚本；
6. 可被外部钉死的参数（OBS 源等）必须支持 URL query 覆盖（localStorage 不共享）。

参考实现：`lyblive/web/src/modules/danmu-overlay/`（配置/数据流/视图/设置卡/独立页），
以及 `lilyco-ultra-ui` 的 `graph` / `designer` / `#blocks` 三页互不复用同一渲染器。

## 3. 测试矩阵

| 层 | 要求 |
| --- | --- |
| 纯函数 | 单独 `node --test` 或 `cargo test`，覆盖 clamp、边界、坏输入 |
| 服务 | `cargo test`；HTTP 用 `web-smoke` 类脚本，只回环、不访问外网 |
| 端到端 | 真实二进制 + 真实浏览器（CDP），断言 DOM/接口/无 console 错误 |
| 跨平台 | CI 至少 ubuntu + windows；macOS 单独作业（容量问题会取消，需重跑失败项） |
| 安装 | 一键脚本必须能被目标 shell 解析（`bash -n`、PowerShell `Parser::ParseFile`） |

验收口径：

- 逐条给**命令 + 退出码 + 关键输出**，汇总式“测试全绿”不算；
- 不许为了过测改断言放宽判据；
- “旧行为不变”必须对旧路由实测回归，而不是只跑新用例。

## 4. 发布三件套

对外交付必须同时具备：

1. **Release**：全平台资产 + `SHA256SUMS` + 安装脚本（`install.sh` / `install.ps1`）；
2. **教程**：仓库内可照做的安装/使用文档（含各平台差异与移动端方案）；
3. **一键安装**：脚本要实测（下载 → 校验 → 落位 → 可执行），不能只发二进制。

发布前检查：

- 版本号与 `Cargo.toml` 一致；
- `publish` 作业有 `checkout`（否则拷不到安装脚本）；
- macOS 被取消时要 `rerun --failed`，不能当成功；
- 安装脚本随 Release 一起发，不要只放仓库 raw。

## 5. 文档

- 教程按“可直接照做”写（shadcn/ui guideline 风格），代码块可编译；
- 每章给一个最小可运行示例，避免“只讲概念”；
- 变更写进对应 `docs/*.md`，不要只留在会话里。

## 6. 安全与凭据

- 本机服务默认只监听回环；需要远程时用 SSH 隧道 / USB 端口转发；
- 写接口必须鉴权（manager 用 `x-lilyco-token`），只读巡检用 `READ_ONLY` 模式；
- 凭据默认不落盘；确需落盘时 Unix `0600`、Windows 依赖用户目录 ACL；
- 日志不打印推流密钥、cookie、token。

## 7. 命名与目录

- crate 名 `lilyco-<domain>`；应用放 `apps/<name>`；
- 端口常量集中在一处（`ports.rs` / `DEFAULT_PORT`），不要散落在 handler 里；
- 环境变量统一 `LILYCO_*` 前缀；应用自有变量（如 `LYBLIVE_PORT`）在 manifest 里登记。

## 8. 成熟商业方案优先与 Web UI / DSH 规范

- **优先参考商业化成熟方案实现，不要闭门造车写简陋 MVP**：立项与编码前先调研同领域高星开源实现及 `lib.rs` / npm 成熟生态库，直接对标成熟产品的信息架构、权限分级（访客 / 用户 / VIP / 管理员）与交互完整度（如真流式输出、新手引导），拒绝交付残缺占位页或假交互。
- **AI / Agent 交互界面优先推荐 DSH（DeepSeek 开源网页 UI & Harness）**：详见 [DSH.md](DSH.md)。通过 MCP 挂入 DSH 即可复用其成熟的多轮流式对话、工具面板与响应式布局，避免重复造轮子与排版缺陷。
- **独立商业 Web 控制台必须基于官方模板起步**：禁止从零手搓 CSS 骨架，直接基于 `lilyco-template/web/`（融合 `lycodl` 视觉底座与 `new-api` 前端生态：`Tailwind v4` + `shadcn` `oklch` 令牌 + `@fontsource-variable/geist` + `@base-ui/react` + `@lobehub/icons` + `@tanstack/react-table` + `cmdk` + `sonner` + `react-markdown` + `recharts`），后端严禁拼接绘制 UI，只提供强类型零信任 API 与真流式通道。
- **视觉验收铁律**：重大 UI 变更后必须通过真浏览器实机截图并识图自查（重点排查按钮折行、视口等高对齐、明暗主题对比度与空态引导）。
