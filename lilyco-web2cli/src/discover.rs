//! 网页 / OpenAPI / HAR 自动发现引擎：将任意网页转化为 `WebCliSpec`。
//!
//! 三条发现通路：
//! 1. `discover_from_html`: 解析 `<form>`（含 `<label>` / `<select>` / `<input>` 类型映射）+ 静态扫描 `<script>` 中的 API 端点
//! 2. `discover_from_openapi`: 解析 OpenAPI 3.x / Swagger 2.0 JSON，直接生成全量类型化命令
//! 3. `discover_from_har`: 从浏览器 HAR 抓包自动过滤静态资源、路径参数模板化并推导 JSON/Query 参数

use std::collections::{BTreeMap, HashMap, HashSet};

use lilyco::__core::error::AppError;
use lilyco::__core::safety::SafetyTier;
use regex::Regex;
use serde_json::Value;

use crate::distill::resolve_url;
use crate::dom::DomTree;
use crate::spec::{
    sanitize_ident, ArgLocation, BodyEncoding, HttpMethod, ResponseExtract, WebArgKind, WebArgSpec,
    WebCliSpec, WebCommandSpec,
};

/// 从任意 HTML 页面自动发现表单与 API 端点，生成 `WebCliSpec`。
pub fn discover_from_html(page_url: &str, html: &str) -> WebCliSpec {
    let dom = DomTree::parse(html);
    let base = url::Url::parse(page_url).ok();
    let site_name = base
        .as_ref()
        .and_then(|u| u.host_str())
        .map(|h| sanitize_ident(h, "web"))
        .unwrap_or_else(|| "web".to_string());
    let title = dom
        .select("title")
        .first()
        .map(|&id| dom.text(id))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| page_url.to_string());

    // Build map of <label for="id"> -> text
    let mut labels_by_for: HashMap<String, String> = HashMap::new();
    for lid in dom.select("label") {
        if let Some(target) = dom.attr(lid, "for") {
            let txt = dom.text(lid);
            if !txt.is_empty() {
                labels_by_for.insert(target.to_string(), txt);
            }
        }
    }

    let mut commands = Vec::new();

    // 1. HTML <form> -> typed WebCommandSpec
    for (idx, form_id) in dom.select("form").into_iter().enumerate() {
        let action_raw = dom.attr(form_id, "action").unwrap_or("");
        let action_url = if action_raw.is_empty() {
            page_url.to_string()
        } else {
            resolve_url(base.as_ref(), action_raw)
        };
        let method_str = dom
            .attr(form_id, "method")
            .unwrap_or("get")
            .to_ascii_uppercase();
        let method = if method_str == "POST" {
            HttpMethod::Post
        } else {
            HttpMethod::Get
        };
        let mut encoding = match method {
            HttpMethod::Post => {
                let enctype = dom
                    .attr(form_id, "enctype")
                    .unwrap_or("")
                    .to_ascii_lowercase();
                if enctype.contains("multipart") {
                    BodyEncoding::Multipart
                } else if enctype.contains("json") {
                    BodyEncoding::Json
                } else {
                    BodyEncoding::Form
                }
            }
            _ => BodyEncoding::Query,
        };
        let safety = if method == HttpMethod::Get {
            SafetyTier::ReadOnly
        } else {
            SafetyTier::Confirm
        };

        let raw_name = dom
            .attr(form_id, "id")
            .or_else(|| dom.attr(form_id, "name"))
            .or_else(|| dom.attr(form_id, "aria-label"))
            .map(String::from)
            .unwrap_or_else(|| {
                let path_hint = url::Url::parse(&action_url)
                    .ok()
                    .and_then(|u| {
                        u.path_segments()
                            .and_then(|mut s| s.rfind(|seg| !seg.is_empty()).map(String::from))
                    })
                    .unwrap_or_default();
                if path_hint.is_empty() {
                    format!("form-{}", idx + 1)
                } else {
                    path_hint
                }
            });

        let mut args = Vec::new();
        let mut static_params = BTreeMap::new();
        collect_form_controls(
            &dom,
            form_id,
            &labels_by_for,
            method,
            encoding,
            &mut args,
            &mut static_params,
        );

        // 只要表单包含文件上传控件，自动升级为 Multipart 编码
        if method == HttpMethod::Post && args.iter().any(|a| matches!(a.kind, WebArgKind::Path)) {
            encoding = BodyEncoding::Multipart;
        }

        commands.push(WebCommandSpec {
            name: sanitize_ident(&raw_name, "form"),
            about: format!("Submit form {} ({})", action_url, method.as_str()),
            method,
            url: action_url,
            encoding,
            args,
            static_params,
            headers: BTreeMap::new(),
            extract: ResponseExtract::Auto,
            safety,
        });
    }

    // 2. Static JS / HTML API endpoint discovery
    let mut discovered_apis = HashSet::new();
    let api_re = Regex::new(
        r#"(?:fetch|axios\.(?:get|post|put|delete)|\.open\([^,]+,)\s*['"`]((?:/|https?://)[^'"`\s?]+)"#,
    )
    .expect("valid regex");
    let path_api_re =
        Regex::new(r#"['"`](/(?:api|v[0-9]+)/[a-zA-Z0-9_/{}-]+)['"`]"#).expect("valid regex");

    for sid in dom.select("script") {
        let script = dom.raw_text(sid);
        for cap in api_re.captures_iter(&script) {
            if let Some(m) = cap.get(1) {
                discovered_apis.insert(m.as_str().to_string());
            }
        }
        for cap in path_api_re.captures_iter(&script) {
            if let Some(m) = cap.get(1) {
                discovered_apis.insert(m.as_str().to_string());
            }
        }
    }

    let param_re = Regex::new(r"\{([a-zA-Z0-9_]+)\}").expect("valid regex");
    for api_path in discovered_apis.into_iter().take(25) {
        let resolved = resolve_url(base.as_ref(), &api_path);
        let mut args = Vec::new();
        for cap in param_re.captures_iter(&api_path) {
            if let Some(p) = cap.get(1) {
                args.push(WebArgSpec {
                    name: sanitize_ident(p.as_str(), "id"),
                    param_name: p.as_str().to_string(),
                    about: format!("Path parameter `{}`", p.as_str()),
                    location: ArgLocation::Path,
                    kind: WebArgKind::Text,
                    required: true,
                    default: None,
                });
            }
        }
        let cmd_name = api_path
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .trim_start_matches('/');
        commands.push(WebCommandSpec {
            name: sanitize_ident(cmd_name, "api"),
            about: format!("Discovered API endpoint: GET {resolved}"),
            method: HttpMethod::Get,
            url: resolved,
            encoding: BodyEncoding::Query,
            args,
            static_params: BTreeMap::new(),
            headers: BTreeMap::new(),
            extract: ResponseExtract::Auto,
            safety: SafetyTier::ReadOnly,
        });
    }

    // 3. Always include universal page extraction commands for this URL
    commands.push(WebCommandSpec {
        name: "read-page".to_string(),
        about: format!("Fetch and distill {page_url}"),
        method: HttpMethod::Get,
        url: page_url.to_string(),
        encoding: BodyEncoding::None,
        args: Vec::new(),
        static_params: BTreeMap::new(),
        headers: BTreeMap::new(),
        extract: ResponseExtract::Distill,
        safety: SafetyTier::ReadOnly,
    });
    commands.push(WebCommandSpec {
        name: "links".to_string(),
        about: format!("Extract all links from {page_url}"),
        method: HttpMethod::Get,
        url: page_url.to_string(),
        encoding: BodyEncoding::None,
        args: Vec::new(),
        static_params: BTreeMap::new(),
        headers: BTreeMap::new(),
        extract: ResponseExtract::Links,
        safety: SafetyTier::ReadOnly,
    });
    commands.push(WebCommandSpec {
        name: "tables".to_string(),
        about: format!("Extract HTML tables from {page_url}"),
        method: HttpMethod::Get,
        url: page_url.to_string(),
        encoding: BodyEncoding::None,
        args: Vec::new(),
        static_params: BTreeMap::new(),
        headers: BTreeMap::new(),
        extract: ResponseExtract::Tables,
        safety: SafetyTier::ReadOnly,
    });

    let mut spec = WebCliSpec {
        name: site_name,
        base_url: page_url.to_string(),
        about: format!("Auto-generated CLI for {title} ({page_url})"),
        commands,
    };
    spec.normalize();
    spec
}

fn collect_form_controls(
    dom: &DomTree,
    node_id: usize,
    labels: &HashMap<String, String>,
    method: HttpMethod,
    encoding: BodyEncoding,
    args: &mut Vec<WebArgSpec>,
    static_params: &mut BTreeMap<String, String>,
) {
    let location = match (method, encoding) {
        (HttpMethod::Get, _) => ArgLocation::Query,
        (_, BodyEncoding::Json) => ArgLocation::JsonBody,
        _ => ArgLocation::Form,
    };

    if let Some(tag) = dom.tag(node_id) {
        match tag {
            "input" => {
                if let Some(param_name) = dom.attr(node_id, "name").filter(|s| !s.is_empty()) {
                    let itype = dom
                        .attr(node_id, "type")
                        .unwrap_or("text")
                        .to_ascii_lowercase();
                    if itype == "hidden" {
                        let val = dom.attr(node_id, "value").unwrap_or("").to_string();
                        static_params.insert(param_name.to_string(), val);
                    } else if !matches!(itype.as_str(), "submit" | "button" | "reset" | "image") {
                        let about = control_about(dom, node_id, labels, param_name);
                        let required = dom.has_attr(node_id, "required");
                        let default = dom
                            .attr(node_id, "value")
                            .filter(|s| !s.is_empty())
                            .map(|s| Value::String(s.to_string()));
                        let kind = match itype.as_str() {
                            "checkbox" => WebArgKind::Flag,
                            "number" | "range" => WebArgKind::Number {
                                min: dom.attr(node_id, "min").and_then(|s| s.parse().ok()),
                                max: dom.attr(node_id, "max").and_then(|s| s.parse().ok()),
                            },
                            "file" => WebArgKind::Path,
                            _ => WebArgKind::Text,
                        };
                        args.push(WebArgSpec {
                            name: sanitize_ident(param_name, "arg"),
                            param_name: param_name.to_string(),
                            about,
                            location,
                            kind,
                            required,
                            default,
                        });
                    }
                }
            }
            "textarea" => {
                if let Some(param_name) = dom.attr(node_id, "name").filter(|s| !s.is_empty()) {
                    let about = control_about(dom, node_id, labels, param_name);
                    let required = dom.has_attr(node_id, "required");
                    args.push(WebArgSpec {
                        name: sanitize_ident(param_name, "arg"),
                        param_name: param_name.to_string(),
                        about,
                        location,
                        kind: WebArgKind::Text,
                        required,
                        default: None,
                    });
                }
            }
            "select" => {
                if let Some(param_name) = dom.attr(node_id, "name").filter(|s| !s.is_empty()) {
                    let about = control_about(dom, node_id, labels, param_name);
                    let required = dom.has_attr(node_id, "required");
                    let mut choices = Vec::new();
                    let mut default_choice = None;
                    for &c in &dom.node(node_id).children {
                        if dom.tag(c) == Some("option") {
                            let val = dom
                                .attr(c, "value")
                                .map(String::from)
                                .unwrap_or_else(|| dom.text(c));
                            if !val.is_empty() {
                                if dom.has_attr(c, "selected") && default_choice.is_none() {
                                    default_choice = Some(Value::String(val.clone()));
                                }
                                choices.push(val);
                            }
                        }
                    }
                    let kind = if choices.is_empty() {
                        WebArgKind::Text
                    } else {
                        WebArgKind::Enum { values: choices }
                    };
                    args.push(WebArgSpec {
                        name: sanitize_ident(param_name, "arg"),
                        param_name: param_name.to_string(),
                        about,
                        location,
                        kind,
                        required,
                        default: default_choice,
                    });
                }
                return; // options already handled
            }
            _ => {}
        }
    }

    for &c in &dom.node(node_id).children {
        collect_form_controls(dom, c, labels, method, encoding, args, static_params);
    }
}

fn control_about(
    dom: &DomTree,
    node_id: usize,
    labels: &HashMap<String, String>,
    fallback_name: &str,
) -> String {
    if let Some(id_attr) = dom.attr(node_id, "id") {
        if let Some(lbl) = labels.get(id_attr) {
            return lbl.clone();
        }
    }
    dom.attr(node_id, "aria-label")
        .or_else(|| dom.attr(node_id, "placeholder"))
        .or_else(|| dom.attr(node_id, "title"))
        .map(String::from)
        .unwrap_or_else(|| format!("Field `{fallback_name}`"))
}

/// 从 OpenAPI 3.x / Swagger 2.0 JSON 文档生成 `WebCliSpec`。
pub fn discover_from_openapi(source_url: &str, json_str: &str) -> Result<WebCliSpec, AppError> {
    let root: Value = serde_json::from_str(json_str)
        .map_err(|e| AppError::InvalidInput(format!("解析 OpenAPI JSON 失败: {e}")))?;
    let paths = root
        .get("paths")
        .and_then(Value::as_object)
        .ok_or_else(|| AppError::InvalidInput("OpenAPI 文档缺少 `paths` 对象".into()))?;

    let title = root
        .pointer("/info/title")
        .and_then(Value::as_str)
        .unwrap_or("openapi-site");
    let description = root
        .pointer("/info/description")
        .and_then(Value::as_str)
        .unwrap_or("");

    let base_url = root
        .pointer("/servers/0/url")
        .and_then(Value::as_str)
        .map(|s| resolve_url(url::Url::parse(source_url).ok().as_ref(), s))
        .unwrap_or_else(|| {
            url::Url::parse(source_url)
                .ok()
                .map(|mut u| {
                    u.set_path("");
                    u.set_query(None);
                    u.to_string()
                })
                .unwrap_or_else(|| source_url.to_string())
        });

    let mut commands = Vec::new();
    for (path_str, methods_val) in paths {
        let Some(methods_obj) = methods_val.as_object() else {
            continue;
        };
        for (m_str, op_val) in methods_obj {
            let method = match m_str.to_ascii_lowercase().as_str() {
                "get" => HttpMethod::Get,
                "post" => HttpMethod::Post,
                "put" => HttpMethod::Put,
                "patch" => HttpMethod::Patch,
                "delete" => HttpMethod::Delete,
                _ => continue,
            };
            let op_id = op_val
                .get("operationId")
                .and_then(Value::as_str)
                .map(String::from)
                .unwrap_or_else(|| format!("{m_str}-{path_str}"));
            let about = op_val
                .get("summary")
                .or_else(|| op_val.get("description"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();

            let mut args = Vec::new();
            if let Some(params) = op_val.get("parameters").and_then(Value::as_array) {
                for p in params {
                    let Some(pname) = p.get("name").and_then(Value::as_str) else {
                        continue;
                    };
                    let loc = match p.get("in").and_then(Value::as_str).unwrap_or("query") {
                        "path" => ArgLocation::Path,
                        "header" => ArgLocation::Header,
                        _ => ArgLocation::Query,
                    };
                    let required = p.get("required").and_then(Value::as_bool).unwrap_or(false)
                        || loc == ArgLocation::Path;
                    let p_about = p
                        .get("description")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    let kind = schema_to_web_arg_kind(p.get("schema").unwrap_or(&Value::Null));
                    args.push(WebArgSpec {
                        name: sanitize_ident(pname, "arg"),
                        param_name: pname.to_string(),
                        about: p_about,
                        location: loc,
                        kind,
                        required,
                        default: None,
                    });
                }
            }

            // OpenAPI 3.x requestBody application/json
            let mut encoding = BodyEncoding::Query;
            if let Some(props) = op_val
                .pointer("/requestBody/content/application~1json/schema/properties")
                .and_then(Value::as_object)
            {
                encoding = BodyEncoding::Json;
                let req_list: HashSet<&str> = op_val
                    .pointer("/requestBody/content/application~1json/schema/required")
                    .and_then(Value::as_array)
                    .map(|arr| arr.iter().filter_map(Value::as_str).collect())
                    .unwrap_or_default();
                for (prop_name, prop_schema) in props {
                    let p_about = prop_schema
                        .get("description")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    args.push(WebArgSpec {
                        name: sanitize_ident(prop_name, "arg"),
                        param_name: prop_name.clone(),
                        about: p_about,
                        location: ArgLocation::JsonBody,
                        kind: schema_to_web_arg_kind(prop_schema),
                        required: req_list.contains(prop_name.as_str()),
                        default: None,
                    });
                }
            }

            let safety = if method == HttpMethod::Get {
                SafetyTier::ReadOnly
            } else {
                SafetyTier::Confirm
            };

            commands.push(WebCommandSpec {
                name: sanitize_ident(&op_id, "op"),
                about,
                method,
                url: path_str.clone(),
                encoding,
                args,
                static_params: BTreeMap::new(),
                headers: BTreeMap::new(),
                extract: ResponseExtract::Json,
                safety,
            });
        }
    }

    let mut spec = WebCliSpec {
        name: sanitize_ident(title, "openapi"),
        base_url,
        about: description.to_string(),
        commands,
    };
    spec.normalize();
    Ok(spec)
}

fn schema_to_web_arg_kind(schema: &Value) -> WebArgKind {
    if let Some(enum_arr) = schema.get("enum").and_then(Value::as_array) {
        let values: Vec<String> = enum_arr
            .iter()
            .filter_map(|v| v.as_str().map(String::from).or_else(|| Some(v.to_string())))
            .collect();
        if !values.is_empty() {
            return WebArgKind::Enum { values };
        }
    }
    match schema.get("type").and_then(Value::as_str) {
        Some("boolean") => WebArgKind::Flag,
        Some("integer") | Some("number") => WebArgKind::Number {
            min: schema.get("minimum").and_then(Value::as_f64),
            max: schema.get("maximum").and_then(Value::as_f64),
        },
        _ => WebArgKind::Text,
    }
}

/// 从 Chrome DevTools HAR 抓包自动发现 API 端点并生成 `WebCliSpec`。
pub fn discover_from_har(
    har_json: &str,
    host_filter: Option<&str>,
) -> Result<WebCliSpec, AppError> {
    let root: Value = serde_json::from_str(har_json)
        .map_err(|e| AppError::InvalidInput(format!("解析 HAR JSON 失败: {e}")))?;
    let entries = root
        .pointer("/log/entries")
        .and_then(Value::as_array)
        .ok_or_else(|| AppError::InvalidInput("HAR 缺少 `log.entries` 数组".into()))?;

    let mut commands = Vec::new();
    let mut seen_endpoints: HashSet<String> = HashSet::new();
    let mut detected_base = String::new();

    for entry in entries {
        let Some(req) = entry.get("request") else {
            continue;
        };
        let Some(url_str) = req.get("url").and_then(Value::as_str) else {
            continue;
        };
        let Ok(parsed) = url::Url::parse(url_str) else {
            continue;
        };
        if let Some(filter) = host_filter {
            if parsed.host_str() != Some(filter) {
                continue;
            }
        }
        if is_static_asset(parsed.path()) {
            continue;
        }

        if detected_base.is_empty() {
            detected_base = format!("{}://{}", parsed.scheme(), parsed.host_str().unwrap_or(""));
        }

        let method_str = req.get("method").and_then(Value::as_str).unwrap_or("GET");
        let method = match method_str.to_ascii_uppercase().as_str() {
            "GET" => HttpMethod::Get,
            "POST" => HttpMethod::Post,
            "PUT" => HttpMethod::Put,
            "PATCH" => HttpMethod::Patch,
            "DELETE" => HttpMethod::Delete,
            _ => continue,
        };

        let (templated_path, path_params) = templatize_path(parsed.path());
        let dedupe_key = format!(
            "{} {}{}",
            method.as_str(),
            parsed.host_str().unwrap_or(""),
            templated_path
        );
        if !seen_endpoints.insert(dedupe_key) {
            continue;
        }

        let mut args = Vec::new();
        for p in path_params {
            args.push(WebArgSpec {
                name: sanitize_ident(&p, "id"),
                param_name: p,
                about: "Templated path parameter from HAR trace".to_string(),
                location: ArgLocation::Path,
                kind: WebArgKind::Text,
                required: true,
                default: None,
            });
        }

        for (k, v) in parsed.query_pairs() {
            if args.iter().any(|a| a.param_name == k) {
                continue;
            }
            args.push(WebArgSpec {
                name: sanitize_ident(&k, "q"),
                param_name: k.to_string(),
                about: format!("Query parameter `{k}` (sample: `{v}`)"),
                location: ArgLocation::Query,
                kind: infer_kind_from_sample(&v),
                required: false,
                default: None,
            });
        }

        let mut encoding = BodyEncoding::Query;
        if let Some(post_text) = req.pointer("/postData/text").and_then(Value::as_str) {
            if let Ok(Value::Object(map)) = serde_json::from_str::<Value>(post_text) {
                encoding = BodyEncoding::Json;
                for (k, v) in map {
                    args.push(WebArgSpec {
                        name: sanitize_ident(&k, "field"),
                        param_name: k,
                        about: "JSON body field from HAR trace".to_string(),
                        location: ArgLocation::JsonBody,
                        kind: infer_kind_from_json(&v),
                        required: false,
                        default: None,
                    });
                }
            }
        }

        let endpoint_url = format!(
            "{}://{}{}",
            parsed.scheme(),
            parsed.host_str().unwrap_or(""),
            templated_path
        );
        let cmd_name = format!(
            "{}-{}",
            method.as_str().to_ascii_lowercase(),
            templated_path.trim_start_matches('/')
        );
        let safety = if method == HttpMethod::Get {
            SafetyTier::ReadOnly
        } else {
            SafetyTier::Confirm
        };

        commands.push(WebCommandSpec {
            name: sanitize_ident(&cmd_name, "api"),
            about: format!(
                "HAR captured endpoint: {} {}",
                method.as_str(),
                endpoint_url
            ),
            method,
            url: endpoint_url,
            encoding,
            args,
            static_params: BTreeMap::new(),
            headers: BTreeMap::new(),
            extract: ResponseExtract::Auto,
            safety,
        });
    }

    if detected_base.is_empty() {
        detected_base = "https://example.com".to_string();
    }
    let site_name = url::Url::parse(&detected_base)
        .ok()
        .and_then(|u| u.host_str().map(|h| sanitize_ident(h, "har-site")))
        .unwrap_or_else(|| "har-site".to_string());

    let mut spec = WebCliSpec {
        name: site_name,
        base_url: detected_base,
        about: "CLI synthesized from HAR network trace".to_string(),
        commands,
    };
    spec.normalize();
    Ok(spec)
}

fn is_static_asset(path: &str) -> bool {
    let lc = path.to_ascii_lowercase();
    [
        ".js", ".mjs", ".css", ".png", ".jpg", ".jpeg", ".gif", ".svg", ".webp", ".ico", ".woff",
        ".woff2", ".ttf", ".eot", ".map", ".mp4", ".webm",
    ]
    .iter()
    .any(|ext| lc.ends_with(ext))
}

fn templatize_path(path: &str) -> (String, Vec<String>) {
    let mut out_segs = Vec::new();
    let mut params = Vec::new();
    let mut id_idx = 1usize;
    for seg in path.split('/') {
        if seg.is_empty() {
            continue;
        }
        let is_num = seg.chars().all(|c| c.is_ascii_digit());
        let is_uuid = seg.len() >= 32 && seg.chars().all(|c| c.is_ascii_hexdigit() || c == '-');
        if is_num || is_uuid {
            let pname = if id_idx == 1 {
                "id".to_string()
            } else {
                format!("id{id_idx}")
            };
            id_idx += 1;
            out_segs.push(format!("{{{pname}}}"));
            params.push(pname);
        } else {
            out_segs.push(seg.to_string());
        }
    }
    (format!("/{}", out_segs.join("/")), params)
}

fn infer_kind_from_sample(s: &str) -> WebArgKind {
    if s == "true" || s == "false" {
        WebArgKind::Flag
    } else if s.parse::<f64>().is_ok() {
        WebArgKind::Number {
            min: None,
            max: None,
        }
    } else {
        WebArgKind::Text
    }
}

fn infer_kind_from_json(v: &Value) -> WebArgKind {
    match v {
        Value::Bool(_) => WebArgKind::Flag,
        Value::Number(_) => WebArgKind::Number {
            min: None,
            max: None,
        },
        _ => WebArgKind::Text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovers_html_forms_and_script_apis() {
        let html = r#"
        <html>
          <head><title>Book Store</title></head>
          <body>
            <form id="search_books" action="/books/search" method="get">
              <label for="q_input">Keyword</label>
              <input id="q_input" type="text" name="q" required>
              <select name="category">
                <option value="all" selected>All</option>
                <option value="rust">Rust</option>
              </select>
              <input type="checkbox" name="in_stock">
              <input type="hidden" name="fmt" value="json">
            </form>
            <script>
              fetch('/api/v1/books/{id}');
            </script>
          </body>
        </html>
        "#;
        let spec = discover_from_html("https://books.example.com/", html);
        let search = spec
            .commands
            .iter()
            .find(|c| c.name == "search-books")
            .expect("search-books form");
        assert_eq!(search.method, HttpMethod::Get);
        assert_eq!(search.url, "https://books.example.com/books/search");
        assert_eq!(
            search.static_params.get("fmt").map(String::as_str),
            Some("json")
        );
        assert_eq!(search.args.len(), 3);
        assert_eq!(search.args[0].about, "Keyword");
        assert!(matches!(search.args[1].kind, WebArgKind::Enum { .. }));
        assert!(matches!(search.args[2].kind, WebArgKind::Flag));

        let api_cmd = spec
            .commands
            .iter()
            .find(|c| c.name.contains("api-v1-books"))
            .expect("api endpoint");
        assert_eq!(api_cmd.args.len(), 1);
        assert_eq!(api_cmd.args[0].location, ArgLocation::Path);
    }

    #[test]
    fn discovers_openapi_and_har() {
        let openapi = r#"{
          "openapi": "3.0.0",
          "info": { "title": "Pet Store", "description": "Pets API" },
          "servers": [{ "url": "https://pets.example.com/v1" }],
          "paths": {
            "/pets/{petId}": {
              "get": {
                "operationId": "getPet",
                "summary": "Fetch a pet",
                "parameters": [
                  { "name": "petId", "in": "path", "required": true, "schema": { "type": "integer" } }
                ]
              }
            }
          }
        }"#;
        let spec = discover_from_openapi("https://pets.example.com/openapi.json", openapi).unwrap();
        assert_eq!(spec.commands.len(), 1);
        assert_eq!(spec.commands[0].name, "getpet");
        assert_eq!(spec.commands[0].args[0].location, ArgLocation::Path);

        let har = r#"{
          "log": {
            "entries": [
              { "request": { "method": "GET", "url": "https://api.example.com/static/app.js" } },
              { "request": { "method": "GET", "url": "https://api.example.com/users/42?verbose=true" } }
            ]
          }
        }"#;
        let har_spec = discover_from_har(har, None).unwrap();
        assert_eq!(har_spec.commands.len(), 1);
        assert_eq!(
            har_spec.commands[0].url,
            "https://api.example.com/users/{id}"
        );
        assert_eq!(har_spec.commands[0].args.len(), 2);
    }
}
