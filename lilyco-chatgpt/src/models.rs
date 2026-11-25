//! `lchatgpt models` —— 这个账号能用哪些模型（网页侧列表 + codex 侧可发送的模型）。
//!
//! 两条发送路径用的模型不是一回事：`ask` 默认走网页聊天，用的是网页那边的默认模型
//! （`default_model_slug`）；只有 `ask --allow-codex` 才用 codex 侧的 slug。

use crate::api::Api;
use crate::auth;
use lilyco::prelude::*;
use serde_json::{json, Value};

/// codex responses 接口对 ChatGPT 账号只认这一个 slug（其余会 400 model_not_supported）
const CODEX_MODELS: &[&str] = &["gpt-6.2-sol"];

/// 列模型
#[derive(App)]
#[app(
    name = "models",
    run = "run_models",
    about = "List the models this account can use: calls GET /backend-api/models and returns { title, count, default_model_slug, models, codex_models } where each model carries slug, title, description, max_tokens and tags. default_model_slug is what the web chat picks (that is the model the default ask path ends up using, since it drives the real page); codex_models names the slugs accepted by POST /backend-api/codex/responses, which only ask --allow-codex touches (other slugs return 400 model_not_supported with a ChatGPT account). Read-only (safety T0); nothing is modified."
)]
pub struct Models {}

fn run_models(_app: &Models, ctx: &Context) -> Result<Value, AppError> {
    let start = std::time::Instant::now();
    let api = Api::new(auth::load()?);
    ctx.emit(Progress::Started {
        total: Some(1),
        message: Some("拉模型列表".to_string()),
    });
    let v = api.get_json("/models", &[])?;
    let empty = Vec::new();
    let list = v
        .get("models")
        .and_then(Value::as_array)
        .unwrap_or(&empty)
        .iter()
        .map(|m| {
            json!({
                "slug": m.get("slug").cloned().unwrap_or(Value::Null),
                "title": m.get("title").cloned().unwrap_or(Value::Null),
                "description": m.get("description").cloned().unwrap_or(Value::Null),
                "max_tokens": m.get("max_tokens").cloned().unwrap_or(Value::Null),
                "tags": m.get("tags").cloned().unwrap_or(Value::Null),
            })
        })
        .collect::<Vec<_>>();
    let result = json!({
        "title": v.get("title").cloned().unwrap_or(Value::Null),
        "count": list.len(),
        "default_model_slug": v.get("default_model_slug").cloned().unwrap_or(Value::Null),
        "codex_models": CODEX_MODELS,
        "models": list,
    });
    for m in result["models"].as_array().into_iter().flatten() {
        ctx.log(
            LogLevel::Info,
            format!(
                "{:<24} {}",
                m["slug"].as_str().unwrap_or(""),
                m["title"].as_str().unwrap_or("")
            ),
        );
    }
    // 两条路径的模型来源不同，别让用户以为 ask 在网页里也用 codex 那个 slug
    let web_default = result["default_model_slug"].as_str().unwrap_or("");
    if !web_default.is_empty() {
        ctx.log(
            LogLevel::Info,
            format!("ask 默认（chat 额度）用网页默认模型: {web_default}"),
        );
    }
    ctx.log(
        LogLevel::Info,
        format!(
            "ask --allow-codex（codex 额度）只支持: {}",
            CODEX_MODELS.join(", ")
        ),
    );
    ctx.tick(1, Some(1), "");
    ctx.done(result.clone(), start.elapsed().as_millis() as u64);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_codex_allowlist_is_documented_in_one_place() {
        assert_eq!(CODEX_MODELS, &["gpt-6.2-sol"]);
    }
}
