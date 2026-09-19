# lilyco-bitnet — P3 采样桥：bitnet-rs 本地 LLM 实现 HostBridge

用本地 [bitnet-rs](https://github.com/lilyco-42/bitnet-rs)（BitNet b1.58-2B 推理引擎）
实现 `lilyco_core::HostBridge`，打通

**pet 聊天 → `ctx.sample` → 本地 BitNet 推理 → pet-say 语音**

的全离线闭环——模型在设备上，数据不出设备。

## 快速开始

```bash
# 1. 下载模型（约 1.1GB，一次性）
huggingface-cli download microsoft/bitnet-b1.58-2B-4T-gguf \
  ggml-model-i2_s.gguf --local-dir ~/.lilyco/models
mv ~/.lilyco/models/ggml-model-i2_s.gguf ~/.lilyco/models/bitnet-b1.58-2b-i2_s.gguf

# 2. CLI 本地推理（T0 只读，全离线）
cargo run -p lilyco-bitnet -- bitnet-ask --prompt "你好" --max-tokens 128

# 3. host 桥注入演示（工具内 ctx.sample → 本地推理）
cargo run -p lilyco-bitnet -- bitnet-chat-demo --prompt "介绍一下你自己"

# 4. MCP 服务器：Agent 直接可调（离线采样）
cargo run -p lilyco-bitnet -- --mcp
```

模型路径解析优先级：`--model <路径>` > 环境变量 `BITNET_MODEL` >
`~/.lilyco/models/bitnet-b1.58-2b-i2_s.gguf`。缺失时报**带指引的错误**
（去哪下载、放哪、怎么指定），见 [docs/BITNET_BRIDGE.md](docs/BITNET_BRIDGE.md)。

## 命令

| 命令 | 分级 | 说明 |
|------|------|------|
| `bitnet-ask` | T0 | 直接本地推理，上报 `bitnet.tps` / `bitnet.tokens` 遥测 |
| `bitnet-chat-demo` | T0 | 演示 host 桥注入：工具内 `ctx.sample()` → 本地推理 |

同一二进制 `lbitnet` = CLI + TUI + Web + MCP 四端。

## 桥注入示例

```rust
use std::sync::Arc;
use lilyco::prelude::*;
use lilyco_bitnet::BitNetBridge;

// 与 MCP 服务器 tools/call 完全相同的注入点
let bridge: Arc<dyn HostBridge> = Arc::new(BitNetBridge::new("~/.lilyco/models/bitnet.gguf"));
let outcome = lilyco::prelude::execute_with(handler, args, Some(bridge));
// handler 内：ctx.sample("…", 256) 走本地 BitNet；ctx.roots() 返回模型目录
```

## 测试策略（CI 零真模型）

- 假采样器（`Sampler` trait mock）覆盖 sample 截断契约 / 错误路径 / 遥测发射
- 极小手工 gguf fixture（~78KB，进程内构造）覆盖 加载→前向→采样→解码 真链路
- 真模型端到端 `#[ignore]`：设 `BITNET_MODEL` 后
  `cargo test -p lilyco-bitnet -- --ignored`
