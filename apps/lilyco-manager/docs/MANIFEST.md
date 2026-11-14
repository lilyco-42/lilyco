# 应用清单 `lilyco.app.json`

管理器不内置应用适配器。每个应用在自己的仓库根目录提交一份 `lilyco.app.json`，管理器读取清单后即可发现、启动、读取命令 Schema 并聚合 MCP 工具。

## 发现路径

按顺序：

1. `LILYCO_MANAGER_APPS`：路径列表（Linux/macOS 用 `:`，Windows 用 `;`）。
2. `$XDG_CONFIG_HOME/lilyco/apps` 或 `$HOME/.config/lilyco/apps`（Windows 回落到 `%APPDATA%`）。

目录内支持两种布局：

```text
~/.config/lilyco/apps/
├── lycomc.json              # 直接放清单
└── lycodl/
    └── lilyco.app.json      # 或子目录 + 固定文件名
```

仓库开发时推荐直接指向源码目录：

```bash
LILYCO_MANAGER_APPS=/home/lilyco/code lilyco-manager
```

管理器会扫描该目录下一层的每个项目目录，查找其中的 `lilyco.app.json`。

## 字段

| 字段 | 必填 | 说明 |
|---|---|---|
| `schema_version` | 是 | 当前固定为 `1` |
| `id` | 是 | 小写字母/数字/`-`/`.`，最长 64，全局唯一 |
| `name` | 是 | 展示名，1–120 字符 |
| `description` | 否 | 一句话说明 |
| `repository` | 否 | `https://` 链接 |
| `binary` | 是 | PATH 中的可执行文件名；禁止路径分隔符 |
| `capabilities` | 否 | 缺省时按存在 `run`/`mcp`/`schema` 段自动推断 |
| `run` | 否 | 启动参数、环境变量、UI 地址 |
| `mcp` | 否 | MCP stdio 启动参数，默认 `["--mcp"]` |
| `schema` | 否 | 命令清单启动参数，默认 `["--schema"]` |
| `release` | 否 | GitHub Release 托管安装/更新配置 |
| `dev` | 否 | 仓库内固定开发任务 |

### `run`

```json
{
  "args": ["--web"],
  "env": { "LYCOMC_PORT": "8766", "LYCOMC_NO_OPEN": "1" },
  "url": "http://127.0.0.1:8766"
}
```

- `args` 会原样传给 `binary`，不经过 shell。
- `env` 只允许大写变量名，最多 16 个。用于分配互不冲突的端口、关闭应用自带的浏览器自动打开。
- `url` 必须是 `http://127.0.0.1:*`、`http://localhost:*` 或 `http://[::1]:*`，仅供 UI 的“打开”按钮使用。

### `mcp`

```json
{ "args": ["--mcp"] }
```

管理器会启动该进程，执行 `initialize` 与 `tools/list`，把工具名改写为 `<app>__<tool>` 后聚合展示，然后结束子进程。不会手写工具 Schema。

### `schema`

```json
{ "args": ["--schema"] }
```

管理器运行该命令并把 JSON 命令清单返回给 UI，用于确认应用已安装且契约可用。

### `release`

```json
{
  "repo": "lilyco-42/lycodl",
  "assets": {
    "linux-x64": "lycodl-linux-x64.tar.gz",
    "linux-aarch64": "lycodl-linux-aarch64.tar.gz",
    "macos-universal": "lycodl-macos-universal.tar.gz",
    "windows-x64": "lycodl-windows-x64.zip"
  }
}
```

- `repo` 必须是 `owner/name`；管理器通过 GitHub Releases API 读取最新正式版本。
- `assets` 把平台键映射到发布资产文件名。平台键：`linux-x64`、`linux-aarch64`、`windows-x64`、`windows-arm64`、`macos-universal`；每个平台还接受常见别名（如 `linux-amd64`、`linux-arm64`、`windows-amd64`）。
- 安装只接受带 `sha256:` digest 的资产；下载后强制校验，再解压 tar.gz / zip 或按裸二进制保存。
- 资产解压后按 `binary` 文件名递归查找（最多 4 层）；压缩包内路径固定时可用 `binary_path` 指定相对路径。
- 托管目录：`$XDG_DATA_HOME/lilyco/apps/<id>`（默认 `~/.local/share/lilyco/apps/<id>`，Windows 用 `%LOCALAPPDATA%`）。`current.json` 只在解压与校验成功后原子替换。
- 二进制解析优先级：`LILYCO_MANAGER_BIN_<ID>` > 托管安装 > `PATH`。
- 卸载只删除上述托管目录；安装/卸载前若应用由管理器启动，会先优雅停止；不会删除源码仓库或 `PATH` 中的二进制。

### `dev`

```json
{
  "tasks": {
    "check": ["cargo", "check"],
    "test": ["cargo", "test"],
    "build": ["cargo", "build", "--release"],
    "web-build": ["pnpm", "--dir", "web", "build"]
  },
  "env": { "RUST_BACKTRACE": "1" }
}
```

- 任务名只允许小写字母、数字、`-`，最长 32；最多 8 个任务。
- 每个任务是一个固定 argv，在清单所在仓库目录执行；HTTP 只能选择任务名，不能传 argv。
- 输出进入内存环形缓冲（最多 500 行、32 个最近任务）；任务进程由管理器持有，管理器退出时终止直接子进程。
- 适用于本地开发闭环：检查、测试、构建、前端产物生成；不适合长期驻留服务（请用 `run`）。

## 校验规则（启动时执行）

- 未知 `schema_version`、非法 `id`、带路径的 `binary`、超长参数、NUL 字符一律拒绝。
- 非回环 `run.url` 拒绝。
- 单个清单最大 64KB；重复 `id` 只保留第一份并产生告警。
- 清单加载失败不会让管理器崩溃，会在 `/api/apps` 的 `warnings` 中展示。

## 安全模型

- HTTP 只能传应用 `id` 与清单中已有的任务名，不能传可执行文件名、argv 或 env。
- 实际 argv 与 env 全部来自本地清单；清单目录属于用户信任边界。
- 启动的子进程只由管理器记录 PID；停止只作用于管理器自己启动的进程。
- 状态变更接口额外校验 `Origin` 与 `Host` 同源。
- `LILYCO_MANAGER_BIN_<ID>` 可覆盖二进制绝对路径（把 `-`/`.` 换成 `_`），仅用于本机调试。
- Release 安装只接受 GitHub HTTPS 资产并校验 SHA-256；不执行安装脚本，不写系统目录。
- 开发任务等同于用户在本机仓库里手动执行同一命令；不要注册来自不可信仓库的清单。

## 示例

```json
{
  "schema_version": 1,
  "id": "lycomc",
  "name": "Lycomc",
  "description": "类 PCL 的 Minecraft 启动器",
  "repository": "https://github.com/lilyco-42/lycomc",
  "binary": "lycomc",
  "capabilities": { "run": true, "schema": true, "mcp": true },
  "run": {
    "args": ["--web"],
    "env": { "LYCOMC_PORT": "8766", "LYCOMC_NO_OPEN": "1" },
    "url": "http://127.0.0.1:8766"
  },
  "mcp": { "args": ["--mcp"] },
  "schema": { "args": ["--schema"] },
  "release": {
    "repo": "lilyco-42/lycodl",
    "assets": {
      "linux-x64": "lycodl-linux-x64.tar.gz",
      "linux-aarch64": "lycodl-linux-aarch64.tar.gz",
      "macos-universal": "lycodl-macos-universal.tar.gz",
      "windows-x64": "lycodl-windows-x64.zip"
    }
  },
  "dev": {
    "tasks": {
      "check": ["cargo", "check"],
      "test": ["cargo", "test"],
      "build": ["cargo", "build", "--release"],
      "web-build": ["pnpm", "--dir", "web", "build"]
    }
  }
}
```

## 端口约定

多个应用默认都使用 8766，必须通过 `run.env` 分配不同端口，并让 `run.url` 与之一致：

| 应用 | 端口变量 | 示例 |
|---|---|---|
| lycomc | `LYCOMC_PORT` | 8766 |
| lycodl | `LYCODL_PORT` | 8767 |
