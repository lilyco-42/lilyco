//! # Triforge Core
//!
//! 让 Rust 软件天生 AI-callable 的核心 crate。
//!
//! 提供核心 trait（`App`、`Renderer`）、Schema 描述类型（`CommandSchema`、
//! `ArgSchema`、`ArgKind`）、统一进度协议（`Progress`）以及运行时上下文
//! （`Context`）。
//!
//! ## 使用
//!
//! ```ignore
//! use triforge_core::prelude::*;
//! ```

// ── 模块声明 ──────────────────────────────────────────────

pub mod app;
pub mod context;
pub mod error;
pub mod progress;
pub mod schema;

// ── 便捷导入 ──────────────────────────────────────────────

// ── 测试 ──────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use crate::prelude::*;
    use crate::schema::{ArgKind, ArgSchema, CommandSchema};

    // ─── Progress 序列化 ───────────────────────────────

    #[test]
    fn progress_tick_serializes_to_json() {
        let p = Progress::Tick {
            current: 120,
            total: Some(1000),
            message: Some("编码第 120 帧".into()),
            percent: Some(0.12),
        };
        let json = serde_json::to_string(&p).unwrap();
        assert!(json.contains("\"type\":\"tick\""), "expected type=tick, got: {json}");
        assert!(json.contains("\"current\":120"), "expected current=120, got: {json}");
    }

    #[test]
    fn progress_roundtrip_all_variants() {
        let cases: Vec<Progress> = vec![
            Progress::Started {
                total: Some(100),
                message: Some("开始".into()),
            },
            Progress::Tick {
                current: 50,
                total: Some(100),
                message: Some("处理中".into()),
                percent: Some(0.5),
            },
            Progress::Log {
                level: LogLevel::Warn,
                message: "磁盘空间不足".into(),
            },
            Progress::Done {
                result: serde_json::json!({"ok": true}),
                duration_ms: 4200,
            },
            Progress::Error {
                code: 42,
                message: "文件不存在".into(),
                kind: Some("io_error".into()),
            },
            Progress::Started {
                total: None,
                message: None,
            },
            Progress::Tick {
                current: 0,
                total: None,
                message: None,
                percent: None,
            },
        ];

        for case in cases {
            let json = serde_json::to_string(&case).unwrap();
            let back: Progress = serde_json::from_str(&json).unwrap();
            let json2 = serde_json::to_string(&back).unwrap();
            assert_eq!(json, json2, "roundtrip failed for {json}");
        }
    }

    #[test]
    fn loglevel_serializes_snake_case() {
        assert_eq!(
            serde_json::to_string(&LogLevel::Info).unwrap(),
            "\"info\""
        );
        assert_eq!(
            serde_json::to_string(&LogLevel::Warn).unwrap(),
            "\"warn\""
        );
    }

    // ─── CommandSchema → tool 导出 ─────────────────────

    #[test]
    fn command_schema_exports_anthropic_tool() {
        let schema = CommandSchema {
            name: "transcode".into(),
            about: "转码视频".into(),
            args: vec![
                ArgSchema {
                    name: "input".into(),
                    about: "输入文件".into(),
                    kind: ArgKind::Path { must_exist: true },
                    required: true,
                    default: None,
                },
                ArgSchema {
                    name: "codec".into(),
                    about: "编码格式".into(),
                    kind: ArgKind::Enum {
                        values: vec!["h264".into(), "h265".into(), "av1".into()],
                    },
                    required: false,
                    default: Some(serde_json::json!("h264")),
                },
            ],
            subcommands: vec![],
        };

        let tool = schema.to_anthropic_tool();
        assert_eq!(tool["name"], "transcode");
        assert!(tool["input_schema"]["properties"]["input"].is_object());
        assert_eq!(
            tool["input_schema"]["required"][0],
            "input",
            "input should be required"
        );
    }

    #[test]
    fn command_schema_exports_openai_tool() {
        let schema = CommandSchema {
            name: "search".into(),
            about: "搜索文档".into(),
            args: vec![ArgSchema {
                name: "query".into(),
                about: "搜索关键词".into(),
                kind: ArgKind::Text,
                required: true,
                default: None,
            }],
            subcommands: vec![],
        };

        let tool = schema.to_openai_tool();
        assert_eq!(tool["type"], "function");
        assert_eq!(tool["function"]["name"], "search");
        assert_eq!(tool["function"]["parameters"]["type"], "object");
    }

    #[test]
    fn command_schema_no_required_args_omits_required_field() {
        let schema = CommandSchema {
            name: "list".into(),
            about: "列出所有项目".into(),
            args: vec![ArgSchema {
                name: "verbose".into(),
                about: "详细输出".into(),
                kind: ArgKind::Flag,
                required: false,
                default: None,
            }],
            subcommands: vec![],
        };

        let js = schema.to_json_schema();
        assert!(js.get("required").is_none(), "no required field expected");
        assert_eq!(js["properties"]["verbose"]["type"], "boolean");
    }

    #[test]
    fn json_schema_maps_all_arg_kinds() {
        let schema = CommandSchema {
            name: "demo".into(),
            about: "演示所有类型".into(),
            args: vec![
                ArgSchema {
                    name: "flag".into(),
                    about: "a flag".into(),
                    kind: ArgKind::Flag,
                    required: false,
                    default: None,
                },
                ArgSchema {
                    name: "text".into(),
                    about: "a text".into(),
                    kind: ArgKind::Text,
                    required: false,
                    default: None,
                },
                ArgSchema {
                    name: "num".into(),
                    about: "a number".into(),
                    kind: ArgKind::Number {
                        min: Some(0.0),
                        max: Some(100.0),
                    },
                    required: false,
                    default: None,
                },
                ArgSchema {
                    name: "mode".into(),
                    about: "an enum".into(),
                    kind: ArgKind::Enum {
                        values: vec!["a".into(), "b".into()],
                    },
                    required: false,
                    default: None,
                },
                ArgSchema {
                    name: "file".into(),
                    about: "a path".into(),
                    kind: ArgKind::Path { must_exist: false },
                    required: false,
                    default: None,
                },
                ArgSchema {
                    name: "tags".into(),
                    about: "a list".into(),
                    kind: ArgKind::List {
                        item: Box::new(ArgKind::Text),
                    },
                    required: false,
                    default: None,
                },
            ],
            subcommands: vec![],
        };

        let js = schema.to_json_schema();
        let props = &js["properties"];

        assert_eq!(props["flag"]["type"], "boolean");
        assert_eq!(props["text"]["type"], "string");
        assert_eq!(props["num"]["type"], "number");
        assert_eq!(props["num"]["minimum"], 0.0);
        assert_eq!(props["num"]["maximum"], 100.0);
        assert_eq!(props["mode"]["type"], "string");
        assert_eq!(props["mode"]["enum"].as_array().unwrap().len(), 2);
        assert_eq!(props["file"]["type"], "string");
        assert_eq!(props["tags"]["type"], "array");
        assert_eq!(props["tags"]["items"]["type"], "string");
    }

    // ─── Context ───────────────────────────────────────

    #[test]
    fn context_emits_progress() {
        let (tx, rx) = std::sync::mpsc::channel();
        let ctx = Context::new_test(tx);
        ctx.tick(1, Some(10), "step 1");
        let event = rx.recv().unwrap();
        assert!(
            matches!(event, Progress::Tick { current: 1, .. }),
            "expected Tick with current=1"
        );
    }

    #[test]
    fn context_log_and_done() {
        let (tx, rx) = std::sync::mpsc::channel();
        let ctx = Context::new_test(tx);

        ctx.log(LogLevel::Error, "something went wrong");
        let event = rx.recv().unwrap();
        assert!(matches!(event, Progress::Log { level: LogLevel::Error, .. }));

        ctx.done(serde_json::json!({"done": true}), 100);
        let event = rx.recv().unwrap();
        assert!(matches!(event, Progress::Done { .. }));
    }

    #[test]
    fn context_is_not_cancelled_by_default() {
        let (tx, _rx) = std::sync::mpsc::channel();
        let ctx = Context::new_test(tx);
        assert!(!ctx.is_cancelled());
    }

    #[test]
    fn context_is_cancelled_when_atomic_set() {
        let (tx, _rx) = std::sync::mpsc::channel();
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let ctx = Context::new(
            tx,
            cancel.clone(),
            OutputFormat::Human,
        );
        assert!(!ctx.is_cancelled());
        cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        assert!(ctx.is_cancelled());
    }

    #[test]
    fn context_tick_computes_percent() {
        let (tx, rx) = std::sync::mpsc::channel();
        let ctx = Context::new_test(tx);
        ctx.tick(5, Some(10), "halfway");
        let event = rx.recv().unwrap();
        if let Progress::Tick { percent, .. } = event {
            assert!(
                (percent.unwrap() - 0.5).abs() < f32::EPSILON,
                "expected ~0.5, got {:?}",
                percent
            );
        } else {
            panic!("expected Tick");
        }
    }

    // ─── AppError ──────────────────────────────────────

    #[test]
    fn app_error_display() {
        assert_eq!(
            AppError::InvalidArg("missing input".into()).to_string(),
            "参数错误: missing input"
        );
        assert_eq!(
            AppError::Cancelled.to_string(),
            "已取消"
        );
    }

    #[test]
    fn app_error_serde_roundtrip() {
        let cases = vec![
            AppError::InvalidArg("bad arg".into()),
            AppError::InvalidInput("bad input".into()),
            AppError::Runtime("boom".into()),
            AppError::Cancelled,
        ];

        for case in cases {
            let json = serde_json::to_string(&case).unwrap();
            let back: AppError = serde_json::from_str(&json).unwrap();
            assert_eq!(case.to_string(), back.to_string(), "roundtrip failed for {json}");
        }
    }

    #[test]
    fn app_error_from_io() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let app_err: AppError = io_err.into();
        assert!(app_err.to_string().contains("IO 错误"));
        assert!(app_err.to_string().contains("file not found"));
    }

    #[test]
    fn app_error_from_serde_json() {
        let serde_err: AppError =
            serde_json::from_str::<serde_json::Value>("not json").unwrap_err().into();
        assert!(serde_err.to_string().contains("序列化错误"));
    }

    // ─── Schema serde ─────────────────────────────────

    #[test]
    fn arg_schema_roundtrip() {
        let arg = ArgSchema {
            name: "count".into(),
            about: "数量".into(),
            kind: ArgKind::Number {
                min: Some(1.0),
                max: None,
            },
            required: true,
            default: Some(serde_json::json!(10)),
        };
        let val = serde_json::to_value(&arg).unwrap();
        let back: ArgSchema = serde_json::from_value(val).unwrap();
        assert_eq!(back.name, "count");
        assert_eq!(back.required, true);
    }

    #[test]
    fn command_schema_roundtrip() {
        let schema = CommandSchema {
            name: "build".into(),
            about: "构建项目".into(),
            args: vec![ArgSchema {
                name: "release".into(),
                about: "release 模式".into(),
                kind: ArgKind::Flag,
                required: false,
                default: None,
            }],
            subcommands: vec![CommandSchema {
                name: "clean".into(),
                about: "清理构建产物".into(),
                args: vec![],
                subcommands: vec![],
            }],
        };

        let val = serde_json::to_value(&schema).unwrap();
        let back: CommandSchema = serde_json::from_value(val).unwrap();
        assert_eq!(back.name, "build");
        assert_eq!(back.subcommands.len(), 1);
        assert_eq!(back.subcommands[0].name, "clean");
    }

    // ─── 编译期保证 ──────────────────────────────────

    #[test]
    fn context_is_send() {
        // 如果 Context 不是 Send，这行编译不过
        fn assert_send<T: Send>(v: T) -> T { v }
        let (tx, _rx) = std::sync::mpsc::channel();
        let ctx = Context::new_test(tx);
        let _ = assert_send(ctx);
    }
}

/// Triforge 的 prelude：一次导入所有常用类型和 trait
pub mod prelude {
    pub use crate::app::{App, Renderer};
    pub use crate::context::{Context, OutputFormat};
    pub use crate::error::AppError;
    pub use crate::progress::{LogLevel, Progress};
    pub use crate::schema::{ArgKind, ArgSchema, CommandSchema, ValueEnum};
}
