//! MCP 协议面的测试：握手、工具表、调用与校验拒绝、进度通知、双向 serve。
//!
//! 文件头等价于拆分前的 `use super::*`（lib.rs 的全部导入）+ 三个模块的内部项。

use std::collections::HashMap;
use std::io::{BufReader, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use lilyco_core::registry::{RegisteredCommand, Registry};
use lilyco_core::schema::{ArgKind, ArgSchema, CommandSchema};
use lilyco_core::{App, AppError, Context};

use crate::protocol::*;
use crate::server::McpServer;

/// 一个可执行的最小 App：echo
struct Echo;

impl App for Echo {
    fn schema() -> CommandSchema {
        CommandSchema {
            name: "echo".into(),
            about: "echo the given text".into(),
            args: vec![ArgSchema {
                name: "text".into(),
                about: "text to echo".into(),
                kind: ArgKind::Text,
                required: true,
                default: None,
            }],
            subcommands: vec![],
            safety: lilyco_core::safety::SafetyTier::ReadOnly,
        }
    }

    fn from_args(args: &HashMap<String, serde_json::Value>) -> Result<Self, AppError> {
        if !args.contains_key("text") {
            return Err(AppError::InvalidArg("missing text".into()));
        }
        Ok(Echo)
    }

    fn run(&self, ctx: &Context) -> Result<serde_json::Value, AppError> {
        let r = serde_json::json!({ "echoed": true });
        ctx.done(r.clone(), 0);
        Ok(r)
    }
}

fn test_registry() -> Registry {
    let mut reg = Registry::new();
    reg.register(RegisteredCommand::from_app::<Echo>()).unwrap();
    reg
}

#[test]
fn initialize_returns_protocol_version() {
    let server = McpServer::new(test_registry());
    let resp = server
        .handle_line(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#)
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["id"], 1);
    assert_eq!(v["result"]["protocolVersion"], PROTOCOL_VERSION);
    assert_eq!(v["result"]["capabilities"]["tools"], serde_json::json!({}));
}

#[test]
fn tools_list_lists_visible_commands() {
    let server = McpServer::new(test_registry());
    let resp = server
        .handle_line(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#)
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    let tools = v["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0]["name"], "echo");
    assert_eq!(tools[0]["inputSchema"]["type"], "object");
}

#[test]
fn tools_call_executes_handler() {
    let server = McpServer::new(test_registry());
    let req = r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"echo","arguments":{"text":"hi"}}}"#;
    let resp = server.handle_line(req).unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["result"]["isError"], false);
    assert!(v["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("echoed"));
}

#[test]
fn tools_call_unknown_tool_is_error() {
    let server = McpServer::new(test_registry());
    let req = r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"nope"}}"#;
    let resp = server.handle_line(req).unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["error"]["code"], ERROR_INVALID_PARAMS);
}

#[test]
fn notification_gets_no_response() {
    let server = McpServer::new(test_registry());
    assert!(server
        .handle_line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)
        .is_none());
}

#[test]
fn unknown_notification_gets_no_response() {
    // JSON-RPC：通知（无 id）绝不响应——DSH 客户端会发 notifications/cancelled
    let server = McpServer::new(test_registry());
    assert!(server
        .handle_line(r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{}}"#)
        .is_none());
}

#[test]
fn unknown_method_returns_error() {
    let server = McpServer::new(test_registry());
    let resp = server
        .handle_line(r#"{"jsonrpc":"2.0","id":5,"method":"bogus"}"#)
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["error"]["code"], ERROR_METHOD_NOT_FOUND);
}

#[test]
fn parse_error_returns_error() {
    let server = McpServer::new(test_registry());
    let resp = server.handle_line("not json at all").unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["error"]["code"], ERROR_PARSE);
}

#[test]
fn serve_roundtrips_over_memory_io() {
    let server = McpServer::new(test_registry());
    let input = concat!(
        "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\n",
        "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\"}\n",
    );
    let out = SharedBuf::default();
    server
        .serve(std::io::Cursor::new(input), out.clone())
        .unwrap();
    let s = out.string();
    assert!(s.contains("\"result\":{}"), "ping response: {s}");
    assert!(s.contains("\"tools\""), "tools/list response: {s}");
    // 两行请求 → 两行响应
    assert_eq!(s.lines().count(), 2);
}

#[test]
fn serve_skips_blank_lines() {
    let server = McpServer::new(test_registry());
    let input = "\n\n{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\n\n";
    let out = SharedBuf::default();
    server
        .serve(std::io::Cursor::new(input), out.clone())
        .unwrap();
    assert_eq!(out.string().lines().count(), 1);
}

/// **EOF 后在途的 `tools/call` 响应不能丢**。
///
/// `tools/call` 被丢到 worker 线程执行（为了支持反向 sampling/roots）。
/// 曾经的缺陷：主循环读到 EOF 直接 break，worker 还没写完就被进程退出带走，
/// 短连接客户端（脚本 / CI / `echo … | binary --mcp`）全部拿不到结果。
/// 修法：EOF 后 join 所有 worker 再返回。
///
/// 这里的输入是 `Cursor`，读完即 EOF —— 正好模拟"写完就关 stdin"。
#[test]
fn serve_waits_for_inflight_tools_call_on_eof() {
    let server = McpServer::new(test_registry());
    let input = concat!(
        "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{}}\n",
        "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",",
        "\"params\":{\"name\":\"echo\",\"arguments\":{\"text\":\"hi\"}}}\n",
    );
    let out = SharedBuf::default();
    server
        .serve(std::io::Cursor::new(input), out.clone())
        .unwrap();

    let s = out.string();
    // 必须拿到 id=2 的响应体，而不是只有 initialize
    let call_line = s
        .lines()
        .find(|l| l.contains("\"id\":2"))
        .unwrap_or_else(|| panic!("tools/call 响应丢失（EOF 杀死了 worker）: {s}"));
    assert!(
        call_line.contains("\"isError\":false"),
        "echo 应成功返回: {call_line}"
    );
    assert!(
        call_line.contains("echoed"),
        "tools/call 响应体不完整: {call_line}"
    );
}

// ─── 进度通知（notifications/progress） ────────────

use lilyco_core::registry::Handler;

/// 携带自定义 handler 的注册表：执行时上报两次 Tick + 一次 Done
fn progress_registry() -> Registry {
    let mut reg = Registry::new();
    let handler: Handler = Arc::new(|ctx, _args| {
        ctx.tick(1, Some(2), "step 1");
        ctx.tick(2, Some(2), "step 2");
        let r = serde_json::json!({ "ok": true });
        ctx.done(r.clone(), 3);
        Ok(r)
    });
    let schema = CommandSchema {
        name: "progress".into(),
        about: "progress test".into(),
        args: vec![],
        subcommands: vec![],
        safety: lilyco_core::safety::SafetyTier::ReadOnly,
    };
    reg.register(RegisteredCommand::new("progress", schema).with_handler(handler))
        .unwrap();
    reg
}

fn progress_call_req(token_json: &str) -> String {
    format!(
        r#"{{"jsonrpc":"2.0","id":9,"method":"tools/call","params":{{"_meta":{{"progressToken":{token_json}}},"name":"progress","arguments":{{}}}}}}"#
    )
}

#[test]
fn progress_token_emits_notifications() {
    let server = McpServer::new(progress_registry());
    let mut notifications = Vec::new();
    let resp = server
        .handle_line_with_sink(&progress_call_req("\"tok-1\""), &mut |n| {
            notifications.push(n.to_string())
        })
        .unwrap();

    // 两次 Tick → 两条通知，内容与进度一一对应
    assert_eq!(notifications.len(), 2, "notifications: {notifications:?}");
    let first: serde_json::Value = serde_json::from_str(&notifications[0]).unwrap();
    assert_eq!(first["jsonrpc"], "2.0");
    assert_eq!(first["method"], "notifications/progress");
    assert_eq!(first["params"]["progressToken"], "tok-1");
    assert_eq!(first["params"]["progress"], 1.0);
    assert_eq!(first["params"]["total"], 2);
    assert_eq!(first["params"]["message"], "step 1");
    let second: serde_json::Value = serde_json::from_str(&notifications[1]).unwrap();
    assert_eq!(second["params"]["progress"], 2.0);
    assert_eq!(second["params"]["message"], "step 2");

    // 最终响应仍是合法 tools/call 结果
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["id"], 9);
    assert_eq!(v["result"]["isError"], false);
    assert!(v["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("ok"));
}

#[test]
fn progress_token_can_be_number() {
    let server = McpServer::new(progress_registry());
    let mut notifications = Vec::new();
    server
        .handle_line_with_sink(&progress_call_req("42"), &mut |n| {
            notifications.push(n.to_string())
        })
        .unwrap();
    let first: serde_json::Value = serde_json::from_str(&notifications[0]).unwrap();
    assert_eq!(first["params"]["progressToken"], 42);
}

#[test]
fn no_progress_token_emits_no_notifications() {
    let server = McpServer::new(progress_registry());
    let mut notifications = Vec::new();
    let resp = server
        .handle_line_with_sink(
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"progress","arguments":{}}}"#,
            &mut |n| notifications.push(n.to_string()),
        )
        .unwrap();
    assert!(notifications.is_empty(), "no token → no notifications");
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["result"]["isError"], false);
}

#[test]
fn plain_handle_line_never_notifies() {
    // 兼容入口：handle_line 等价于空 sink
    let server = McpServer::new(progress_registry());
    let resp = server.handle_line(&progress_call_req("\"t\"")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["result"]["isError"], false);
}

#[test]
fn telemetry_forwards_over_progress_channel() {
    // P1 遥测：Telemetry 数据点在 progressToken 已协商时转成 key=value 消息，
    // Agent 在 tools/call 进行中即可看到物理世界状态（无人机姿态流）
    let mut reg = Registry::new();
    let handler: Handler = Arc::new(|ctx, _args| {
        ctx.telemetry("altitude", serde_json::json!(120.5));
        ctx.telemetry("battery", serde_json::json!(87));
        let r = serde_json::json!({ "landed": true });
        ctx.done(r.clone(), 1);
        Ok(r)
    });
    let schema = CommandSchema {
        name: "telemetry".into(),
        about: "emit telemetry".into(),
        args: vec![],
        subcommands: vec![],
        safety: lilyco_core::safety::SafetyTier::ReadOnly,
    };
    reg.register(RegisteredCommand::new("telemetry", schema).with_handler(handler))
        .unwrap();
    let server = McpServer::new(reg);

    let mut notifications: Vec<String> = Vec::new();
    let resp = server
        .handle_line_with_sink(
            r#"{"jsonrpc":"2.0","id":30,"method":"tools/call","params":{"_meta":{"progressToken":"tok-t"},"name":"telemetry","arguments":{}}}"#,
            &mut |n| notifications.push(n.to_string()),
        )
        .unwrap();
    let messages: Vec<String> = notifications
        .iter()
        .filter_map(|s| {
            let v: serde_json::Value = serde_json::from_str(s).ok()?;
            if v["method"] == "notifications/progress" {
                v["params"]["message"].as_str().map(String::from)
            } else {
                None
            }
        })
        .collect();
    assert!(
        messages.iter().any(|m| m.contains("altitude=120.5")),
        "{messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("battery=87")),
        "{messages:?}"
    );
    // 最终响应仍是合法结果
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert!(v["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("landed"));
}

#[test]
fn error_tool_with_progress_still_returns_error_response() {
    let mut reg = Registry::new();
    let handler: Handler = Arc::new(|ctx, _args| {
        ctx.tick(1, None, "working");
        Err(AppError::Runtime("boom".into()))
    });
    let schema = CommandSchema {
        name: "fail".into(),
        about: "always fails".into(),
        args: vec![],
        subcommands: vec![],
        safety: lilyco_core::safety::SafetyTier::ReadOnly,
    };
    reg.register(RegisteredCommand::new("fail", schema).with_handler(handler))
        .unwrap();

    let server = McpServer::new(reg);
    let mut notifications = Vec::new();
    let resp = server
        .handle_line_with_sink(
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"_meta":{"progressToken":"t"},"name":"fail","arguments":{}}}"#,
            &mut |n| notifications.push(n.to_string()),
        )
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["result"]["isError"], true);
    assert!(v["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("boom"));
}

#[test]
fn serve_streams_notifications_before_response() {
    let server = McpServer::new(progress_registry());
    let input = format!("{}\n", progress_call_req("\"t\""));
    let out = SharedBuf::default();
    server
        .serve(std::io::Cursor::new(input), out.clone())
        .unwrap();
    // tools/call 在 worker 线程执行，serve 读到 EOF 即返回 —— 轮询等待全部输出
    out.wait_for("notifications/progress", Duration::from_secs(5));
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let stdout = loop {
        let s = out.string();
        if s.lines().count() >= 3 {
            break s;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "expected 3 output lines, got: {s}"
        );
        std::thread::sleep(Duration::from_millis(10));
    };
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 3, "2 notifications + 1 response: {lines:?}");

    let first: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(first["method"], "notifications/progress");
    let second: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
    assert_eq!(second["method"], "notifications/progress");
    let last: serde_json::Value = serde_json::from_str(lines[2]).unwrap();
    assert_eq!(last["result"]["isError"], false);
}

#[test]
fn tools_call_missing_required_arg_is_invalid_params() {
    let server = McpServer::new(test_registry());
    let req = r#"{"jsonrpc":"2.0","id":10,"method":"tools/call","params":{"name":"echo","arguments":{}}}"#;
    let resp = server.handle_line(req).unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["error"]["code"], ERROR_INVALID_PARAMS);
    assert!(
        v["error"]["message"]
            .as_str()
            .unwrap()
            .contains("缺少必填参数"),
        "schema validation error expected: {v}"
    );
}

#[test]
fn tools_call_wrong_arg_type_is_invalid_params() {
    let server = McpServer::new(test_registry());
    let req = r#"{"jsonrpc":"2.0","id":11,"method":"tools/call","params":{"name":"echo","arguments":{"text":42}}}"#;
    let resp = server.handle_line(req).unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["error"]["code"], ERROR_INVALID_PARAMS);
}

#[test]
fn tools_call_validation_rejects_before_execution() {
    // 携带 progressToken 的请求同样先过校验，不产生任何通知
    let server = McpServer::new(progress_registry());
    let mut notifications = Vec::new();
    let req = r#"{"jsonrpc":"2.0","id":12,"method":"tools/call","params":{"_meta":{"progressToken":"t"},"name":"nonexistent","arguments":{}}}"#;
    let resp = server
        .handle_line_with_sink(req, &mut |n| notifications.push(n.to_string()))
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["error"]["code"], ERROR_INVALID_PARAMS);
    assert!(notifications.is_empty());
}

#[test]
fn progress_notification_omits_none_fields() {
    let line = progress_notification(&serde_json::json!("t"), 3.0, None, None);
    let v: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert!(v["params"].get("total").is_none());
    assert!(v["params"].get("message").is_none());
    assert_eq!(v["params"]["progress"], 3.0);
}

// ─── serve 双向测试基建（共享输出 + 按需输入管道） ──

use std::sync::Condvar;

/// 线程共享输出：serve 的 worker 线程与主循环并发写
#[derive(Default, Clone)]
struct SharedBuf(Arc<Mutex<Vec<u8>>>);

impl SharedBuf {
    fn string(&self) -> String {
        String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
    }
    /// 轮询直到输出包含 needle（worker 线程异步写，不可即时断言）
    fn wait_for(&self, needle: &str, timeout: Duration) -> String {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            let s = self.string();
            if s.contains(needle) {
                return s;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "timeout waiting for {needle:?}; got: {s}"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Write for SharedBuf {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// 内存管道：客户端线程按需写入，server 端 Read 阻塞等待。
/// 用于确定性 e2e —— 客户端必须**看到**服务端的反向请求后才能回包
/// （预置输入会让响应先于 worker 注册 pending 到达，真实 stdio 无此序）。
#[derive(Default)]
struct Pipe(Mutex<Vec<u8>>, Condvar, AtomicBool);

impl Pipe {
    fn push(&self, line: &str) {
        let mut g = self.0.lock().unwrap();
        g.extend_from_slice(line.as_bytes());
        self.1.notify_all();
    }
    fn close(&self) {
        self.2.store(true, Ordering::Relaxed);
        self.1.notify_all();
    }
}

impl Pipe {
    /// 阻塞读取（内部可变性：Arc<Pipe> 共享下也可调用）
    fn read_inner(&self, out: &mut [u8]) -> std::io::Result<usize> {
        let mut g = self.0.lock().unwrap();
        loop {
            if !g.is_empty() {
                let n = out.len().min(g.len());
                out[..n].copy_from_slice(&g[..n]);
                g.drain(..n);
                return Ok(n);
            }
            if self.2.load(Ordering::Relaxed) {
                return Ok(0); // EOF
            }
            g = self.1.wait(g).unwrap();
        }
    }
}

impl std::io::Read for Pipe {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        self.read_inner(out)
    }
}

/// `Arc<Pipe>` 的 Read 句柄（serve 需要 BufRead —— 由外层 BufReader 提供）
struct PipeHandle(Arc<Pipe>);

impl std::io::Read for PipeHandle {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        self.0.read_inner(out)
    }
}

/// 执行中反向采样 / 取 roots 的测试命令
fn bridge_registry(sample: bool) -> Registry {
    let mut reg = Registry::new();
    let handler: Handler = Arc::new(move |ctx, _args| {
        let r = if sample {
            let reply = ctx.sample("给我一个字：好", 64)?;
            serde_json::json!({ "sampled": reply })
        } else {
            let roots = ctx.roots()?;
            serde_json::json!({
                "roots": roots.iter().map(|(uri, name)| serde_json::json!({"uri": uri, "name": name})).collect::<Vec<_>>()
            })
        };
        ctx.done(r.clone(), 0);
        Ok(r)
    });
    let schema = CommandSchema {
        name: "needs-host".into(),
        about: "needs host bridge".into(),
        args: vec![],
        subcommands: vec![],
        safety: lilyco_core::safety::SafetyTier::ReadOnly,
    };
    reg.register(RegisteredCommand::new("needs-host", schema).with_handler(handler))
        .unwrap();
    reg
}

#[test]
fn sample_without_host_bridge_is_guided_error() {
    // handle_line 路径无宿主桥（CLI/直调场景）→ 错误消息带指引
    let server = McpServer::new(bridge_registry(true));
    let resp = server
        .handle_line(r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"needs-host","arguments":{}}}"#)
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["result"]["isError"], true);
    let msg = v["result"]["content"][0]["text"].as_str().unwrap();
    assert!(msg.contains("不支持 LLM 采样"), "{msg}");
}

#[test]
fn serve_e2e_sampling_roundtrip() {
    let server = Arc::new(McpServer::new(bridge_registry(true)));
    let out = SharedBuf::default();
    let pipe = Arc::new(Pipe::default());
    let (done_tx, done_rx) = mpsc::channel();

    // 服务端线程：EOF（pipe.close）后退出
    {
        let server = Arc::clone(&server);
        let pipe = Arc::clone(&pipe);
        let out = out.clone();
        std::thread::spawn(move || {
            let _ = server.serve(BufReader::new(PipeHandle(Arc::clone(&pipe))), out);
            let _ = done_tx.send(());
        });
    }

    // 客户端线程：initialize（声明 sampling）→ tools/call
    // →（看到服务端 sampling/createMessage 后）回包 → 等结果 → 关闭
    let client = {
        let pipe = Arc::clone(&pipe);
        let out = out.clone();
        std::thread::spawn(move || {
            pipe.push(concat!(
                r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{"sampling":{}}}}"#,
                "\n",
            ));
            pipe.push(concat!(
                r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"needs-host","arguments":{}}}"#,
                "\n",
            ));
            out.wait_for("sampling/createMessage", Duration::from_secs(5));
            pipe.push(concat!(
                r#"{"jsonrpc":"2.0","id":"srv-1","result":{"role":"assistant","content":{"type":"text","text":"好"}}}"#,
                "\n",
            ));
            out.wait_for("sampled", Duration::from_secs(5));
            pipe.close();
        })
    };

    let final_out = out.wait_for("sampled", Duration::from_secs(5));
    assert!(
        final_out.contains("好"),
        "sampled text should round-trip: {final_out}"
    );
    client.join().unwrap();
    done_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("serve exits after EOF");
}

#[test]
fn serve_e2e_sampling_requires_client_capability() {
    let server = Arc::new(McpServer::new(bridge_registry(true)));
    let out = SharedBuf::default();
    let pipe = Arc::new(Pipe::default());

    {
        let server = Arc::clone(&server);
        let pipe = Arc::clone(&pipe);
        let out = out.clone();
        std::thread::spawn(move || {
            let _ = server.serve(BufReader::new(PipeHandle(Arc::clone(&pipe))), out);
        });
    }

    // initialize 不声明 sampling → tools/call 内采样报带指引的错误
    pipe.push(concat!(
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}"#,
        "\n",
    ));
    pipe.push(concat!(
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"needs-host","arguments":{}}}"#,
        "\n",
    ));
    let s = out.wait_for("isError", Duration::from_secs(5));
    assert!(
        s.contains("未声明 sampling"),
        "capability gate message: {s}"
    );
    pipe.close();
}

#[test]
fn serve_e2e_roots_roundtrip() {
    let server = Arc::new(McpServer::new(bridge_registry(false)));
    let out = SharedBuf::default();
    let pipe = Arc::new(Pipe::default());

    {
        let server = Arc::clone(&server);
        let pipe = Arc::clone(&pipe);
        let out = out.clone();
        std::thread::spawn(move || {
            let _ = server.serve(BufReader::new(PipeHandle(Arc::clone(&pipe))), out);
        });
    }

    let client = {
        let pipe = Arc::clone(&pipe);
        let out = out.clone();
        std::thread::spawn(move || {
            pipe.push(concat!(
                r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{"roots":{}}}}"#,
                "\n",
            ));
            pipe.push(concat!(
                r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"needs-host","arguments":{}}}"#,
                "\n",
            ));
            out.wait_for("roots/list", Duration::from_secs(5));
            pipe.push(concat!(
                r#"{"jsonrpc":"2.0","id":"srv-1","result":{"roots":[{"uri":"file:///workspace","name":"w"}]}}"#,
                "\n",
            ));
            out.wait_for("file:///workspace", Duration::from_secs(5));
            pipe.close();
        })
    };

    // 等 roots 的回程值（"roots" 会先匹配到服务端自己的 roots/list 请求行，不能用）
    let final_out = out.wait_for("file:///workspace", Duration::from_secs(5));
    assert!(
        final_out.contains("file:///workspace"),
        "roots should round-trip: {final_out}"
    );
    client.join().unwrap();
}
