# BitNet 采样桥 — 全离线 LLM 闭环设计（P3 旗舰）

> pet 聊天 → `ctx.sample` → 本地 BitNet 推理 → pet-say 语音

## 1. 这是什么

`lilyco_core::HostBridge` 是 handler 反向调用宿主能力的唯一接口：

```rust
pub trait HostBridge: Send + Sync {
    fn sample(&self, prompt: &str, max_tokens: u32) -> Result<String, AppError>;
    fn roots(&self) -> Result<Vec<(String, String)>, AppError>;
}
```

此前唯一实现是 `lilyco-mcp` 的 `McpBridge`（把 `sample` 映射为 MCP
`sampling/createMessage` 请求，发给**云端**客户端）。`lilyco-bitnet` 提供
第二个实现 `BitNetBridge`——把 `sample` 落到本地 [bitnet-rs]
(https://github.com/lilyco-42/bitnet-rs) 推理，**无网、无云、无 API key**。

| | McpBridge（云端） | BitNetBridge（本地） |
|---|---|---|
| `sample` | sampling/createMessage → MCP 客户端 | BitNet b1.58-2B 进程内推理 |
| `roots` | 客户端 roots/list | 模型文件所在目录 |
| 依赖 | 客户端声明 sampling 能力 | 一个 ~1.1GB gguf 文件 |
| 隐私 | prompt 出设备 | **数据不出设备** |

## 2. 模型下载与放置

官方 gguf（I2_S 量化，~1.1GB）：

```bash
# 方式 A：huggingface-cli
huggingface-cli download microsoft/bitnet-b1.58-2B-4T-gguf \
  ggml-model-i2_s.gguf --local-dir ~/.lilyco/models

# 方式 B：直接 curl
curl -L -o ~/.lilyco/models/bitnet-b1.58-2b-i2_s.gguf \
  https://huggingface.co/microsoft/bitnet-b1.58-2B-4T-gguf/resolve/main/ggml-model-i2_s.gguf
```

（以 [microsoft/BitNet](https://github.com/microsoft/BitNet) 仓库 README 为准；
也可用其转换脚本产出其他量化版本，bitnet-rs 按官方 I2_S 布局解析。）

放置位置三选一（优先级从高到低）：

1. `--model <路径>` 命令行参数
2. 环境变量 `BITNET_MODEL=<路径>`
3. 默认路径 `~/.lilyco/models/bitnet-b1.58-2b-i2_s.gguf`

模型缺失时 `bitnet-ask` 立即返回带指引的错误（下载地址 / 放置路径 / 文档链接），
不会先跑半截再失败。

## 3. HostBridge 注入

### 3.1 executor 注入点（MCP 同款）

`lilyco_core::executor` 的 `spawn_with` / `execute_with` 接受
`Option<Arc<dyn HostBridge>>`——这是唯一注入点，MCP 服务器在 `tools/call`
时就是这样把 `McpBridge` 接进 handler 的 `Context`：

```rust
use std::sync::Arc;
use lilyco::prelude::*;
use lilyco_bitnet::BitNetBridge;

let bridge: Arc<dyn HostBridge> = Arc::new(BitNetBridge::new(
    std::env::var("BITNET_MODEL").unwrap_or_default(),
));

let handler: Handler = Arc::new(|ctx: &Context, args: &serde_json::Value| {
    // 反向采样：走本地 BitNet，无网络
    let reply = ctx.sample(&args["prompt"].as_str().unwrap_or_default(), 256)?;
    Ok(serde_json::json!({ "reply": reply }))
});

let outcome = execute_with(handler, serde_json::json!({}), Some(bridge));
```

### 3.2 Context 直接附加

```rust
let ctx = ctx.with_host(Arc::new(BitNetBridge::new(model_path)));
let reply = ctx.sample("你好", 256)?;   // 本地推理
let roots = ctx.roots()?;               // 模型目录 file:// uri
```

### 3.3 开箱即用的演示

```bash
lbitnet bitnet-chat-demo --prompt "介绍一下你自己"
# bitnet-chat-demo 内部：handler → ctx.sample → BitNetBridge → 本地推理
# （execute_with 注入，与 MCP tools/call 路径完全一致——本地自举闭环）
```

## 4. pet 全离线闭环路线

```
┌─────────┐   聊天文本   ┌──────────────────┐   回复文本   ┌──────────┐
│ pet 聊天 │ ──────────→ │ ctx.sample        │ ──────────→ │ pet-say  │
│ (TUI/GUI)│             │  └→ BitNetBridge  │             │ (TTS 语音)│
└─────────┘             │      └→ bitnet-rs │             └──────────┘
                        └──────────────────┘
   设备内全链路：麦克风/键盘 → 本地 LLM → 本地 TTS，零云端依赖
```

落地步骤：

1. **P3（本 PR）**：`BitNetBridge` 可用，`lbitnet --mcp` 让 Agent 拥有离线采样
2. **P4**：`lilyco-pet` 的聊天 handler 持有 `BitNetBridge`（`with_host` 注入），
   聊天回复改走 `ctx.sample`；`pet-say` 拿同一文本出语音
3. **P5**：`pet-act` 动作决策也经 `ctx.sample`（BitNet 输出结构化动作 JSON），
   做到"断网宠物照样陪你聊、照样动"

BitNet b1.58-2B 是三值权重（{-1,0,+1}）模型，CPU 上 ~0.5s/token（NEON），
桌面 x86 标量参考实现更慢——适合短回复场景（宠物一句话 ≤ 32 token），
`bitnet-ask` 默认 `max_tokens=256`，pet 集成建议压到 32。

## 5. 实现细节

- **模型生命周期**：`BitnetSampler` 首次 `generate` 惰性加载，`Mutex` 持有到
  生成结束——加载只发生一次，且推理期间禁止重入（天然串行）
- **确定性**：桥默认温度 0.0（贪心解码），工具/Agent 采样要的是稳定输出
- **截断契约**：`Sampler::generate` 保证 `gen_tokens ≤ max_tokens`
- **遥测**：`bitnet-ask` 上报 `bitnet.tps`（tokens/s，含 prefill 实测）与
  `bitnet.tokens`（{prompt, generated}）
- **CI 零下载**：假采样器覆盖桥接逻辑 + ~78KB 手工 gguf fixture 覆盖真推理
  链路（加载→前向→采样→解码），真模型测试 `#[ignore]`（`BITNET_MODEL` 就位后
  `cargo test -p lilyco-bitnet -- --ignored`）

## 6. bitnet-rs 依赖形态

`lilyco-bitnet` 依赖 lib 形态的 bitnet-rs（`Model::load / generate`），
对应 bitnet-rs PR「最小 API 补丁 — 推理层提升进 lib」（**#1，已合并进 main**）：

```toml
bitnet-rs = { git = "https://github.com/lilyco-42/bitnet-rs", rev = "6e12ab9e8ad4c99a88cd0ffec9f0d08b1d507f31" }
```

按组织惯例**锁 rev**（不用分支引用，避免上游推进时内核被静默换掉）。

## 7. 推理内核归属（唯一结论，2026-10-11）

生态里出现过「BitNet 有三份实现，需要定以谁为准」的说法。核实后的归属如下 ——
**不是三选一，是三者各司其职，其中只有一个是 lilyco 自有内核**：

| # | 位置 | 角色 | 归属 |
|---|---|---|---|
| 1 | `lilyco/lilyco-bitnet/` | **桥**：`BitNetBridge` 实现 `HostBridge::sample` | 本仓（lilyco-42） |
| 2 | `lilyco-42/bitnet-rs` | **推理内核**：`Model::load / generate` | lilyco-42 自研（port 自 bitnet.cpp） |
| 3 | `lilyco-42/lyco_chat/src/core/bitnet.rs` | **TinyGPT 自用内核**：三值量化 + 256 项 LUT matmul | 第三方（Kevin Thomas，MIT），服务 TinyGPT |

**结论**

- **`lilyco-bitnet`（桥）的推理内核唯一指向 `bitnet-rs`。** 硬证据在 `src/lib.rs`：

  ```rust
  Loaded(Box<bitnet_rs::model::Model>)                 // lib.rs:114
  bitnet_rs::model::Model::load(&self.model_path)      // lib.rs:145
  ```

  以及 `Cargo.toml` 的 `bitnet-rs = { git = "…/bitnet-rs", rev = "6e12ab9e…" }`（见 §6）。

- **`lyco_chat/src/core/bitnet.rs` 不是 lilyco 的第二套内核。** 文件头署名
  `Copyright (c) 2025 Kevin Thomas`（MIT），自述
  *"Ports the bitnet.cpp i2_s scheme … for TinyGPT"*，服务于 `lyco_chat` 自己的
  TinyGPT 模型。它与 `lilyco-bitnet` 之间**没有任何调用关系**（互不依赖）。

- 因此不存在「两套 lilyco 内核需要对齐」的重复问题：**lilyco 自有内核只有
  `bitnet-rs` 一个**；`lyco_chat` 那份是它自己（教学向 TinyGPT）的第三方实现，
  各自独立演进，**无需对齐接口**。

> 若将来要统一（例如让 TinyGPT 复用 bitnet-rs 的 I2_S 解析），那是一次**独立重构**，
> 不属于「桥接层归属」问题，需单独立项、单独验收。
