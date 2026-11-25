//! `lchatgpt show-conversation` —— 把一条会话的活动分支摊平成可读的对话记录。
//!
//! ChatGPT 的会话是一张图（`mapping`）而不是一条线：一条消息的 `children` 里可能有
//! 编辑重答、工具调用、思考摘要等多个分支。`current_node` 指向用户最后看到的那条，
//! 从它沿 `parent` 一路回溯到根，才是屏幕上真实出现过的那条时间线。

use crate::api::Api;
use crate::auth;
use crate::util;
use lilyco::prelude::*;
use serde_json::{json, Value};
use std::collections::HashSet;

/// 省略时返回多少条消息
const DEFAULT_MESSAGES: u64 = 20;
/// 防环：真实会话最多几千个节点，超过这个数一定是数据坏了
const MAX_DEPTH: usize = 20_000;

/// 读一条会话
#[derive(App)]
#[app(
    name = "show-conversation",
    run = "run_show_conversation",
    about = "Render one conversation as a readable transcript: calls GET /backend-api/conversation/{id}, walks the active branch from current_node upwards through parent links, keeps only user/assistant turns addressed to the user (tool calls and hidden context are dropped) and returns { conversation_id, title, create_time, update_time, model_slug, node_count, message_count, returned, truncated, messages } where each message carries role, kind (text or code), time and text. Read-only (safety T0); nothing is modified."
)]
pub struct ShowConversation {
    #[arg(about = "Conversation id, as printed by the list command")]
    conversation_id: String,
    #[arg(
        about = "Return the last N messages (0 means the default 20)",
        default = 20
    )]
    messages: u64,
    #[arg(
        about = "Return every message on the active branch, ignoring the messages limit",
        default = false
    )]
    all: bool,
    #[arg(
        about = "Also include thoughts / reasoning_recap nodes",
        default = false
    )]
    include_reasoning: bool,
}

/// 从 `current_node` 回溯出的节点链（根在前）。
///
/// `ask` 的 chat 路径（`chat.rs`）也要用它判断「最后一条 assistant 说完了没有」，
/// 所以对 crate 内可见 —— 会话图的走法只能有一份实现。
pub(crate) fn active_branch(conversation: &Value) -> Vec<Value> {
    let mapping = match conversation.get("mapping").and_then(Value::as_object) {
        Some(m) => m,
        None => return Vec::new(),
    };
    let mut chain = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut cursor = conversation
        .get("current_node")
        .and_then(Value::as_str)
        .map(str::to_string);
    while let Some(id) = cursor {
        if !seen.insert(id.clone()) || seen.len() > MAX_DEPTH {
            break;
        }
        let node = match mapping.get(&id) {
            Some(n) => n,
            None => break,
        };
        chain.push(node.clone());
        cursor = node
            .get("parent")
            .and_then(Value::as_str)
            .map(str::to_string);
    }
    chain.reverse();
    chain
}

/// 一条节点消息里的可见文本；返回 (kind, text)，不可见的节点返回 None。
/// 同样给 `chat.rs` 复用：判断「答完了」时取的正文必须和 §show-conversation§ 一模一样。
pub(crate) fn visible_text(msg: &Value, include_reasoning: bool) -> Option<(String, String)> {
    // 发给工具的节点（网页搜索、代码执行）里装的是调用参数，不是给人看的话
    let recipient = msg
        .get("recipient")
        .and_then(Value::as_str)
        .unwrap_or("all");
    if recipient != "all" {
        return None;
    }
    let content = msg.get("content")?;
    let ctype = content
        .get("content_type")
        .and_then(Value::as_str)
        .unwrap_or("");
    match ctype {
        "text" | "multimodal_text" => {
            let mut out = String::new();
            for part in content.get("parts").and_then(Value::as_array)? {
                match part {
                    Value::String(s) => out.push_str(s),
                    Value::Object(o) => {
                        if let Some(t) = o.get("text").and_then(Value::as_str) {
                            out.push_str(t);
                        }
                    }
                    _ => {}
                }
            }
            Some(("text".to_string(), out))
        }
        "code" => content
            .get("text")
            .and_then(Value::as_str)
            .map(|t| ("code".to_string(), t.to_string())),
        "thoughts" if include_reasoning => {
            let mut out = String::new();
            for t in content
                .get("thoughts")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                if let Some(s) = t.get("summary").and_then(Value::as_str) {
                    out.push_str(s);
                    out.push('\n');
                }
            }
            Some(("thoughts".to_string(), out.trim_end().to_string()))
        }
        "reasoning_recap" if include_reasoning => content
            .get("content")
            .and_then(Value::as_str)
            .map(|t| ("reasoning_recap".to_string(), t.to_string())),
        _ => None,
    }
}

fn run_show_conversation(app: &ShowConversation, ctx: &Context) -> Result<Value, AppError> {
    let start = std::time::Instant::now();
    let id = app.conversation_id.trim();
    if id.is_empty() {
        return Err(AppError::InvalidArg(
            "conversation_id 不能为空；用 `lchatgpt list` 取一个".to_string(),
        ));
    }
    let api = Api::new(auth::load()?);
    ctx.emit(Progress::Started {
        total: Some(2),
        message: Some(format!("读会话 {id}")),
    });
    let conv = api.get_json(&format!("/conversation/{id}"), &[])?;
    let chain = active_branch(&conv);
    let node_count = chain.len();
    ctx.tick(1, Some(2), "摊平活动分支");

    let mut all: Vec<Value> = Vec::new();
    for node in &chain {
        let msg = match node.get("message") {
            Some(m) => m,
            None => continue,
        };
        let role = msg
            .get("author")
            .and_then(|a| a.get("role"))
            .and_then(Value::as_str)
            .unwrap_or("");
        if role != "user" && role != "assistant" {
            continue;
        }
        if let Some((kind, text)) = visible_text(msg, app.include_reasoning) {
            if text.trim().is_empty() {
                continue;
            }
            let ts = msg.get("create_time").unwrap_or(&Value::Null);
            all.push(json!({
                "role": role,
                "kind": kind,
                "time": util::iso8601_any(ts),
                "text": text,
            }));
        }
    }
    let message_count = all.len();
    let want = if app.all {
        message_count
    } else {
        let n = if app.messages == 0 {
            DEFAULT_MESSAGES
        } else {
            app.messages
        };
        (n as usize).min(message_count)
    };
    let truncated = want < message_count;
    let messages: Vec<Value> = all.split_off(message_count - want);
    let result = json!({
        "conversation_id": conv.get("conversation_id").cloned().unwrap_or(json!(id)),
        "title": conv.get("title").cloned().unwrap_or(Value::Null),
        "create_time": util::iso8601_any(conv.get("create_time").unwrap_or(&Value::Null)),
        "update_time": util::iso8601_any(conv.get("update_time").unwrap_or(&Value::Null)),
        "model_slug": conv.get("default_model_slug").cloned().unwrap_or(Value::Null),
        "node_count": node_count,
        "message_count": message_count,
        "returned": messages.len(),
        "truncated": truncated,
        "messages": messages,
    });

    // 人读面：直接打成人话，末了再提示怎么拿全文
    let title = result["title"].as_str().unwrap_or("(无标题)");
    ctx.log(LogLevel::Info, format!("《{title}》"));
    for m in result["messages"].as_array().into_iter().flatten() {
        let who = match m["role"].as_str().unwrap_or("") {
            "user" => "我",
            other => other,
        };
        let kind = m["kind"].as_str().unwrap_or("text");
        let tag = if kind == "text" {
            String::new()
        } else {
            format!(" <{kind}>")
        };
        ctx.log(
            LogLevel::Info,
            format!("\n[{who}]{tag} {}", m["text"].as_str().unwrap_or("")),
        );
    }
    if truncated {
        ctx.log(
            LogLevel::Warn,
            format!(
                "只显示最后 {} / {} 条消息；加 --all 看完整会话",
                result["returned"], message_count
            ),
        );
    }
    ctx.tick(2, Some(2), "");
    ctx.done(result.clone(), start.elapsed().as_millis() as u64);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: &str, parent: Option<&str>, role: &str, text: &str) -> Value {
        json!({
            "id": id,
            "parent": parent,
            "message": {
                "author": {"role": role},
                "recipient": "all",
                "create_time": 1_795_589_543.5,
                "content": {"content_type": "text", "parts": [text]},
            }
        })
    }

    fn conversation(nodes: Vec<Value>, current: &str) -> Value {
        let mut mapping = serde_json::Map::new();
        for n in nodes {
            mapping.insert(n["id"].as_str().unwrap().to_string(), n.clone());
        }
        json!({"current_node": current, "mapping": mapping})
    }

    /// 活动分支必须按「根 → 叶」给出，而不是按 mapping 的插入顺序
    #[test]
    fn the_active_branch_is_walked_from_the_leaf_upwards() {
        let conv = conversation(
            vec![
                node("root", None, "system", "根"),
                node("u1", Some("root"), "user", "第一问"),
                node("a1", Some("u1"), "assistant", "第一答"),
                node("u2", Some("a1"), "user", "第二问"),
                node("a2", Some("u2"), "assistant", "第二答"),
                node("ghost", Some("u2"), "assistant", "被放弃的分支"),
            ],
            "a2",
        );
        let chain = active_branch(&conv);
        let ids: Vec<&str> = chain.iter().map(|n| n["id"].as_str().unwrap()).collect();
        assert_eq!(ids, vec!["root", "u1", "a1", "u2", "a2"]);
    }

    /// 工具调用节点（recipient 不是 all）不该混进对话记录
    #[test]
    fn tool_call_nodes_are_dropped() {
        let tool = json!({
            "author": {"role": "assistant"},
            "recipient": "web.run",
            "content": {"content_type": "text", "parts": ["{\"search_query\":[]}"]},
        });
        assert!(visible_text(&tool, true).is_none());
        let say = json!({
            "author": {"role": "assistant"},
            "recipient": "all",
            "content": {"content_type": "text", "parts": ["你好"]},
        });
        assert_eq!(visible_text(&say, false).unwrap().1, "你好");
    }

    /// 思考摘要默认不进正文，只有显式要求时才出现
    #[test]
    fn reasoning_is_opt_in() {
        let thoughts = json!({
            "author": {"role": "assistant"},
            "content": {
                "content_type": "thoughts",
                "thoughts": [{"summary": "已搜索 11 个网站", "content": "", "finished": true}],
            },
        });
        assert!(visible_text(&thoughts, false).is_none());
        let (kind, text) = visible_text(&thoughts, true).unwrap();
        assert_eq!(kind, "thoughts");
        assert_eq!(text, "已搜索 11 个网站");
    }

    /// 只有 parts 为空的思考节点 / 断开 parent 的畸形数据不能把命令卡死
    #[test]
    fn a_broken_parent_chain_terminates() {
        let conv = json!({
            "current_node": "a1",
            "mapping": {"a1": {"id": "a1", "parent": "missing", "message": null}}
        });
        let chain = active_branch(&conv);
        assert_eq!(chain.len(), 1);
        assert_eq!(active_branch(&json!({})).len(), 0);
    }
}
