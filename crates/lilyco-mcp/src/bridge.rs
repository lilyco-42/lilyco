//! 宿主桥：把 `sampling/createMessage` / `roots/list` 变成 server → client 的反向请求。
//!
//! 反向请求由 serve 主循环把响应路由进 `PendingMap`，桥这边按 `srv-N` 字符串 id 等待；
//! 客户端没在 initialize 里声明对应能力就直接报错（人机协同：模型访问与审批控制权留在客户端）。

use std::collections::HashMap;
use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use lilyco_core::context::HostBridge;
use lilyco_core::AppError;

/// 客户端能力（initialize 时探测，门控反向请求）
#[derive(Default)]
pub(crate) struct Caps {
    pub(crate) sampling: AtomicBool,
    pub(crate) roots: AtomicBool,
}

/// 服务端反向请求的 pending 表：id → 响应回传 sender
pub(crate) type PendingMap =
    Arc<Mutex<HashMap<String, mpsc::Sender<Result<serde_json::Value, String>>>>>;

/// 反向请求的响应等待上限（工具内嵌 LLM 调用可能较慢）
pub(crate) const BRIDGE_TIMEOUT: Duration = Duration::from_secs(300);

// ── 宿主桥（MCP 反向能力：采样 / roots） ──────────────────

/// 把 `sampling/createMessage` / `roots/list` 映射为 server → client
/// JSON-RPC 请求的宿主桥。
///
/// 协议规定采样与 roots 是**客户端**能力：客户端须在 initialize 的
/// capabilities 中声明，未声明时对应方法返回带指引的错误（人机协同：
/// 客户端保留模型访问与审批控制权）。
pub(crate) struct McpBridge {
    pub(crate) writer: Arc<Mutex<dyn Write + Send>>,
    pub(crate) pending: PendingMap,
    pub(crate) caps: Arc<Caps>,
    pub(crate) next_id: AtomicU64,
}

impl McpBridge {
    /// 发出反向请求并等待客户端响应（响应由 serve 主循环路由进 pending 表）。
    /// server 侧请求 id 用 `srv-N` 字符串，与客户端的数字 id 天然不冲突。
    fn request(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, AppError> {
        let id = format!("srv-{}", self.next_id.fetch_add(1, Ordering::Relaxed));
        let (tx, rx) = mpsc::channel();
        self.pending.lock().unwrap().insert(id.clone(), tx);

        let req = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        {
            let mut w = self.writer.lock().unwrap();
            if writeln!(w, "{req}").and_then(|_| w.flush()).is_err() {
                self.pending
                    .lock()
                    .unwrap()
                    .remove(&format!("srv-{}", self.next_id.load(Ordering::Relaxed) - 1));
                return Err(AppError::Runtime(
                    "反向请求写出失败（客户端已断开？）".into(),
                ));
            }
        }

        match rx.recv_timeout(BRIDGE_TIMEOUT) {
            Ok(Ok(result)) => Ok(result),
            Ok(Err(msg)) => Err(AppError::Runtime(format!("客户端拒绝 {method}: {msg}"))),
            Err(_) => Err(AppError::Runtime(format!(
                "{method} 响应超时（{BRIDGE_TIMEOUT:?}）"
            ))),
        }
    }
}

impl HostBridge for McpBridge {
    fn sample(&self, prompt: &str, max_tokens: u32) -> Result<String, AppError> {
        if !self.caps.sampling.load(Ordering::Relaxed) {
            return Err(AppError::Runtime(
                "MCP 客户端未声明 sampling 能力（initialize capabilities）".into(),
            ));
        }
        // 2024-11-05 规范：messages[{role, content{type,text}}] + maxTokens 必填
        let result = self.request(
            "sampling/createMessage",
            serde_json::json!({
                "messages": [
                    { "role": "user", "content": { "type": "text", "text": prompt } }
                ],
                "maxTokens": max_tokens,
            }),
        )?;
        match result["content"]["text"].as_str() {
            Some(text) => Ok(text.to_string()),
            None => Err(AppError::Runtime("sampling 响应缺少 content.text".into())),
        }
    }

    fn roots(&self) -> Result<Vec<(String, String)>, AppError> {
        if !self.caps.roots.load(Ordering::Relaxed) {
            return Err(AppError::Runtime(
                "MCP 客户端未声明 roots 能力（initialize capabilities）".into(),
            ));
        }
        let result = self.request("roots/list", serde_json::json!({}))?;
        let mut out = Vec::new();
        if let Some(roots) = result["roots"].as_array() {
            for r in roots {
                out.push((
                    r["uri"].as_str().unwrap_or_default().to_string(),
                    r["name"].as_str().unwrap_or_default().to_string(),
                ));
            }
        }
        Ok(out)
    }
}
