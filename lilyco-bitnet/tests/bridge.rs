//! P3 采样桥端到端（CI 零真模型）
//!
//! 覆盖三层：
//! 1. **假采样器**（trait mock）：sample 截断契约 / 错误路径 / 遥测发射
//! 2. **executor 注入**：`execute_with` + `BitNetBridge` —— 与 MCP tools/call 同路径
//! 3. **极小手工 gguf fixture**（~78KB，进程内构造）：真·加载→前向→采样→解码
//!    全链路，argmax 落在构造的 'A' token 上，输出确定性 "AAA"
//!
//! 真模型（1-2GB）的端到端测试标 `#[ignore]`，CI 永不下载。

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use lilyco::prelude::*;
use lilyco_bitnet::{
    ask_with_sampler, BitNetAsk, BitNetBridge, BitnetSampler, Generation, Sampler, MODEL_ENV,
};

// ── 假采样器（Arc 包裹调用记录，装箱进桥后仍可读） ────────

struct MockInner {
    calls: Mutex<Vec<(String, u32)>>,
    /// 每次 generate 模拟生成的 token 数
    gen_tokens: usize,
    /// 是否拒绝生成（错误路径）
    fail: bool,
}

struct SharedMock {
    inner: Arc<MockInner>,
}

impl SharedMock {
    fn new(gen_tokens: usize) -> Self {
        Self {
            inner: Arc::new(MockInner {
                calls: Mutex::new(Vec::new()),
                gen_tokens,
                fail: false,
            }),
        }
    }

    /// 恒定失败的假采样器（错误路径）
    fn failing() -> Self {
        Self {
            inner: Arc::new(MockInner {
                calls: Mutex::new(Vec::new()),
                gen_tokens: 0,
                fail: true,
            }),
        }
    }

    /// 装箱为 trait 对象；克隆共享记录（构造处仍可 calls() 读调用历史）
    fn boxed(&self) -> Box<dyn Sampler> {
        Box::new(SharedMock {
            inner: Arc::clone(&self.inner),
        })
    }

    fn calls(&self) -> Vec<(String, u32)> {
        self.inner.calls.lock().unwrap().clone()
    }
}

impl Sampler for SharedMock {
    fn generate(&self, prompt: &str, max_tokens: u32) -> Result<Generation, AppError> {
        self.inner
            .calls
            .lock()
            .unwrap()
            .push((prompt.to_string(), max_tokens));
        if self.inner.fail {
            return Err(AppError::Runtime("模拟推理失败".into()));
        }
        let n = (max_tokens as usize).min(self.inner.gen_tokens);
        // 文本长度与 token 数一致（每 token 一个字符），方便验证截断契约
        Ok(Generation {
            text: "A".repeat(n),
            prompt_tokens: 2,
            gen_tokens: n,
            tps: 12.5,
        })
    }
}

// ── 1. 桥接（HostBridge）契约 ─────────────────────────────

#[test]
fn sample_delegates_and_passes_max_tokens_through() {
    let mock = SharedMock::new(8);
    let bridge = BitNetBridge::with_sampler(mock.boxed());
    let text = bridge.sample("你好", 8).unwrap();
    // 截断契约：返回文本对应 token 数 ≤ max_tokens
    assert_eq!(text.chars().count(), 8);
}

#[test]
fn sample_records_call_args_exactly() {
    let mock = SharedMock::new(4);
    let bridge = BitNetBridge::with_sampler(mock.boxed());
    let text = bridge.sample("hi", 7).unwrap();
    assert_eq!(text, "AAAA");
    let calls = mock.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(
        calls[0],
        ("hi".to_string(), 7u32),
        "max_tokens 必须原样透传"
    );
}

#[test]
fn sample_never_exceeds_max_tokens_on_either_side() {
    // 采样器想生成 10，上限 4 → 4；采样器只想生成 4，上限 10 → 4
    let capped = SharedMock::new(10);
    let out = BitNetBridge::with_sampler(capped.boxed())
        .sample("x", 4)
        .unwrap();
    assert_eq!(out.chars().count(), 4);

    let lazy = SharedMock::new(4);
    let out = BitNetBridge::with_sampler(lazy.boxed())
        .sample("x", 10)
        .unwrap();
    assert_eq!(out.chars().count(), 4);
}

#[test]
fn sample_surfaces_sampler_error() {
    let mock = SharedMock::failing();
    let err = BitNetBridge::with_sampler(mock.boxed())
        .sample("hi", 8)
        .unwrap_err();
    assert!(err.to_string().contains("模拟推理失败"), "{err}");
}

#[test]
fn roots_expose_model_directory() {
    let bridge = BitNetBridge::with_sampler(SharedMock::new(1).boxed())
        .with_model_path("/tmp/models/bitnet.gguf");
    let roots = bridge.roots().unwrap();
    assert_eq!(roots.len(), 1);
    assert!(roots[0].0.starts_with("file://"), "{}", roots[0].0);
    assert_eq!(roots[0].1, "bitnet-model");
}

// ── 2. executor 注入路径（与 MCP tools/call 同构） ────────

#[test]
fn execute_with_injects_bitnet_bridge_into_ctx_sample() {
    let mock = SharedMock::new(5);
    let bridge: Arc<dyn HostBridge> =
        Arc::new(BitNetBridge::with_sampler(mock.boxed()).with_model_path("/tmp/m.gguf"));
    let handler: Handler = Arc::new(|ctx: &Context, _args: &serde_json::Value| {
        let reply = ctx.sample("ping", 9)?;
        let roots = ctx.roots()?;
        Ok(serde_json::json!({ "reply": reply, "roots": roots.len() }))
    });
    let outcome = execute_with(handler, serde_json::json!({}), Some(bridge));
    let r = outcome.result.expect("注入桥后 ctx.sample 必须成功");
    assert_eq!(r["reply"], "AAAAA");
    assert_eq!(r["roots"], 1);
    // 采样器收到的 max_tokens = 9（透传）
    assert_eq!(mock.calls()[0].1, 9);
}

// ── 3. bitnet-ask 应用：遥测发射与错误路径 ────────────────

fn ask_app(prompt: &str, model: &str) -> BitNetAsk {
    let map: HashMap<String, serde_json::Value> = HashMap::from([
        ("prompt".to_string(), serde_json::json!(prompt)),
        ("model".to_string(), serde_json::json!(model)),
    ]);
    BitNetAsk::from_args(&map).unwrap()
}

#[test]
fn ask_emits_telemetry_and_done() {
    let (tx, rx) = std::sync::mpsc::channel();
    let ctx = Context::new_test(tx);
    let app = ask_app("你好", "unused.gguf");
    let mock = SharedMock::new(3);
    let sampler = mock.boxed();

    let r = ask_with_sampler(&app, &ctx, &*sampler).unwrap();

    // Done 结果：生成文本 + token 统计
    assert_eq!(r["text"], "AAA");
    assert_eq!(r["generated_tokens"], 3);
    assert_eq!(r["prompt_tokens"], 2);

    // ctx 内部持有 progress sender，先 drop 关闭 channel 再收集事件（否则 iter 死等）
    drop(ctx);
    let events: Vec<Progress> = rx.iter().collect();
    // 遥测点 1：bitnet.tps
    assert!(
        events.iter().any(|e| matches!(
            e,
            Progress::Telemetry { key, value }
                if key == "bitnet.tps" && value.as_f64() == Some(12.5)
        )),
        "缺少 bitnet.tps 遥测: {events:?}"
    );
    // 遥测点 2：bitnet.tokens
    assert!(
        events.iter().any(|e| matches!(
            e,
            Progress::Telemetry { key, value }
                if key == "bitnet.tokens" && value["generated"] == 3
        )),
        "缺少 bitnet.tokens 遥测: {events:?}"
    );
    // 终态：Done
    assert!(events.iter().any(|e| matches!(e, Progress::Done { .. })));
}

#[test]
fn ask_missing_model_reports_guided_error() {
    // 真采样器 + 不存在的路径 → 带下载指引的错误（无需真模型）
    let (tx, _rx) = std::sync::mpsc::channel();
    let ctx = Context::new_test(tx);
    let app = ask_app("你好", "/nonexistent/bitnet.gguf");
    let path = lilyco_bitnet::resolve_model_path(Some(Path::new("/nonexistent/bitnet.gguf")));
    let sampler = BitnetSampler::new(path, 0.0);
    let err = ask_with_sampler(&app, &ctx, &sampler).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("BitNet 模型未找到"), "{msg}");
    assert!(
        msg.contains("huggingface.co/microsoft/bitnet-b1.58-2B-4T-gguf"),
        "{msg}"
    );
    assert!(msg.contains("BITNET_MODEL"), "{msg}");
}

#[test]
fn resolve_model_path_prefers_arg_then_env() {
    // 参数优先
    assert_eq!(
        lilyco_bitnet::resolve_model_path(Some(Path::new("/a/b.gguf"))),
        PathBuf::from("/a/b.gguf")
    );
    // 空参数回退默认路径
    assert!(lilyco_bitnet::resolve_model_path(Some(Path::new("")))
        .ends_with("bitnet-b1.58-2b-i2_s.gguf"));
    // 环境变量次之
    std::env::set_var(MODEL_ENV, "/env/path.gguf");
    assert_eq!(
        lilyco_bitnet::resolve_model_path(None),
        PathBuf::from("/env/path.gguf")
    );
    std::env::remove_var(MODEL_ENV);
}

// ── 4. 极小手工 gguf fixture：真推理链路 ──────────────────
//
// 1 层、n_embd=64、n_ff=128、vocab=258 的 BitNet b1.58 结构：
// 全部 I2_S 权重置零（scale=0）→ logits 恒为 0 → argmax 取最后一个
// token（构造为 'A'）→ 贪心解码输出 "AAA"（max_tokens=3）。
// 覆盖：GGUF 解析、张量加载、前向、采样、解码的完整桥下链路。

const GGML_TYPE_F32: u32 = 0;
const GGML_TYPE_I2_S: u32 = 36;
const N_EMBD: u64 = 64;
const N_FF: u64 = 128;
const VOCAB: u64 = 258;

fn w_u32(v: u32, out: &mut Vec<u8>) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn w_u64(v: u64, out: &mut Vec<u8>) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn w_f32(v: f32, out: &mut Vec<u8>) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn w_str(s: &str, out: &mut Vec<u8>) {
    w_u64(s.len() as u64, out);
    out.extend_from_slice(s.as_bytes());
}
fn w_kv_str(key: &str, val: &str, out: &mut Vec<u8>) {
    w_str(key, out);
    w_u32(8, out); // GGUF type: string
    w_str(val, out);
}
fn w_kv_u32(key: &str, val: u32, out: &mut Vec<u8>) {
    w_str(key, out);
    w_u32(4, out); // GGUF type: u32
    w_u32(val, out);
}
fn w_kv_f32(key: &str, val: f32, out: &mut Vec<u8>) {
    w_str(key, out);
    w_u32(6, out); // GGUF type: f32
    w_f32(val, out);
}
fn w_kv_str_array(key: &str, items: &[String], out: &mut Vec<u8>) {
    w_str(key, out);
    w_u32(9, out); // GGUF type: array
    w_u32(8, out); // elem type: string
    w_u64(items.len() as u64, out);
    for it in items {
        w_str(it, out);
    }
}
fn w_tensor_info(name: &str, dims: &[u64], ty: u32, offset: u64, out: &mut Vec<u8>) {
    w_str(name, out);
    w_u32(dims.len() as u32, out);
    for d in dims {
        w_u64(*d, out);
    }
    w_u32(ty, out);
    w_u64(offset, out);
}

/// fixture 词表：id0="<s>"（BOS）、id1="</s>"（EOS）、随后 255 个字节映射字符、
/// 最后一个 id=257 是 'A' —— 零 logits 的 argmax（max_by 取最后一个最大值）
fn fixture_vocab() -> Vec<String> {
    let mut tokens = vec!["<s>".to_string(), "</s>".to_string()];
    for (byte, ch) in bitnet_rs::tokenizer::byte_to_unicode() {
        if byte == b'A' {
            continue;
        }
        tokens.push(ch.to_string());
    }
    tokens.push("A".to_string());
    tokens
}

fn f32_tensor_data(n: u64) -> Vec<u8> {
    vec![0u8; (n * 4) as usize]
}

/// I2_S 张量数据：packed(n/4 字节，全零) + f32 scale(0.0) + 尾部对齐（≥ +32 字节）
fn i2s_tensor_data(n: u64) -> Vec<u8> {
    let packed = n / 4;
    let total = ((packed + 4 + 31) / 32) * 32;
    assert!(total >= packed + 32, "I2_S 布局要求张量尾 32 字节");
    vec![0u8; total as usize]
}

/// 构造极小 BitNet gguf（约 78KB）并写入文件
fn write_tiny_gguf(path: &Path) {
    let mut head: Vec<u8> = Vec::new();
    // header
    head.extend_from_slice(&0x4655_4747u32.to_le_bytes()); // "GGUF"
    w_u32(3, &mut head); // version
    w_u64(13, &mut head); // n_tensors
    w_u64(16, &mut head); // n_kv

    // kv 元数据（n_kv = 16）
    let p = "bitnet-b1.58.";
    w_kv_str("general.architecture", "bitnet-b1.58", &mut head);
    w_kv_u32(&format!("{p}block_count"), 1, &mut head);
    w_kv_u32(&format!("{p}embedding_length"), N_EMBD as u32, &mut head);
    w_kv_u32(&format!("{p}attention.head_count"), 1, &mut head);
    w_kv_u32(&format!("{p}attention.head_count_kv"), 1, &mut head);
    w_kv_u32(
        &format!("{p}rope.dimension_count"),
        N_EMBD as u32,
        &mut head,
    );
    w_kv_u32(&format!("{p}feed_forward_length"), N_FF as u32, &mut head);
    w_kv_u32(&format!("{p}vocab_size"), VOCAB as u32, &mut head);
    w_kv_f32(&format!("{p}rope.freq_base"), 10000.0, &mut head);
    w_kv_f32(&format!("{p}rope.frequency_scale"), 1.0, &mut head);
    w_kv_f32(
        &format!("{p}attention.layer_norm_rms_epsilon"),
        1e-5,
        &mut head,
    );
    w_kv_str_array("tokenizer.ggml.tokens", &fixture_vocab(), &mut head);
    w_kv_str_array("tokenizer.ggml.merges", &[], &mut head);
    w_kv_str_array("tokenizer.ggml.special_tokens", &[], &mut head);
    w_kv_u32("tokenizer.ggml.bos_token_id", 0, &mut head);
    w_kv_u32("tokenizer.ggml.eos_token_id", 1, &mut head);

    // 张量目录（offset 相对数据区起点；每个张量大小都是 32 的倍数 → offset 天然对齐）
    let mut infos: Vec<u8> = Vec::new();
    let mut data: Vec<u8> = Vec::new();
    let mut offset = 0u64;
    {
        let mut tensor = |name: &str, dims: &[u64], ty: u32, blob: Vec<u8>| {
            w_tensor_info(name, dims, ty, offset, &mut infos);
            offset += blob.len() as u64;
            data.extend_from_slice(&blob);
        };

        tensor(
            "token_embd.weight",
            &[N_EMBD, VOCAB],
            GGML_TYPE_F32,
            f32_tensor_data(N_EMBD * VOCAB),
        );
        tensor(
            "output_norm.weight",
            &[N_EMBD],
            GGML_TYPE_F32,
            f32_tensor_data(N_EMBD),
        );
        for norm in ["attn_norm", "attn_sub_norm", "ffn_norm", "ffn_sub_norm"] {
            tensor(
                &format!("blk.0.{norm}.weight"),
                &[N_EMBD],
                GGML_TYPE_F32,
                f32_tensor_data(N_EMBD),
            );
        }
        for name in ["attn_q", "attn_k", "attn_v", "attn_output"] {
            let n = N_EMBD * N_EMBD;
            tensor(
                &format!("blk.0.{name}.weight"),
                &[N_EMBD, N_EMBD],
                GGML_TYPE_I2_S,
                i2s_tensor_data(n),
            );
        }
        for name in ["ffn_gate", "ffn_up"] {
            let n = N_EMBD * N_FF;
            tensor(
                &format!("blk.0.{name}.weight"),
                &[N_EMBD, N_FF],
                GGML_TYPE_I2_S,
                i2s_tensor_data(n),
            );
        }
        let n_down = N_FF * N_EMBD;
        tensor(
            "blk.0.ffn_down.weight",
            &[N_FF, N_EMBD],
            GGML_TYPE_I2_S,
            i2s_tensor_data(n_down),
        );
    }

    // 组装：header + infos，补齐到 32 字节数据区对齐
    let mut buf = head;
    buf.extend_from_slice(&infos);
    let pad = (32 - (buf.len() % 32)) % 32;
    buf.extend(vec![0u8; pad]);
    assert_eq!(buf.len() % 32, 0, "数据区必须 32 字节对齐");
    buf.extend_from_slice(&data);

    let mut f = std::fs::File::create(path).unwrap();
    f.write_all(&buf).unwrap();
}

#[test]
fn tiny_gguf_fixture_full_inference_roundtrip() {
    let dir = std::env::temp_dir().join(format!("lilyco-bitnet-fixture-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("tiny-bitnet.gguf");
    write_tiny_gguf(&path);

    // 走完整桥：加载（惰性）→ 前向 → 贪心采样 → 解码
    let bridge = BitNetBridge::new(&path);
    let out = bridge.sample("hello", 3).unwrap();
    assert_eq!(out, "AAA", "零 logits 的 argmax 应落在构造的 'A' token 上");
    assert!(bridge.roots().unwrap()[0].0.starts_with("file://"));

    let _ = std::fs::remove_dir_all(&dir);
}

// ── 5. 真模型端到端（#[ignore]，CI 永不下载） ─────────────

#[test]
#[ignore = "需要真模型（约 1.1GB gguf）：设置 BITNET_MODEL 后 `cargo test -p lilyco-bitnet -- --ignored`"]
fn real_model_end_to_end() {
    let path = std::env::var(MODEL_ENV).unwrap_or_else(|_| {
        panic!("请设置 {MODEL_ENV} 指向 BitNet b1.58-2B gguf（下载见 docs/BITNET_BRIDGE.md）")
    });
    let bridge = BitNetBridge::new(PathBuf::from(&path));
    let out = bridge.sample("用一句话介绍你自己", 32).unwrap();
    assert!(!out.trim().is_empty(), "真模型不应返回空文本");
    println!("BitNet reply: {out}");
}
