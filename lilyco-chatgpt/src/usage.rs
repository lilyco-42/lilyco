//! `lchatgpt usage` —— 两套额度各还剩多少、什么时候恢复。
//!
//! 账号上有两份互不相干的配额，这里分开报：
//!
//! - **codex 额度**（`GET /backend-api/codex/usage`）：5 小时窗 + 周窗，带 used_percent。
//!   这条决定 `ask --allow-codex` 能不能跑。
//! - **chat 额度**（网页那条路，`ask` 默认走它）：
//!   - `GET /backend-api/wham/usage` 是网页自己用的那份 rate_limit（5 小时窗 + 周窗）。
//!     实测它和 `/codex/usage` **当前报同一份窗口**；两个池子是真合并还是各算各的后端没说，
//!     所以两份都摆出来、数值并列，让用户自己看到它们一样。
//!   - `POST /backend-api/conversation/init` 给 deep_research / file_upload /
//!     paste_text_to_file / image_gen 这类 feature 的剩余次数，**没有消息条数的维度** ——
//!     网页聊天还剩几条消息，后端不提供接口，只能以「发出去成不成功」体现。

use crate::api::Api;
use crate::auth;
use crate::util;
use lilyco::prelude::*;
use serde_json::{json, Value};

/// 两套额度分开报
#[derive(App)]
#[app(
    name = "usage",
    run = "run_usage",
    about = "Report both quota pools for this account and when they recover: the CODEX pool from GET /backend-api/codex/usage (plan_type, allowed, limit_reached, credits, primary_window, secondary_window, model_usage, where each window carries used_percent / limit_window_seconds / reset_at plus reset_at_iso and reset_in_human) and the CHAT pool the web app itself reads: GET /backend-api/wham/usage for its rate_limit windows, plus POST /backend-api/conversation/init for default_model_slug and every limits_progress entry as feature / remaining / reset_after. Neither pool exposes a message count, and the two rate_limit endpoints currently report the same windows. Read-only (safety T0); nothing is sent, no quota is consumed."
)]
pub struct Usage {}

/// codex 的一个配额窗口
fn window(v: Option<&Value>) -> Value {
    let w = match v {
        Some(w) => w,
        None => return Value::Null,
    };
    let reset_at = w.get("reset_at").and_then(Value::as_i64);
    let secs = w.get("reset_after_seconds").and_then(Value::as_i64);
    json!({
        "used_percent": w.get("used_percent").cloned().unwrap_or(Value::Null),
        "limit_window_seconds": w.get("limit_window_seconds").cloned().unwrap_or(Value::Null),
        "reset_at": reset_at,
        "reset_at_iso": reset_at.map(util::iso8601_from_epoch_secs).unwrap_or_default(),
        "reset_in_human": secs.map(human_span).unwrap_or_default(),
    })
}

/// 秒数 → 「1 小时 23 分」。用户问的是「还要等多久」，不是「还有几秒」。
pub fn human_span(secs: i64) -> String {
    if secs <= 0 {
        return "0 秒".to_string();
    }
    let (d, h, m, s) = (
        secs / 86_400,
        (secs % 86_400) / 3600,
        (secs % 3600) / 60,
        secs % 60,
    );
    let mut parts = Vec::new();
    if d > 0 {
        parts.push(format!("{d} 天"));
    }
    if h > 0 {
        parts.push(format!("{h} 小时"));
    }
    if m > 0 && d == 0 {
        parts.push(format!("{m} 分"));
    }
    if s > 0 && d == 0 && h == 0 {
        parts.push(format!("{s} 秒"));
    }
    if parts.is_empty() {
        "0 秒".to_string()
    } else {
        parts.join(" ")
    }
}

/// `POST /backend-api/conversation/init` 的 `limits_progress` → 一份 feature 清单。
///
/// 字段名就是 `feature_name` / `remaining` / `reset_after`（不是 remaining_count 之类），
/// `reset_after` 是带时区的 ISO-8601 字符串（形如 `2026-12-25T13:16:51+00:00`）。
fn chat_limits(init: &Value) -> Vec<Value> {
    let mut out = Vec::new();
    let list = match init.get("limits_progress").and_then(Value::as_array) {
        Some(l) => l,
        None => return out,
    };
    for item in list {
        out.push(json!({
            "feature": item.get("feature_name").cloned().unwrap_or(Value::Null),
            "remaining": item.get("remaining").cloned().unwrap_or(Value::Null),
            "reset_after": item.get("reset_after").cloned().unwrap_or(Value::Null),
        }));
    }
    out
}

fn run_usage(_app: &Usage, ctx: &Context) -> Result<Value, AppError> {
    let start = std::time::Instant::now();
    let api = Api::new(auth::load()?);
    ctx.emit(Progress::Started {
        total: Some(2),
        message: Some("查询两套额度".to_string()),
    });

    // ---- codex 额度 ----
    let u = api.get_json("/codex/usage", &[])?;
    let rl = u.get("rate_limit");
    let allowed = rl
        .and_then(|r| r.get("allowed"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let limit_reached = rl
        .and_then(|r| r.get("limit_reached"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let primary = window(rl.and_then(|r| r.get("primary_window")));
    let secondary = window(rl.and_then(|r| r.get("secondary_window")));
    ctx.tick(1, Some(2), "");

    // ---- chat 额度（网页自己读的那份 rate_limit + feature 维度；读不到也不算失败） ----
    // /wham/usage 是 chatgpt.com 网页启动时就打的那一个，比 /codex/usage 更贴近 chat 口径。
    let wham = api.get_json("/wham/usage", &[]).ok();
    let chat_rl = wham.as_ref().and_then(|w| w.get("rate_limit"));
    let chat = match api.post_json("/conversation/init", &json!({})) {
        Ok(init) => json!({
            "ok": true,
            "default_model_slug": init.get("default_model_slug").cloned().unwrap_or(Value::Null),
            "limits": chat_limits(&init),
            "rate_limit": json!({
                "available": chat_rl.is_some(),
                "allowed": chat_rl.and_then(|r| r.get("allowed")).cloned().unwrap_or(Value::Null),
                "limit_reached": chat_rl.and_then(|r| r.get("limit_reached")).cloned().unwrap_or(Value::Null),
                "primary_window": window(chat_rl.and_then(|r| r.get("primary_window"))),
                "secondary_window": window(chat_rl.and_then(|r| r.get("secondary_window"))),
            }),
            "note": "chat 侧没有「还剩几条消息」这种接口；wham/usage 的窗口与 codex/usage 目前报同一份",
        }),
        Err(e) => json!({
            "ok": false,
            "error": format!("{e}"),
            "limits": [],
            "rate_limit": Value::Null,
        }),
    };

    let result = json!({
        "plan_type": u.get("plan_type").cloned().unwrap_or(Value::Null),
        "email": u.get("email").cloned().unwrap_or(Value::Null),
        "allowed": allowed,
        "limit_reached": limit_reached,
        "credits": u.get("credits").cloned().unwrap_or(Value::Null),
        "primary_window": primary,
        "secondary_window": secondary,
        "model_usage": u.get("model_usage").cloned().unwrap_or(Value::Null),
        "chat": chat,
    });

    // ---- 人读面：两段分开说，免得把「网页聊天还能说几句」和「codex 还能跑几轮」搞混 ----
    let plan = result["plan_type"].as_str().unwrap_or("unknown");
    let reset_in = primary
        .get("reset_in_human")
        .and_then(Value::as_str)
        .unwrap_or("");
    if limit_reached {
        let p = if reset_in.is_empty() {
            String::new()
        } else {
            format!(
                "，{reset_in}后恢复（{}）",
                primary["reset_at_iso"].as_str().unwrap_or("")
            )
        };
        ctx.log(
            LogLevel::Warn,
            format!("codex 额度已用尽（{plan}），此时 `lchatgpt ask --allow-codex` 会返回 429{p}"),
        );
    } else {
        ctx.log(
            LogLevel::Info,
            format!("codex 额度：可以发送（plan: {plan}）—— 只影响 `ask --allow-codex`"),
        );
    }
    match result["chat"]["ok"].as_bool() {
        Some(true) => {
            let slug = result["chat"]["default_model_slug"]
                .as_str()
                .unwrap_or("(未知)");
            ctx.log(
                LogLevel::Info,
                format!("chat 额度：网页默认模型 {slug} —— `ask` 默认走这条，不花 codex"),
            );
            // 网页自己读的 rate_limit：与 codex 那份并列摆出来，一眼看出是不是同一套窗口
            let crl = &result["chat"]["rate_limit"];
            if crl["available"].as_bool() == Some(true) {
                for (label, key) in [("5 小时窗", "primary_window"), ("周窗", "secondary_window")]
                {
                    let w = &crl[key];
                    if w.is_null() {
                        continue;
                    }
                    let used = w["used_percent"]
                        .as_i64()
                        .map(|n| format!("{n}%"))
                        .unwrap_or_else(|| "?".to_string());
                    let reset = w["reset_in_human"].as_str().unwrap_or("");
                    let tail = if reset.is_empty() {
                        String::new()
                    } else {
                        format!("（{reset}后恢复）")
                    };
                    ctx.log(LogLevel::Info, format!("  {label}: 已用 {used}{tail}"));
                }
                ctx.log(
                    LogLevel::Info,
                    "  （这两个窗口和上面 codex 那份数值一样：网页额度也记在这套窗口里）"
                        .to_string(),
                );
            }
            for item in result["chat"]["limits"].as_array().into_iter().flatten() {
                let feature = item["feature"].as_str().unwrap_or("?");
                let remaining = item["remaining"]
                    .as_i64()
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| "?".to_string());
                let reset = item["reset_after"].as_str().unwrap_or("");
                let tail = if reset.is_empty() {
                    String::new()
                } else {
                    format!("，{reset} 重置")
                };
                ctx.log(LogLevel::Info, format!("  {feature}: 剩 {remaining}{tail}"));
            }
            ctx.log(
                LogLevel::Info,
                "  （消息条数没有接口可查；能不能发就看 ask 成不成）。".to_string(),
            );
        }
        _ => {
            let why = result["chat"]["error"].as_str().unwrap_or("");
            ctx.log(LogLevel::Warn, format!("chat 侧额度没读到：{why}"));
        }
    }
    ctx.tick(2, Some(2), "");
    ctx.done(result.clone(), start.elapsed().as_millis() as u64);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 用户看的是「还要等多久」，不是秒数
    #[test]
    fn seconds_become_a_readable_span() {
        assert_eq!(human_span(0), "0 秒");
        assert_eq!(human_span(45), "45 秒");
        assert_eq!(human_span(90), "1 分 30 秒");
        assert_eq!(human_span(3_600 + 600), "1 小时 10 分");
        assert_eq!(human_span(86_400 + 7_200), "1 天 2 小时");
    }

    /// 缺字段的窗口不能崩，只能给 null / 空串
    #[test]
    fn a_window_without_timestamps_degrades_to_empty_strings() {
        let w = window(Some(&json!({"used_percent": 100})));
        assert_eq!(w["used_percent"], 100);
        assert_eq!(w["reset_at_iso"], "");
        assert_eq!(w["reset_in_human"], "");
        assert_eq!(window(None), Value::Null);
    }

    /// chat 侧额度取的是 feature_name / remaining / reset_after 这三个字段
    #[test]
    fn chat_limits_are_read_from_limits_progress() {
        let init = json!({"default_model_slug": "gpt-6-2-thinking", "limits_progress": [
            {"feature_name": "deep_research", "remaining": 25, "reset_after": "2026-12-25T13:16:51+00:00"},
            {"feature_name": "image_gen", "remaining": 120, "reset_after": "2026-11-26T13:16:51+00:00"}
        ]});
        let limits = chat_limits(&init);
        assert_eq!(limits.len(), 2);
        assert_eq!(limits[0]["feature"], "deep_research");
        assert_eq!(limits[0]["remaining"], 25);
        assert_eq!(limits[1]["reset_after"], "2026-11-26T13:16:51+00:00");
        // 没有 limits_progress 时给空清单，不能 panic
        assert!(chat_limits(&json!({})).is_empty());
    }
}
