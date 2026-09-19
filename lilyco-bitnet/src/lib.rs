//! # Lilyco BitNet 采样桥 — 全离线 LLM 后端
//!
//! 用本地 [bitnet-rs](https://github.com/lilyco-42/bitnet-rs)（BitNet b1.58-2B
//! 推理引擎）实现 [`lilyco::__core::context::HostBridge`]，打通
//! **pet 聊天 → ctx.sample → 本地 BitNet 推理 → pet-say 语音** 的全离线闭环。
//!
//! 设计要点：
//! - **进程内 lib 复用**：直接依赖 bitnet-rs 的 `Model::load / generate`，
//!   不起子进程（lyco 信条：最小化验证，原子化构建）
//! - **惰性加载一次**：[`BitnetSampler`] 首次 `generate` 时加载 gguf，
//!   `Mutex` 持有到生成结束——推理期间天然禁止重入
//! - **可测性**：推理细节挡在 [`Sampler`] trait 后面，CI 用假采样器 +
//!   极小手工 gguf fixture 覆盖桥接逻辑，**绝不下载 1-2GB 真模型**
//! - **T0 只读**：`bitnet-ask` 无副作用（纯本地推理），自动化面直接放行
//!
//! ```ignore
//! use lilyco_bitnet::BitNetBridge;
//! use std::sync::Arc;
//!
//! // 注入点与 MCP 服务器完全一致（executor::spawn_with / execute_with）：
//! let bridge = Arc::new(BitNetBridge::new("~/.lilyco/models/bitnet.gguf"));
//! let ctx = ctx.with_host(bridge);          // 之后 ctx.sample() 走本地推理
//! let reply = ctx.sample("你好", 256)?;
//! ```

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use lilyco::__core::context::HostBridge;
use lilyco::__core::error::AppError;

pub mod app;

pub use app::{ask_with_sampler, BitNetAsk, BitNetChatDemo};

/// 模型路径环境变量（`--model` 参数 > `BITNET_MODEL` > 默认路径）
pub const MODEL_ENV: &str = "BITNET_MODEL";

/// 默认采样温度：0 = 贪心解码（确定性，工具 / Agent 采样推荐）
pub const DEFAULT_TEMPERATURE: f32 = 0.0;

/// `bitnet-ask` 默认生成上限
pub const DEFAULT_MAX_TOKENS: u32 = 256;

/// 默认模型存放路径：`~/.lilyco/models/bitnet-b1.58-2b-i2_s.gguf`
pub fn default_model_path() -> PathBuf {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_default();
    PathBuf::from(home)
        .join(".lilyco")
        .join("models")
        .join("bitnet-b1.58-2b-i2_s.gguf")
}

/// 解析模型路径：显式参数（非空）> `BITNET_MODEL` > 默认路径
pub fn resolve_model_path(arg: Option<&Path>) -> PathBuf {
    if let Some(p) = arg {
        if !p.as_os_str().is_empty() {
            return p.to_path_buf();
        }
    }
    if let Ok(env_path) = std::env::var(MODEL_ENV) {
        if !env_path.is_empty() {
            return PathBuf::from(env_path);
        }
    }
    default_model_path()
}

/// 模型缺失时的带指引错误（去哪下载、放哪、怎么指定）
pub fn model_missing_error(path: &Path) -> AppError {
    AppError::Runtime(format!(
        "BitNet 模型未找到: {}\n\
         指引：\n\
         1. 下载官方 gguf（约 1.1GB）：\n\
            https://huggingface.co/microsoft/bitnet-b1.58-2B-4T-gguf （文件 ggml-model-i2_s.gguf）\n\
         2. 放到默认位置 ~/.lilyco/models/bitnet-b1.58-2b-i2_s.gguf，\n\
            或用 --model <路径> 指定，或设置环境变量 {MODEL_ENV}=<路径>\n\
         3. 完整文档见 lilyco-bitnet/docs/BITNET_BRIDGE.md",
        path.display()
    ))
}

/// 一次生成的结果（供遥测上报）
pub struct Generation {
    /// 生成文本（不含 prompt 与 EOS）
    pub text: String,
    /// prompt 编码后的 token 数
    pub prompt_tokens: usize,
    /// 实际生成的 token 数（≤ max_tokens，截断契约）
    pub gen_tokens: usize,
    /// 生成速度 tokens/s（单次 generate 端到端实测，含 prompt 预填充）
    pub tps: f32,
}

/// 本地采样器抽象：把 bitnet-rs 推理细节挡在 trait 后面，
/// 测试可注入假实现覆盖桥接逻辑（CI 无需真模型）
pub trait Sampler: Send + Sync {
    /// 生成 ≤ `max_tokens` 个 token 的回复
    fn generate(&self, prompt: &str, max_tokens: u32) -> Result<Generation, AppError>;
}

/// bitnet-rs 真实现：惰性加载一次，推理期间禁止重入
pub struct BitnetSampler {
    model_path: PathBuf,
    temperature: f32,
    state: Mutex<SamplerState>,
}

enum SamplerState {
    Idle,
    Loaded(Box<bitnet_rs::model::Model>),
}

impl BitnetSampler {
    /// 新建采样器（模型延迟到首次 generate 时加载）
    pub fn new(model_path: impl Into<PathBuf>, temperature: f32) -> Self {
        Self {
            model_path: model_path.into(),
            temperature,
            state: Mutex::new(SamplerState::Idle),
        }
    }

    /// 模型文件路径
    pub fn model_path(&self) -> &Path {
        &self.model_path
    }

    /// 模型是否已加载进内存
    pub fn is_loaded(&self) -> bool {
        let st = self.state.lock().expect("bitnet sampler lock");
        matches!(&*st, SamplerState::Loaded(_))
    }
}

impl Sampler for BitnetSampler {
    fn generate(&self, prompt: &str, max_tokens: u32) -> Result<Generation, AppError> {
        // Mutex 从"加载 or 取已加载模型"一直持有到生成结束：
        // 惰性加载只发生一次，且同一时刻只有一个推理在跑（禁止重入）
        let mut st = self.state.lock().expect("bitnet sampler lock");
        if matches!(&*st, SamplerState::Idle) {
            let model = bitnet_rs::model::Model::load(&self.model_path).map_err(|e| match e {
                bitnet_rs::model::ModelError::Io(io)
                    if io.kind() == std::io::ErrorKind::NotFound =>
                {
                    model_missing_error(&self.model_path)
                }
                other => AppError::Runtime(format!(
                    "BitNet 模型加载失败（{}）: {other}",
                    self.model_path.display()
                )),
            })?;
            *st = SamplerState::Loaded(Box::new(model));
        }
        let SamplerState::Loaded(model) = &*st else {
            return Err(AppError::Runtime("BitNet 模型内部状态异常".into()));
        };

        let t0 = Instant::now();
        let g = model
            .generate(prompt, max_tokens, self.temperature)
            .map_err(|e| AppError::Runtime(format!("BitNet 推理失败: {e}")))?;
        let secs = t0.elapsed().as_secs_f32().max(1e-6);
        Ok(Generation {
            text: g.text,
            prompt_tokens: g.prompt_tokens,
            gen_tokens: g.gen_tokens,
            tps: g.gen_tokens as f32 / secs,
        })
    }
}

/// BitNet 本地推理实现的宿主桥（`HostBridge` 的全离线后端）
///
/// - `sample` → 本地 BitNet 生成（MCP `sampling/createMessage` 的本地等价物）
/// - `roots`  → 模型文件所在目录（file:// uri），供 Agent 感知可读范围
pub struct BitNetBridge {
    sampler: Box<dyn Sampler>,
    model_path: PathBuf,
    /// 桥的元信息暴露一次即可（roots 用），避免每次拼接
    roots_cache: OnceLock<Vec<(String, String)>>,
}

impl BitNetBridge {
    /// 用真模型路径构造桥（内部为 [`BitnetSampler`]，温度取 [`DEFAULT_TEMPERATURE`]）
    pub fn new(model_path: impl Into<PathBuf>) -> Self {
        let path = model_path.into();
        Self {
            sampler: Box::new(BitnetSampler::new(path.clone(), DEFAULT_TEMPERATURE)),
            model_path: path,
            roots_cache: OnceLock::new(),
        }
    }

    /// 注入自定义采样器（测试 / 换后端）
    pub fn with_sampler(sampler: Box<dyn Sampler>) -> Self {
        Self {
            sampler,
            model_path: PathBuf::from("."),
            roots_cache: OnceLock::new(),
        }
    }

    /// 覆盖 roots 上报的模型路径
    pub fn with_model_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.model_path = path.into();
        self
    }

    /// 模型文件路径
    pub fn model_path(&self) -> &Path {
        &self.model_path
    }
}

impl HostBridge for BitNetBridge {
    fn sample(&self, prompt: &str, max_tokens: u32) -> Result<String, AppError> {
        Ok(self.sampler.generate(prompt, max_tokens)?.text)
    }

    fn roots(&self) -> Result<Vec<(String, String)>, AppError> {
        if let Some(r) = self.roots_cache.get() {
            return Ok(r.clone());
        }
        let dir = self
            .model_path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        // file:// uri：Windows 盘符路径补前导斜杠（file:///D:/...）
        let mut uri = dir.to_string_lossy().replace('\\', "/");
        if !uri.starts_with('/') {
            uri = format!("/{uri}");
        }
        let roots = vec![(format!("file://{uri}"), "bitnet-model".to_string())];
        let _ = self.roots_cache.set(roots.clone());
        Ok(roots)
    }
}

// ── 编译期保证 ────────────────────────────────────────────
// HostBridge 对象要求 Send + Sync；桥将被 Arc 后跨线程注入
const _: fn() = || {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<BitNetBridge>();
    assert_send_sync::<BitnetSampler>();
};
