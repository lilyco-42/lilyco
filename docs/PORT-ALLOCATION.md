# 端口分配规范（Port Allocation）

本规范约束 **lilyco 生态所有本机应用**（含 `lilyco-manager` 托管的第三方应用）的监听端口，
目标是：

1. 同机多应用不抢端口；
2. 端口号本身能表达用途（看一眼就知道是金融、游戏还是直播工具）；
3. 冲突时由程序自动漂移，而不是让人手动改环境变量。

实现位置：`apps/lilyco-manager/src/ports.rs`（段表 + 分配器 + 快照），
对外接口：`GET /api/ports`。

## 1. 段表

| 段 key | 范围 | 用途 | 典型应用 |
| --- | --- | --- | --- |
| `finance` | 4200–4299 | 重要应用 / 金融 / 银行 / 支付 | 账单、钱包、发票、银行桥接 |
| `game` | 4300–4399 | 游戏 | 游戏启动器、手柄桥、模拟器 |
| `media` | 4400–4499 | 媒体创作（图像 / 音视频） | rembg-ui、vision-studio、音频工具 |
| `office` | 4500–4599 | 生产力 / 办公 | 文档、笔记、日历 |
| `social` | 4600–4699 | 社交 / 通讯 | IM、邮件、社区客户端 |
| `commerce` | 4700–4799 | 商业 / 计费 | 计费服务、订单、商店 |
| `ai` | 4800–4899 | AI / Agent 服务 | MCP 服务、模型网关、Agent 运行时 |
| `devtools` | 4900–4999 | 开发工具 / 构建 | 构建服务、调试器、本地 CI |
| `ops` | 8000–8099 | 运维 / 管理面板 | proxy-panel、集群面板 |
| `livestream` | 8700–8799 | 本机直播 / 媒体工具链 | lyblive、lycomc、lycodl |
| `internal` | 57890–57899 | lilyco 内部微服务 | lilyco-manager、facade 辅助进程 |

> 4200/4300 这样的起段是按用途分组的建议值，不是 IANA 注册；选择依据是“本机常见软件很少占用 4xxx”。

## 2. 规则

1. **只监听回环**：`127.0.0.1`，任何情况下不 bind `0.0.0.0`（远程访问走 SSH 隧道/端口转发）。
2. **段内分配**：从 `preferred_port` 起线性 +1 探测，最多 20 次；**不跨段借用**。
3. **不抢外部端口**：段内已被其它程序监听的端口视为不可用（`TcpListener::bind` 失败即跳过）。
4. **冲突自动漂移**：漂移时 manager 输出日志并返回实际端口，调用方不得自行改环境变量绕过。
5. **禁止系统端口**：0–1023 一律不分配。
6. **禁止硬编码**：应用默认端口可以写死，但必须同时提供 `preferred_port` 与 `port_env` 让 manager 覆写。
7. **新应用先登记**：新增应用时在 manifest 里声明段与首选端口，避免与他人“先到先得”。

## 3. 应用如何接入（manifest）

`apps/lilyco-manager` 的应用清单（`run` 段）新增三个字段：

```json
{
  "id": "lyblive",
  "capabilities": { "run": true },
  "run": {
    "args": ["--web", "--no-browser"],
    "preferred_port": 8768,
    "port_env": "LYBLIVE_PORT",
    "port_band": "livestream",
    "url": "http://127.0.0.1:8768/"
  }
}
```

- `preferred_port`：首选端口（必须在所属段内；段外会被回落为段起点并标记 `drifted`）。
- `port_env`：把分配到的端口注入哪个环境变量，默认 `PORT`。
- `port_band`：显式指定段；缺省按应用 id 关键词推断（`bank|pay`→finance、`game`→game、
  `rembg|vision|asset|audio|music`→media、`agent|mcp|llm`→ai、`lyblive|lycomc|lycodl|obs`→livestream、
  `lilyco*`→internal、其余→devtools）。

启动时 `manager` 会：
1. 取应用所属段；
2. 在段内探测空闲端口；
3. 注入 `port_env`；
4. 返回 `{"ok":true,"pid":…,"port":…}`，日志里有漂移记录。

## 4. 查询接口

```
GET /api/ports        # 需要 x-lilyco-token
```

返回：

```json
{
  "bands": [
    { "key": "livestream", "label": "本机直播 / 媒体工具链", "start": 8700, "end": 8799,
      "used": [8765, 8766, 8767, 8768], "free": 96 }
  ],
  "apps": [
    { "id": "lyblive", "band": "livestream", "preferredPort": 8768,
      "portEnv": "LYBLIVE_PORT", "suggestedPort": 8768, "drifted": false }
  ]
}
```

`used` 是“当前被任何进程占用”的端口（含外部应用），因此也能当作**冲突预警**。

## 5. 本机现有端口登记（2026-11-18）

| 端口 | 归属 | 段 |
| --- | --- | --- |
| 8765 | AgentDock（MCP 端点 `http://127.0.0.1:8765/mcp`，Apache-2.0） | 外部应用（占用 livestream 段） |
| 8766 | lycodl | livestream |
| 8767 | lycomc | livestream |
| 8768 | lyblive 常驻服务（`lyblive.service`） | livestream |
| 8769 | lyblive `--web` 默认 | livestream |
| 4700 | lain42 studio-billing（远端服务器） | commerce（非本机） |
| 8010 | lain42 proxy-panel（远端服务器） | ops（非本机） |

> AgentDock 是外部程序，占用了 `livestream` 段的 8765。规则 3 要求我们**不抢占**，
> 因此 lyblive 的默认端口自 v0.2.4 起改为 8769，`--open` 固定指向常驻服务的 8768。

## 6. 冲突排查清单

1. `ss -ltnp | grep :<port>` 看占用者 PID/进程名；
2. `GET /api/ports` 看段内 `used`；
3. 确认不是外部应用后，再调 `preferred_port`（不要改 `port_env`）；
4. 仍冲突则换段（改 `port_band`），并在本文件第 5 节登记。