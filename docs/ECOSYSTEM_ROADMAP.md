# 生态融合路线图（P0-P3）

> 2026-09-19 立项。依据：全库 gh 体检（100+ 仓库）+ 内聚/耦合工程评估。
> 主线对齐 [lystack](https://github.com/lilyco-42/lystack)：记忆包协议 → **工具层(lilyco)** → 节点网格 → 记忆市场。

## P0 契约加固（当日完成 ✅）

| 动作 | 证据 |
|---|---|
| lly 锁定 lilyco git 依赖 `rev=fe28262f`（消除 HEAD 漂移） | lly@`3ccbf2d` |
| 归档 7 个零活动 fork（xmake-repo / workbuddy / ATAC / pigma / reverse-skill / cargo-quad-apk / my-girlfriend-jingtian-latex） | gh repo archive |
| 归档 2 个空仓（lfs-cors-test / dev_android_env_in_windows） | gh repo archive |
| 本路线图落库 | 本文件 |

> 纠偏记录：imgui / BitNet / WSL2-Linux-Kernel 实测非 fork（独立仓），BitNet 为 P3 参考件、WSL2 有 ⭐3 —— 不归档。

## P1 契约上收（进行中）

mpkg 格式上收 lystack 为单一契约源：
- `lystack/proto/mpkg/mpkg.schema.json`（JSON Schema 2020-12）
- golden 向量（样例包 → 期望 content-id / blob sha256，canon_json = 递归键排序 + 紧凑序列化）
- lilyco-mpkg 内嵌 golden 一致性测试；cache-node 验证器对齐同一向量
- 消除三处实现（lilyco-mpkg / cache-node / mpkg-registry 前端）的格式漂移

## P2 结构重组（进行中）

- **lyco_agent 拆仓**：`cloudstudio/` 运维脚本（a10_* 系列）迁私有 ops 仓，主仓只留 agent 核心 + 文档 —— 治理 374 文件低内聚
- **cache-node webrtc feature 化**：`mesh = ["dep:webrtc"]` 默认关，恢复"musl 单文件零运行时依赖"承诺；补 CI（fmt/clippy/test × 有无 feature）

## P3 旗舰融合（进行中）

**bitnet-rs ↔ lilyco 采样桥**：用本地 BitNet b1.58 推理实现 lilyco `HostBridge`（`sampling/createMessage` 的本地后端）。
闭环：pet 聊天 → ctx.sample → bitnet-rs 本地推理 → pet-say 语音 —— **全网首个从零 Rust MoE 桌宠全离线闭环**。

## 内聚/耦合守则（从全库评估沉淀）

1. 跨仓格式必须有单一 schema 源 + golden 测试（P1）
2. git 依赖必须锁 rev/tag（P0）
3. 平台重依赖（webrtc / crossterm / tokio）一律 feature 门控默认关（P2）
4. 运维脚本与产品代码分仓（P2）
5. 对外工具一律走 lilyco 工具层（App derive + 安全门 + 遥测），不再手写 CLI
