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
    /// 遥测数据点：物理世界的持续状态流（无人机姿态 / PLC 寄存器 / 传感器读数）
    ///
    /// 与 Tick（任务进度）不同，Telemetry 描述**被控对象**的实时状态，
    /// 由 handler 在执行期间持续上报；MCP 侧复用已协商的 progress 通道
    /// 转发为 `key=value` 消息，Agent 可实时感知物理世界。
    Telemetry {
        /// 数据点名称，如 "altitude"、"battery"、"rpm"
        key: String,
        /// 数据点值（结构化 JSON，可为数字/字符串/对象）
        value: serde_json::Value,
    },
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
