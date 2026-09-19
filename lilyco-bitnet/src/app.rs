//! 派生应用——本地 BitNet 推理的两个入口（同一 struct 派生 CLI/TUI/Web/MCP 四端）
//!
//! - [`BitNetAsk`]（`bitnet-ask`）：T0 只读本地推理，直接驱动采样器并上报遥测
//! - [`BitNetChatDemo`]（`bitnet-chat-demo`）：演示 **host 桥注入**——工具内经
//!   `ctx.sample()` 反向调用本地 BitNet（桥 = [`BitNetBridge`] 自身，本地自举闭环）

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use lilyco::prelude::*;

use crate::{
    model_missing_error, resolve_model_path, BitNetBridge, BitnetSampler, Sampler,
    DEFAULT_MAX_TOKENS, DEFAULT_TEMPERATURE,
};

/// 问本地 BitNet（T0 只读推理，全离线，逐项上报遥测）
#[derive(App)]
#[app(
    name = "bitnet-ask",
    about = "问本地 BitNet b1.58 模型（全离线推理），上报 bitnet.tps / bitnet.tokens 遥测",
    run = "run_ask"
)]
pub struct BitNetAsk {
    /// 提问内容（原样作为 prompt 发给本地模型）
    prompt: String,
    /// 生成 token 上限（默认 256）
    max_tokens: Option<u32>,
    /// 模型 gguf 路径（默认 $BITNET_MODEL 或 ~/.lilyco/models/…）
    model: Option<PathBuf>,
}

pub fn run_ask(app: &BitNetAsk, ctx: &Context) -> Result<serde_json::Value, AppError> {
    let path = resolve_model_path(app.model.as_deref());
    // 缺模型时尽早给出带指引的错误（不用等到加载阶段）
    if !path.is_file() {
        return Err(model_missing_error(&path));
    }
    let sampler = BitnetSampler::new(path, DEFAULT_TEMPERATURE);
    ask_with_sampler(app, ctx, &sampler)
}

/// `bitnet-ask` 的可注入实现：真模型 / 假采样器共用同一执行与遥测路径
pub fn ask_with_sampler(
    app: &BitNetAsk,
    ctx: &Context,
    sampler: &dyn Sampler,
) -> Result<serde_json::Value, AppError> {
    let max_tokens = app.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS);
    ctx.log(
        LogLevel::Info,
        format!("本地 BitNet 推理中（max_tokens={max_tokens}）…"),
    );
    let t0 = Instant::now();
    let g = sampler.generate(&app.prompt, max_tokens)?;
    let dt_ms = t0.elapsed().as_millis() as u64;

    // P1 遥测：生成速度 + token 统计，Agent 在 MCP progress 通道实时可见
    ctx.telemetry(
        "bitnet.tps",
        serde_json::json!((g.tps * 100.0).round() / 100.0),
    );
    ctx.telemetry(
        "bitnet.tokens",
        serde_json::json!({ "prompt": g.prompt_tokens, "generated": g.gen_tokens }),
    );

    let r = serde_json::json!({
        "text": g.text,
        "prompt_tokens": g.prompt_tokens,
        "generated_tokens": g.gen_tokens,
        "max_tokens": max_tokens,
        "tps": (g.tps * 100.0).round() / 100.0,
    });
    ctx.done(r.clone(), dt_ms);
    Ok(r)
}

/// host 桥注入演示：工具内经 `ctx.sample()` 反向调用本地 BitNet
///
/// 与 MCP 服务器 `tools/call` 完全相同的注入路径
/// （`executor::execute_with(handler, args, Some(host))`），
/// 只是宿主桥换成 [`BitNetBridge`]——形成"本地自举"闭环。
#[derive(App)]
#[app(
    name = "bitnet-chat-demo",
    about = "演示 host 桥注入：工具内经 ctx.sample() 走本地 BitNet（本地自举闭环）",
    run = "run_chat_demo"
)]
pub struct BitNetChatDemo {
    /// 要交给 ctx.sample 的问题
    prompt: String,
    /// 生成 token 上限（默认 64）
    max_tokens: Option<u32>,
    /// 模型 gguf 路径（默认 $BITNET_MODEL 或 ~/.lilyco/models/…）
    model: Option<PathBuf>,
}

pub fn run_chat_demo(app: &BitNetChatDemo, ctx: &Context) -> Result<serde_json::Value, AppError> {
    let path = resolve_model_path(app.model.as_deref());
    if !path.is_file() {
        return Err(model_missing_error(&path));
    }
    let bridge: Arc<dyn HostBridge> = Arc::new(BitNetBridge::new(path));
    let prompt = app.prompt.clone();
    let max_tokens = app.max_tokens.unwrap_or(64);

    // handler 与 MCP 服务器执行的 tools/call 代码同构：在工具内反向采样 + roots
    let handler: Handler = Arc::new(move |inner: &Context, _args: &serde_json::Value| {
        let reply = inner.sample(&prompt, max_tokens)?;
        let roots = inner
            .roots()?
            .into_iter()
            .map(|(uri, name)| serde_json::json!({ "uri": uri, "name": name }))
            .collect::<Vec<_>>();
        Ok(serde_json::json!({ "reply": reply, "roots": roots }))
    });

    // 唯一的桥注入点：execute_with 把 BitNetBridge 附加到 handler 的 Context
    let outcome = execute_with(handler, serde_json::json!({}), Some(bridge));
    let r = outcome.result?;
    ctx.done(r.clone(), 0);
    Ok(r)
}
