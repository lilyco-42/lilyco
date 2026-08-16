use serde::{Deserialize, Serialize};

/// 长任务的进度事件，三端都消费这个类型
///
/// - CLI → JSON stream 到 stdout
/// - TUI → 进度条组件更新
/// - GUI → 进度环 + 取消按钮
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Progress {
    /// 任务开始
    Started {
        /// 总步数，None 表示不确定
        total: Option<u64>,
        /// 人类可读的描述
        message: Option<String>,
    },
    /// 进度推进一步
    Tick {
        /// 当前步数
        current: u64,
        /// 总步数
        total: Option<u64>,
        /// 人类可读的当前状态，如 "编码第 120 帧"
        message: Option<String>,
        /// 0.0 ~ 1.0，None 表示不确定
        percent: Option<f32>,
    },
    /// 日志输出
    Log { level: LogLevel, message: String },
    /// 任务完成，携带结果
    Done {
        /// 任务结果，序列化为 JSON
        result: serde_json::Value,
        /// 耗时（毫秒）
        duration_ms: u64,
    },
    /// 任务出错
    Error {
        /// 错误码
        code: i32,
        /// 人类可读的错误消息
        message: String,
        /// 机器可读的错误类型
        kind: Option<String>,
    },
}

/// 日志级别
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}
