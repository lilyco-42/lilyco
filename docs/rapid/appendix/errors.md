# 错误索引

按症状查：原因 → 修复。具体报错文本随版本可能变化，以命令实际输出为准。

## 启动

| 症状 | 原因 | 修复 |
|---|---|---|
| `dev` 启动即失败，提示地址被占用 | 8421/5173/8080 已被占用 | `ss -ltnp` 查占用，改 `Rapid.toml` 或端口环境变量 |
| 全栈启动卡在依赖安装 | Node.js/pnpm 缺失或版本不符 | 先 `dev --api-only` 验证后端，再装 Node + pnpm 12 |
| 数据库连接被拒绝 | Compose 未启动 / 外部库不可达 | `docker compose up -d`，或检查 `RAPID_DATABASE_URL` |
| `readyz` 失败、`healthz` 正常 | 存储不可用 | 看数据库容器日志与连接串；SQLite 检查文件权限 |
| 容器运行时命令找不到 | Docker/Kiri 未安装或未启动 | 装对应运行时；SQLite 项目不需要容器 |

## 生成

| 症状 | 原因 | 修复 |
|---|---|---|
| `new` 失败，目标已存在 | 同名目录已存在 | 换名字，或先手动移走旧目录；CLI 不覆盖 |
| `new ../x` 被拒绝 | 路径逃逸 | 使用合法项目名；输出目录用 `--dir` |
| 改模板后新项目没变化 | 模板编译期内嵌 | 重新 `cargo install --force` 或 `cargo build -p lilyco-rapid` |
| `generate resource` 拒绝字段类型 | 仅支持 `string/bool/i32` | 用支持类型，或手写 Rust 结构体 |

## 运行时

| 症状 | 原因 | 修复 |
|---|---|---|
| 写请求 415 | 缺少 `application/json` | 加 `Content-Type` |
| 写请求 413 | 超过 `max_body_bytes` | 减小请求，或调整配置 |
| 422 且 `field` 有值 | 业务校验失败 | 按 `field` 修正输入 |
| 多传字段被拒绝 | 未声明字段不允许 | 删除多余字段；契约以 `/openapi.json` 为准 |
| 404 | 路径/ID 不存在 | 检查集合名与记录 ID |
| 405 | 方法不被允许 | 查 `/openapi.json` 的 methods |
| 503 `storage_busy` | SQLite 写锁/数据库忙 | 稍后重试；检查长事务与并发写 |
| 500 `storage_failure` | 存储操作失败 | 看服务端日志、迁移是否执行、磁盘空间 |

## 导入导出

| 症状 | 原因 | 修复 |
|---|---|---|
| `data import` 被拒绝 | 目标库非空或 ID 冲突 | 导到空库；先备份再清理目标 |
| 导入后数据不全 | 只导入了 Rapid 资源记录 | 自定义表/索引需单独迁移 |
| 快照解析失败 | JSON 损坏或来源不兼容 | 用同版本 CLI 重新导出 |

## 测试与构建

| 症状 | 原因 | 修复 |
|---|---|---|
| `cargo metadata --locked` 失败 | 锁文件与 Cargo.toml 不一致 | 确认依赖变更，再 `cargo update -p <crate>` |
| `fmt --check` 失败 | 格式漂移 | `cargo fmt --all` 后提交 |
| Clippy 报新 crate 警告 | 新代码未过 lint | 修代码，不要全局 `allow` |
| 测试删断言后变绿 | 绕过了失败 | 恢复断言，修实现；测试零妥协 |

## 仍无法解决

按顺序提供：

1. `lilyco-rapid --version`；
2. 失败命令的完整 stderr；
3. `Rapid.toml`（隐去连接串与令牌）；
4. `curl -i http://127.0.0.1:8421/readyz` 的输出；
5. 容器运行时 `docker ps` / `kiri ps` 状态。

不要只贴「跑不起来」，也不要通过删除业务代码或关闭测试来「修好」。