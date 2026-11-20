//! `WebCommandSpec` HTTP 执行引擎。
//!
//! 职责：
//! 1. 将经过 `CommandSchema::validate_args` 校验的 JSON 参数映射到 Path / Query / Form / JSON Body / Header
//! 2. 通过 `SessionManager` 发送真实 HTTP 请求并维护 Cookie 会话
//! 3. 按 `ResponseExtract` 策略提取结构化结果

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use lilyco::__core::context::Context;
use lilyco::__core::error::AppError;
use lilyco::__core::progress::LogLevel;
use serde_json::{json, Value};

use crate::distill::distill_response;
use crate::dom::DomTree;
use crate::session::SessionManager;
use crate::spec::{
    ArgLocation, BodyEncoding, HttpMethod, ResponseExtract, WebArgKind, WebCommandSpec,
};
use crate::vault::guess_mime;

pub fn execute_web_command(
    ctx: &Context,
    session: &SessionManager,
    base_url: &str,
    cmd: &WebCommandSpec,
    args: &Value,
) -> Result<Value, AppError> {
    let args_obj = args
        .as_object()
        .ok_or_else(|| AppError::InvalidArg("args must be a JSON object".into()))?;

    let mut url_str = resolve_base_url(base_url, &cmd.url)?;
    let mut query_pairs: Vec<(String, String)> = Vec::new();
    let mut form_pairs: Vec<(String, String)> = Vec::new();
    let mut file_parts: Vec<(String, String)> = Vec::new();
    let mut json_body = serde_json::Map::new();
    let mut headers: BTreeMap<String, String> = cmd.headers.clone();

    // Static hidden parameters
    for (k, v) in &cmd.static_params {
        match cmd.encoding {
            BodyEncoding::Query => query_pairs.push((k.clone(), v.clone())),
            BodyEncoding::Form | BodyEncoding::Multipart => form_pairs.push((k.clone(), v.clone())),
            BodyEncoding::Json => {
                json_body.insert(k.clone(), Value::String(v.clone()));
            }
            BodyEncoding::None => {}
        }
    }

    // Dynamic arguments
    for arg in &cmd.args {
        let val_opt = args_obj.get(&arg.name).or(arg.default.as_ref());
        let Some(val) = val_opt else {
            continue;
        };
        if val.is_null() {
            continue;
        }
        let val_str = json_val_to_string(val);
        match arg.location {
            ArgLocation::Path => {
                let encoded = percent_encode(&val_str);
                let placeholder = format!("{{{}}}", arg.param_name);
                let pct_upper = format!("%7B{}%7D", arg.param_name);
                let pct_lower = format!("%7b{}%7d", arg.param_name);
                url_str = url_str
                    .replace(&placeholder, &encoded)
                    .replace(&pct_upper, &encoded)
                    .replace(&pct_lower, &encoded);
            }
            ArgLocation::Query => {
                if !matches!(val, Value::Bool(false)) {
                    query_pairs.push((arg.param_name.clone(), val_str));
                }
            }
            ArgLocation::Form => {
                if matches!(arg.kind, WebArgKind::Path) {
                    if !val_str.trim().is_empty() {
                        file_parts.push((arg.param_name.clone(), val_str));
                    }
                } else if !matches!(val, Value::Bool(false)) {
                    form_pairs.push((arg.param_name.clone(), val_str));
                }
            }
            ArgLocation::JsonBody => {
                json_body.insert(arg.param_name.clone(), val.clone());
            }
            ArgLocation::Header => {
                headers.insert(arg.param_name.clone(), val_str);
            }
        }
    }

    if !query_pairs.is_empty() {
        let mut parsed = url::Url::parse(&url_str)
            .map_err(|e| AppError::InvalidArg(format!("无效 URL `{url_str}`: {e}")))?;
        {
            let mut qp = parsed.query_pairs_mut();
            for (k, v) in &query_pairs {
                qp.append_pair(k, v);
            }
        }
        url_str = parsed.to_string();
    }

    ctx.log(
        LogLevel::Info,
        format!("{} {}", cmd.method.as_str(), url_str),
    );

    let encoded_body: Option<(String, Vec<u8>)> = match cmd.method {
        HttpMethod::Get | HttpMethod::Delete => None,
        HttpMethod::Post | HttpMethod::Put | HttpMethod::Patch => match cmd.encoding {
            BodyEncoding::Json => {
                let s = Value::Object(json_body).to_string();
                Some(("application/json".to_string(), s.into_bytes()))
            }
            BodyEncoding::Multipart => {
                let (ct, bytes) = build_multipart_body(&form_pairs, &file_parts)?;
                Some((ct, bytes))
            }
            BodyEncoding::Form | BodyEncoding::Query => {
                let mut ser = url::form_urlencoded::Serializer::new(String::new());
                for (k, v) in &form_pairs {
                    ser.append_pair(k, v);
                }
                Some((
                    "application/x-www-form-urlencoded".to_string(),
                    ser.finish().into_bytes(),
                ))
            }
            BodyEncoding::None => None,
        },
    };

    let body_ref = encoded_body
        .as_ref()
        .map(|(ct, payload)| (ct.as_str(), payload.as_slice()));

    let resp = session.request_bytes(cmd.method.as_str(), &url_str, &headers, body_ref, false)?;
    Ok(extract_response(
        &cmd.extract,
        &resp.final_url,
        resp.status,
        &resp.content_type,
        &resp.body,
    ))
}

pub fn extract_response(
    mode: &ResponseExtract,
    final_url: &str,
    status: u16,
    content_type: &str,
    body: &str,
) -> Value {
    match mode {
        ResponseExtract::Raw => json!({
            "url": final_url,
            "status": status,
            "content_type": content_type,
            "body": body,
        }),
        ResponseExtract::Json => {
            if let Ok(v) = serde_json::from_str::<Value>(body.trim_start()) {
                json!({
                    "url": final_url,
                    "status": status,
                    "data": v,
                })
            } else {
                json!({
                    "url": final_url,
                    "status": status,
                    "text": body,
                })
            }
        }
        ResponseExtract::Markdown => {
            let page = distill_response(final_url, content_type, body, 0);
            json!({
                "url": final_url,
                "status": status,
                "title": page.title,
                "markdown": page.markdown,
            })
        }
        ResponseExtract::Links => {
            let page = distill_response(final_url, content_type, body, 0);
            let links: Vec<_> = page
                .actions
                .into_iter()
                .filter(|a| a.href.is_some())
                .collect();
            json!({
                "url": final_url,
                "status": status,
                "title": page.title,
                "links": links,
            })
        }
        ResponseExtract::Tables => {
            let page = distill_response(final_url, content_type, body, 0);
            json!({
                "url": final_url,
                "status": status,
                "title": page.title,
                "tables": page.tables,
            })
        }
        ResponseExtract::Selector(sel) => {
            let dom = DomTree::parse(body);
            let matches: Vec<Value> = dom
                .select(sel)
                .into_iter()
                .map(|id| {
                    json!({
                        "tag": dom.tag(id),
                        "text": dom.text(id),
                        "href": dom.attr(id, "href"),
                    })
                })
                .collect();
            json!({
                "url": final_url,
                "status": status,
                "selector": sel,
                "matches": matches,
            })
        }
        ResponseExtract::Auto | ResponseExtract::Distill => {
            if content_type.contains("json") || body.trim_start().starts_with(['{', '[']) {
                if let Ok(v) = serde_json::from_str::<Value>(body.trim_start()) {
                    return json!({
                        "url": final_url,
                        "status": status,
                        "data": v,
                    });
                }
            }
            let page = distill_response(final_url, content_type, body, 4000);
            json!({
                "url": final_url,
                "status": status,
                "title": page.title,
                "compact_view": page.compact_view,
                "actions_count": page.actions.len(),
                "tables_count": page.tables.len(),
            })
        }
    }
}

fn resolve_base_url(base_url: &str, target: &str) -> Result<String, AppError> {
    if target.starts_with("http://") || target.starts_with("https://") {
        return Ok(target.to_string());
    }
    let base = url::Url::parse(base_url)
        .map_err(|e| AppError::InvalidArg(format!("无效 base_url `{base_url}`: {e}")))?;
    base.join(target)
        .map(|u| u.to_string())
        .map_err(|e| AppError::InvalidArg(format!("无法拼接 URL `{target}`: {e}")))
}

fn json_val_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn percent_encode(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

fn build_multipart_body(
    form_pairs: &[(String, String)],
    file_parts: &[(String, String)],
) -> Result<(String, Vec<u8>), AppError> {
    let boundary = "----LilycoWeb2CliBoundary7MA4YWxkTrZu0gW";
    let mut body = Vec::new();

    for (k, v) in form_pairs {
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        body.extend_from_slice(
            format!(
                "Content-Disposition: form-data; name=\"{}\"\r\n\r\n",
                k.replace('"', "")
            )
            .as_bytes(),
        );
        body.extend_from_slice(v.as_bytes());
        body.extend_from_slice(b"\r\n");
    }

    for (k, path_str) in file_parts {
        let path = Path::new(path_str);
        let bytes = fs::read(path).map_err(|e| {
            AppError::InvalidInput(format!("读取上传附件 `{}` 失败: {e}", path.display()))
        })?;
        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("attachment.bin")
            .replace('"', "");
        let mime = guess_mime(path);

        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        body.extend_from_slice(
            format!(
                "Content-Disposition: form-data; name=\"{}\"; filename=\"{}\"\r\n",
                k.replace('"', ""),
                filename
            )
            .as_bytes(),
        );
        body.extend_from_slice(format!("Content-Type: {mime}\r\n\r\n").as_bytes());
        body.extend_from_slice(&bytes);
        body.extend_from_slice(b"\r\n");
    }

    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    Ok((format!("multipart/form-data; boundary={boundary}"), body))
}
