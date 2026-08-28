use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;

use crate::progress::{LogLevel, Progress};

/// 传出给 App::run() 的运行时上下文
///
/// 负责进度上报、取消信号、输出格式控制。
/// Context 是 Send 的，支持多线程任务中跨线程传递。
pub struct Context {
    /// 发送进度事件的 channel sender
    progress_tx: Sender<Progress>,
    /// 取消信号（共享所有权，多线程可读）
    cancel: Arc<AtomicBool>,
    /// 输出格式：human / json / json-stream
    pub output_format: OutputFormat,
    /// 是否已发送终态事件（Done / Error）。
    /// 供 executor 判断是否需要在 handler 返回后合成终态，
    /// 从而保证事件流恒以 Done / Error 结尾（协议不变量）。
    terminal_sent: AtomicBool,
}

/// 输出格式
#[derive(Debug, Clone, PartialEq)]
pub enum OutputFormat {
    /// 人类友好，彩色，有格式
    Human,
    /// 单个 JSON 对象，任务完成后输出
    Json,
    /// 每个 Progress 事件一行 JSON，适合 AI / 脚本消费
    JsonStream,
}

impl Context {
    /// 构造一个新的 Context
    pub fn new(
        progress_tx: Sender<Progress>,
        cancel: Arc<AtomicBool>,
        output_format: OutputFormat,
    ) -> Self {
        Self {
            progress_tx,
            cancel,
            output_format,
            terminal_sent: AtomicBool::new(false),
        }
    }

    /// 测试用构造函数：不需要外部取消信号，默认 Human 输出
    pub fn new_test(tx: Sender<Progress>) -> Self {
        Self {
            progress_tx: tx,
            cancel: Arc::new(AtomicBool::new(false)),
            output_format: OutputFormat::Human,
            terminal_sent: AtomicBool::new(false),
        }
    }

    /// 正式构造：无外部取消信号（后台任务自行管理，如 [`crate::executor::spawn`]）
    pub fn from_sender(progress_tx: Sender<Progress>) -> Self {
        Self {
            progress_tx,
            cancel: Arc::new(AtomicBool::new(false)),
            output_format: OutputFormat::Human,
            terminal_sent: AtomicBool::new(false),
        }
    }

    /// 上报进度事件，三端各自处理显示
    pub fn emit(&self, progress: Progress) {
        let _ = self.progress_tx.send(progress);
    }

    /// 检查是否已被取消
    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    /// 上报 Tick 进度事件
    pub fn tick(&self, current: u64, total: Option<u64>, message: impl Into<String>) {
        let percent = total.map(|t| current as f32 / t as f32);
        self.emit(Progress::Tick {
            current,
            total,
            message: Some(message.into()),
            percent,
        });
    }

    /// 上报 Log 事件
    pub fn log(&self, level: LogLevel, message: impl Into<String>) {
        self.emit(Progress::Log {
            level,
            message: message.into(),
        });
    }

    /// 上报 Done 事件
    pub fn done(&self, result: serde_json::Value, duration_ms: u64) {
        self.emit(Progress::Done {
            result,
            duration_ms,
        });
        self.terminal_sent.store(true, Ordering::Relaxed);
    }

    /// 上报 Error 终态事件（协议保证事件流以 Done / Error 结尾）
    pub fn error(&self, code: i32, message: impl Into<String>) {
        self.emit(Progress::Error {
            code,
            message: message.into(),
            kind: None,
        });
        self.terminal_sent.store(true, Ordering::Relaxed);
    }

    /// 是否已发送终态事件。仅供 executor 在 handler 返回后判断是否需要合成终态。
    pub(crate) fn has_terminal(&self) -> bool {
        self.terminal_sent.load(Ordering::Relaxed)
    }
}

// Context: 手动 Debug（跳过 channel 和 atomic 的内部状态）
impl std::fmt::Debug for Context {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Context")
            .field("output_format", &self.output_format)
            .finish_non_exhaustive()
    }
}

// ── Send 保证 ─────────────────────────────────────────────
// Progress 不含 non-Send 类型，Sender 和 Arc 都是 Send，
// 因此 Context 自动为 Send。
// 编译期验证：
const _: fn() = || {
    fn assert_send<T: Send>() {}
    assert_send::<Context>();
};
