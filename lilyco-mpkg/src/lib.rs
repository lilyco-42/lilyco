//! # Lilyco MPKG — 记忆包（mpkg）接入 lilyco 能力总线
//!
//! lystack 生态融合 Track 1：把 mpkg（记忆包）做成受安全门控的 lilyco 工具。
//! 同一 struct 派生 CLI / TUI / Web / MCP 四端，写路径过 T2 能力令牌门，读/验
//! 路径 T0 直通，全程走 P1 遥测流。
//!
//! ## mpkg 最小格式（对齐 lystack mpkg v0.1，字段名与 cache-node 验证器一致）
//!
//! 包文件 = 单个 UTF-8 JSON，即 mpkg v0.1 content-id 的计算输入直接落盘：
//!
//! ```json
//! {
//!   "manifest": {
//!     "mpkg": "0.1",
//!     "name": "make-snake-game",
//!     "version": "0.1.0",
//!     "intent": "回放后得到可玩的贪吃蛇",
//!     "steps": [ { "run": "echo snake", "expect": { "exit": 0 } } ],
//!     "verify": [ "test -f snake.py" ]
//!   },
//!   "files": { "artifacts/snake.py": "<64位 sha256 hex>" }
//! }
//! ```
//!
//! - **content-id**：`id = "sha256:" + sha256_hex(canon_json(包文件))`，与
//!   cache-node `package_id` 逐字节一致（canon = 键排序 + 紧凑分隔符 + UTF-8）。
//! - **存储**：内容寻址 KV——blob 以原始字节 sha256 命名（`blobs/<hex>.mpkg`），
//!   另存名字索引（`index/<name>.json`），按哈希或按包名取出皆可。
//! - **最小子集差异**：无 zip 容器（`files` 只存哈希引用，不内嵌文件本体）；
//!   `steps`/`verify` 只做结构校验，不真实执行——执行任意外部命令属 T3 级
//!   动作，交由 cache-node verify 在受控环境完成。
//! - **sha256**：自研零依赖实现（见 [`sha256`]，FIPS 180-4 官方向量锁定）。
//!
//! ## 安全模型
//!
//! | 命令 | 分级 | 理由 |
//! |---|---|---|
//! | `mpkg-put` | T2 令牌 | 写记忆是敏感操作：污染 agent 的长期记忆比写 PLC 更难回滚 |
//! | `mpkg-get` | T0 只读 | 读存储不产生副作用 |
//! | `mpkg-verify` | T0 只读 | 纯校验，重算哈希不改任何状态 |
//!
//! `MpkgPut` 注册进 [`lilyco::__core::registry::Registry`] 即被 P0 安全门包住：
//! 默认 `DenyElevated` 策略下自动化面直接拒绝，且拒绝发生在写盘之前。

pub mod app;
pub mod pack;
pub mod sha256;

pub use app::{MpkgGet, MpkgPut, MpkgVerify};
pub use pack::{
    canon, normalize_hash, package_id, validate, verify_pack, PutInfo, Resolved, Store,
};
