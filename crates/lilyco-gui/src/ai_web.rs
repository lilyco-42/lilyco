//! AI 原生网页自描述协议（所有 `lilyco --gui` 应用开箱即用）：
//! - `GET /llms.txt`：面向 LLM/Agent 的极简命令与参数手册（只暴露可见命令，隐藏命令绝不泄漏）
//! - `GET /.well-known/agent.json`：A2A / Agent Card 服务发现清单
//! - `GET /openapi.json`：基于 `CommandSchema::to_json_schema()` 自动生成的 OpenAPI 3.1 规范

use std::sync::Arc;

use axum::extract::State;
use axum::http::header;
use axum::response::{IntoResponse, Response};
use axum::Json;
use lilyco_core::schema::CommandSchema;
use lilyco_core::theme::UI_PROTOCOL_VERSION;
use serde_json::{json, Map, Value};

use crate::state::AppState;

fn visible_schemas(state: &AppState) -> Vec<CommandSchema> {
    match &state.registry {
        Some(reg) => reg.visible().map(|c| c.schema.clone()).collect(),
        None => vec![state.schema.as_ref().clone()],
    }
}

/// `GET /llms.txt`：从 `CommandSchema` / `Registry` 自动生成极简 Agent 说明文档。
pub(crate) async fn llms_txt_handler(State(state): State<Arc<AppState>>) -> Response {
    let schemas = visible_schemas(&state);
    let mut lines = vec![
        format!("# Lilyco AI-Native Web App ({})", state.schema.name),
        String::new(),
        format!("> UI Protocol: v{UI_PROTOCOL_VERSION}"),
        "> Discovery: GET /.well-known/agent.json | GET /openapi.json | GET /themes".to_string(),
        "> Execution: POST /run (requires X-Lilyco-Token) -> GET /progress/{session_id} (SSE)"
            .to_string(),
        String::new(),
        "## Visible Commands".to_string(),
    ];

    for s in &schemas {
        lines.push(format!("- `{}` ({:?}): {}", s.name, s.safety, s.about));
        for a in &s.args {
            let req = if a.required { "required" } else { "optional" };
            lines.push(format!("  - `--{}` ({req}): {}", a.name, a.about));
        }
    }

    (
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        lines.join("\n"),
    )
        .into_response()
}

/// `GET /.well-known/agent.json`：自动生成标准 Agent Card。
pub(crate) async fn agent_card_handler(State(state): State<Arc<AppState>>) -> Json<Value> {
    let schemas = visible_schemas(&state);
    let skills: Vec<Value> = schemas
        .iter()
        .map(|s| {
            json!({
                "id": s.name,
                "name": s.name,
                "description": s.about,
                "safety": s.safety,
                "inputSchema": s.to_json_schema(),
            })
        })
        .collect();

    Json(json!({
        "name": state.schema.name,
        "description": state.schema.about,
        "protocol_version": UI_PROTOCOL_VERSION,
        "endpoints": {
            "ui": "/",
            "llms_txt": "/llms.txt",
            "openapi": "/openapi.json",
            "themes": "/themes",
            "run": "/run",
            "progress": "/progress/{session_id}"
        },
        "skills": skills,
    }))
}

/// `GET /openapi.json`：自动生成 OpenAPI 3.1.0 规范（所有可见命令的 JSON Schema 一致导出）。
pub(crate) async fn openapi_handler(State(state): State<Arc<AppState>>) -> Json<Value> {
    let schemas = visible_schemas(&state);
    let mut cmd_schemas = Map::new();
    for s in &schemas {
        cmd_schemas.insert(s.name.clone(), s.to_json_schema());
    }

    Json(json!({
        "openapi": "3.1.0",
        "info": {
            "title": format!("Lilyco Web App ({})", state.schema.name),
            "version": env!("CARGO_PKG_VERSION"),
            "description": state.schema.about,
        },
        "paths": {
            "/llms.txt": { "get": { "operationId": "llmsTxt", "summary": "LLM concise guide" } },
            "/.well-known/agent.json": { "get": { "operationId": "agentCard", "summary": "A2A Agent Card" } },
            "/themes": { "get": { "operationId": "listThemes", "summary": "10 built-in UI ThemeSpecs and AdapterCapabilities" } },
            "/run": {
                "post": {
                    "operationId": "runCommand",
                    "summary": "Execute a visible Lilyco command and receive an SSE session_id",
                    "requestBody": {
                        "required": true,
                        "content": {
                            "application/json": {
                                "schema": {
                                    "type": "object",
                                    "properties": {
                                        "cmd": { "type": "string" },
                                        "args": { "type": "object" }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        },
        "components": {
            "schemas": cmd_schemas
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use lilyco_core::registry::{RegisteredCommand, Registry};
    use lilyco_core::schema::{ArgKind, ArgSchema, CommandSchema};

    #[tokio::test]
    async fn ai_web_endpoints_expose_only_visible_commands() {
        let mut reg = Registry::new();
        let pub_schema = CommandSchema {
            name: "ping".into(),
            about: "Ping check".into(),
            args: vec![ArgSchema {
                name: "target".into(),
                about: "Target host".into(),
                kind: ArgKind::Text,
                required: true,
                default: None,
            }],
            subcommands: Vec::new(),
            safety: Default::default(),
        };
        let hidden_schema = CommandSchema {
            name: "secret-internal".into(),
            about: "Hidden".into(),
            args: Vec::new(),
            subcommands: Vec::new(),
            safety: Default::default(),
        };
        reg.register(RegisteredCommand::new("ping", pub_schema))
            .unwrap();
        reg.register(RegisteredCommand::new("secret-internal", hidden_schema).hidden(true))
            .unwrap();

        let first_schema = reg.visible().next().unwrap().schema.clone();
        let state = Arc::new(AppState {
            schema: Arc::new(first_schema),
            registry: Some(Arc::new(reg)),
            sessions: tokio::sync::Mutex::new(std::collections::HashMap::new()),
            cancels: tokio::sync::Mutex::new(std::collections::HashMap::new()),
            runner: Arc::new(|_args, _tx| Box::pin(async {})),
            token: "test-token".into(),
        });

        let llms_resp = llms_txt_handler(State(Arc::clone(&state))).await;
        let llms_bytes = to_bytes(llms_resp.into_body(), 1024 * 64).await.unwrap();
        let llms_str = String::from_utf8(llms_bytes.to_vec()).unwrap();
        assert!(llms_str.contains("`ping`"));
        assert!(!llms_str.contains("secret-internal"));

        let Json(card) = agent_card_handler(State(Arc::clone(&state))).await;
        let skills = card["skills"].as_array().unwrap();
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0]["id"], "ping");

        let Json(oa) = openapi_handler(State(state)).await;
        assert_eq!(oa["openapi"], "3.1.0");
        assert!(oa["components"]["schemas"].get("ping").is_some());
        assert!(oa["components"]["schemas"].get("secret-internal").is_none());
    }
}
