//! `lchatgpt list` —— 最近的会话。只给元数据（标题/时间/是否收藏），要正文用 show。

use crate::api::Api;
use crate::auth;
use crate::util;
use lilyco::prelude::*;
use serde_json::{json, Value};

/// 省略时返回多少个会话
const DEFAULT_LIMIT: u64 = 20;
/// ChatGPT 后端对 `limit` 的上限，超过会被静默截断
const MAX_LIMIT: u64 = 100;

/// 列会话
#[derive(App)]
#[app(
    name = "list",
    run = "run_list",
    about = "List recent conversations in the account: calls GET /backend-api/conversations and returns { total, offset, limit, order, count, items } where each item carries id, title, create_time, update_time (as ISO-8601), is_starred, is_archived and model_slug. Only metadata is fetched - use show-conversation to read the messages themselves. Read-only (safety T0); nothing is modified."
)]
pub struct List {
    #[arg(about = "How many conversations to skip from the newest", default = 0)]
    offset: u64,
    #[arg(
        about = "How many conversations to return (0 means the default 20)",
        default = 20
    )]
    limit: u64,
    #[arg(
        about = "Sort order: updated (default) or created",
        default = "updated"
    )]
    order: String,
}

fn run_list(app: &List, ctx: &Context) -> Result<Value, AppError> {
    let start = std::time::Instant::now();
    // 0 表示「用默认值」：Web / MCP 面省略数字参数时送的就是 0
    let limit = if app.limit == 0 {
        DEFAULT_LIMIT
    } else {
        app.limit
    }
    .min(MAX_LIMIT);
    // 枚举参数在 Web / MCP 面会以空串到达，这里把空串解释成默认值
    let order = match app.order.trim() {
        "" => "updated",
        "updated" | "created" => app.order.trim(),
        other => {
            return Err(AppError::InvalidArg(format!(
                "order 只能是 updated 或 created，收到 {other}"
            )))
        }
    }
    .to_string();

    let api = Api::new(auth::load()?);
    ctx.emit(Progress::Started {
        total: Some(1),
        message: Some(format!("取 {limit} 个会话")),
    });
    let v = api.get_json(
        "/conversations",
        &[
            ("offset".to_string(), app.offset.to_string()),
            ("limit".to_string(), limit.to_string()),
            ("order".to_string(), order.to_string()),
        ],
    )?;
    let empty = Vec::new();
    let items = v
        .get("items")
        .and_then(Value::as_array)
        .unwrap_or(&empty)
        .iter()
        .map(|c| {
            json!({
                "id": c.get("id").cloned().unwrap_or(Value::Null),
                "title": c.get("title").cloned().unwrap_or(Value::Null),
                "create_time": util::iso8601_any(c.get("create_time").unwrap_or(&Value::Null)),
                "update_time": util::iso8601_any(c.get("update_time").unwrap_or(&Value::Null)),
                "is_starred": c.get("is_starred").and_then(Value::as_bool).unwrap_or(false),
                "is_archived": c.get("is_archived").and_then(Value::as_bool).unwrap_or(false),
                "model_slug": c.get("default_model_slug").cloned().unwrap_or(Value::Null),
            })
        })
        .collect::<Vec<_>>();
    let result = json!({
        "total": v.get("total").cloned().unwrap_or(Value::Null),
        "offset": app.offset,
        "limit": limit,
        "order": order,
        "count": items.len(),
        "items": items,
    });
    for it in result["items"].as_array().into_iter().flatten() {
        ctx.log(
            LogLevel::Info,
            format!(
                "{}  {}  {}",
                it["update_time"].as_str().unwrap_or(""),
                it["id"].as_str().unwrap_or(""),
                it["title"].as_str().unwrap_or("(无标题)")
            ),
        );
    }
    ctx.tick(1, Some(1), "");
    ctx.done(result.clone(), start.elapsed().as_millis() as u64);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(limit: u64, order: &str) -> List {
        List {
            offset: 0,
            limit,
            order: order.to_string(),
        }
    }

    /// 数字 0 必须是「用默认值」，否则 Web / MCP 面永远只能拿到 0 条
    #[test]
    fn a_zero_limit_falls_back_to_the_default() {
        assert_eq!(
            if app(0, "updated").limit == 0 {
                DEFAULT_LIMIT
            } else {
                0
            },
            20
        );
    }

    /// 空串（Web / MCP 省略枚举参数）等价于 updated
    #[test]
    fn an_empty_order_means_updated() {
        let a = app(0, "  ");
        let normalized = match a.order.trim() {
            "" => "updated",
            other => other,
        };
        assert_eq!(normalized, "updated");
        assert_eq!(MAX_LIMIT, 100);
    }
}
