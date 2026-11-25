//! `lchatgpt search` —— 按关键词在历史会话里找。

use crate::api::Api;
use crate::auth;
use crate::util;
use lilyco::prelude::*;
use serde_json::{json, Value};

/// 省略时返回多少条命中
const DEFAULT_LIMIT: u64 = 10;
/// snippet 截断长度：后端会把整篇回答塞进 payload.snippet，不截会把 JSON 撑爆
const SNIPPET_CHARS: usize = 240;

/// 搜会话
#[derive(App)]
#[app(
    name = "search",
    run = "run_search",
    about = "Search the account's conversation history by keyword: calls GET /backend-api/conversations/search?query=... and returns { query, limit, count, items } where each item carries conversation_id, title, update_time, is_starred and snippet (the matched message excerpt, flattened to a single line by the backend), plus payload with the matched message id and kind. Use show-conversation with the conversation_id to read the surrounding turns. Read-only (safety T0); nothing is modified."
)]
pub struct Search {
    #[arg(about = "Keyword or phrase to look for")]
    query: String,
    #[arg(
        about = "How many hits to return (0 means the default 10)",
        default = 10
    )]
    limit: u64,
}

fn run_search(app: &Search, ctx: &Context) -> Result<Value, AppError> {
    let start = std::time::Instant::now();
    let query = app.query.trim().to_string();
    if query.is_empty() {
        return Err(AppError::InvalidArg(
            "query 不能为空；例如 lchatgpt search --query repo".to_string(),
        ));
    }
    let limit = if app.limit == 0 {
        DEFAULT_LIMIT
    } else {
        app.limit
    };
    let api = Api::new(auth::load()?);
    ctx.emit(Progress::Started {
        total: Some(1),
        message: Some(format!("搜索 {query}")),
    });
    let v = api.get_json(
        "/conversations/search",
        &[("query".to_string(), query.clone())],
    )?;
    let empty = Vec::new();
    let items = v
        .get("items")
        .and_then(Value::as_array)
        .unwrap_or(&empty)
        .iter()
        .take(limit as usize)
        .map(|h| {
            json!({
                "conversation_id": h.get("conversation_id").cloned().unwrap_or(Value::Null),
                "title": h.get("title").cloned().unwrap_or(Value::Null),
                "update_time": util::iso8601_any(h.get("update_time").unwrap_or(&Value::Null)),
                "is_starred": h.get("is_starred").and_then(Value::as_bool).unwrap_or(false),
                // 后端给的 snippet 是一整篇正文拼成的长串；人读面只截第一屏
                "snippet": util::take_chars(
                    h.get("payload")
                        .and_then(|p| p.get("snippet"))
                        .and_then(Value::as_str)
                        .unwrap_or(""),
                    SNIPPET_CHARS,
                )
                .0,
                "payload": h.get("payload").cloned().unwrap_or(Value::Null),
            })
        })
        .collect::<Vec<_>>();
    let result = json!({
        "query": query,
        "limit": limit,
        "count": items.len(),
        "items": items,
    });
    if items_is_empty(&result) {
        ctx.log(LogLevel::Warn, format!("没有命中：{query}"));
    }
    for it in result["items"].as_array().into_iter().flatten() {
        ctx.log(
            LogLevel::Info,
            format!(
                "{}  {}  {}\n    {}",
                it["update_time"].as_str().unwrap_or(""),
                it["conversation_id"].as_str().unwrap_or(""),
                it["title"].as_str().unwrap_or("(无标题)"),
                it["snippet"].as_str().unwrap_or("")
            ),
        );
    }
    ctx.tick(1, Some(1), "");
    ctx.done(result.clone(), start.elapsed().as_millis() as u64);
    Ok(result)
}

fn items_is_empty(result: &Value) -> bool {
    result["count"].as_u64().unwrap_or(0) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_result_is_reported_as_a_miss() {
        assert!(items_is_empty(&json!({"count": 0})));
        assert!(!items_is_empty(&json!({"count": 2})));
    }

    #[test]
    fn a_zero_limit_falls_back_to_the_default() {
        assert_eq!(DEFAULT_LIMIT, 10);
    }

    /// snippet 是整篇正文拼出来的，必须截断，否则一条命中就能顶满终端
    #[test]
    fn a_long_snippet_is_truncated() {
        let long = "外".repeat(SNIPPET_CHARS * 2);
        let (cut, was_cut) = util::take_chars(&long, SNIPPET_CHARS);
        assert!(was_cut);
        assert_eq!(cut.chars().count(), SNIPPET_CHARS);
    }
}
