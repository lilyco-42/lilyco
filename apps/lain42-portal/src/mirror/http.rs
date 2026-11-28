//! HTTP 缓存、上游代理（UPSTREAM_PROXY）与 GitHub Release 无速率限制拉取通道。

use std::collections::{HashMap, HashSet};
use std::sync::{OnceLock, RwLock};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

pub(super) const CACHE_TTL: Duration = Duration::from_secs(1800);

#[derive(Clone)]
struct CachedValue {
    expires: Instant,
    data: Value,
}

fn http_cache() -> &'static RwLock<HashMap<String, CachedValue>> {
    static CACHE: OnceLock<RwLock<HashMap<String, CachedValue>>> = OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(HashMap::new()))
}

fn upstream_proxy_url() -> Option<String> {
    for key in ["UPSTREAM_PROXY", "HTTPS_PROXY", "https_proxy"] {
        if let Ok(val) = std::env::var(key) {
            let trimmed = val.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

fn is_github_url(url: &str) -> bool {
    url.starts_with("https://api.github.com/") || url.starts_with("https://github.com/")
}

fn build_http_agent(url: &str, timeout_secs: u64) -> ureq::Agent {
    let mut cfg = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(timeout_secs)));
    if is_github_url(url) {
        if let Some(proxy_str) = upstream_proxy_url() {
            if let Ok(proxy) = ureq::Proxy::new(&proxy_str) {
                cfg = cfg.proxy(Some(proxy));
            }
        }
    }
    cfg.build().new_agent()
}

pub(super) fn fetch_json_cached(url: &str) -> Option<Value> {
    if let Ok(guard) = http_cache().read() {
        if let Some(entry) = guard.get(url) {
            if Instant::now() < entry.expires {
                return if entry.data.is_null() {
                    None
                } else {
                    Some(entry.data.clone())
                };
            }
        }
    }
    let timeout_secs = if is_github_url(url) { 10 } else { 6 };
    let agent = build_http_agent(url, timeout_secs);
    let mut req = agent
        .get(url)
        .header("User-Agent", "lain42-mirror-hook/1.0")
        .header("Accept", "application/json");
    if url.starts_with("https://api.github.com/") {
        req = req.header("X-GitHub-Api-Version", "2022-11-28");
        if let Ok(token) = std::env::var("GITHUB_TOKEN") {
            let trimmed = token.trim();
            if !trimmed.is_empty() {
                req = req.header("Authorization", &format!("Bearer {trimmed}"));
            }
        }
    }
    let val = req
        .call()
        .ok()
        .and_then(|mut resp| resp.body_mut().read_json::<Value>().ok());

    if let Ok(mut guard) = http_cache().write() {
        match &val {
            Some(v) => {
                guard.insert(
                    url.to_string(),
                    CachedValue {
                        expires: Instant::now() + CACHE_TTL,
                        data: v.clone(),
                    },
                );
            }
            None => {
                // Short negative cache (60s) to avoid thundering herd on transient upstream errors
                guard.insert(
                    url.to_string(),
                    CachedValue {
                        expires: Instant::now() + Duration::from_secs(60),
                        data: Value::Null,
                    },
                );
            }
        }
    }
    val
}


pub(super) fn fetch_text_cached(url: &str) -> Option<String> {
    if let Ok(guard) = http_cache().read() {
        if let Some(entry) = guard.get(url) {
            if Instant::now() < entry.expires {
                return entry.data.as_str().map(ToString::to_string);
            }
        }
    }
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(4)))
        .build()
        .new_agent();
    let result = agent
        .get(url)
        .header("User-Agent", "lain42-mirror-hook/1.0")
        .call()
        .ok()
        .and_then(|mut r| r.body_mut().read_to_string().ok());

    let cached_val = match &result {
        Some(s) => Value::String(s.clone()),
        None => Value::Null,
    };
    if let Ok(mut guard) = http_cache().write() {
        guard.insert(
            url.to_string(),
            CachedValue {
                expires: Instant::now() + CACHE_TTL,
                data: cached_val,
            },
        );
    }
    result
}


/// 无速率限制回退：通过 github.com/{repo}/releases/latest 302 Location + expanded_assets HTML 解析全部 Release 资产
fn fetch_github_release_via_web(repo: &str) -> Option<Value> {
    let latest_url = format!("https://github.com/{repo}/releases/latest");
    let mut cfg = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(10)))
        .max_redirects(0)
        .http_status_as_error(false);
    if let Some(proxy_str) = upstream_proxy_url() {
        if let Ok(proxy) = ureq::Proxy::new(&proxy_str) {
            cfg = cfg.proxy(Some(proxy));
        }
    }
    let no_redir_agent = cfg.build().new_agent();
    let resp = no_redir_agent
        .get(&latest_url)
        .header("User-Agent", "lain42-mirror-hook/1.0")
        .call()
        .ok()?;
    let loc = resp.headers().get("location")?.to_str().ok()?;
    let tag = loc.split("/tag/").nth(1)?.trim().to_string();
    if tag.is_empty() {
        return None;
    }

    let expanded_url = format!("https://github.com/{repo}/releases/expanded_assets/{tag}");
    let agent = build_http_agent(&expanded_url, 10);
    let html = agent
        .get(&expanded_url)
        .header("User-Agent", "lain42-mirror-hook/1.0")
        .call()
        .ok()?
        .body_mut()
        .read_to_string()
        .ok()?;

    let mut seen = HashSet::new();
    let mut assets = Vec::new();
    for part in html.split("href=\"").skip(1) {
        let Some(href) = part.split('"').next() else { continue };
        if !href.contains("/releases/download/") {
            continue;
        }
        let full_url = if href.starts_with("http") {
            href.to_string()
        } else {
            format!("https://github.com{href}")
        };
        if !seen.insert(full_url.clone()) {
            continue;
        }
        let Some(fname) = full_url.rsplit('/').next() else { continue };
        if fname.is_empty() {
            continue;
        }
        assets.push(json!({
            "name": fname,
            "browser_download_url": full_url,
            "size": 0u64,
        }));
    }

    Some(json!({
        "tag_name": tag,
        "html_url": format!("https://github.com/{repo}/releases/tag/{tag}"),
        "assets": assets,
    }))
}

/// 统一获取 GitHub 最新 Release（优先 REST API 带文件大小，限流或失败时自动无缝回退 expanded_assets 无速率限制通道）
pub(super) fn fetch_github_latest_release(repo: &str) -> Option<Value> {
    let cache_key = format!("gh_release:{repo}");
    if let Ok(guard) = http_cache().read() {
        if let Some(entry) = guard.get(&cache_key) {
            if Instant::now() < entry.expires {
                return if entry.data.is_null() {
                    None
                } else {
                    Some(entry.data.clone())
                };
            }
        }
    }

    let api_url = format!("https://api.github.com/repos/{repo}/releases/latest");
    let data = fetch_json_cached(&api_url)
        .filter(|v| v.get("assets").and_then(Value::as_array).is_some())
        .or_else(|| fetch_github_release_via_web(repo));

    if let Ok(mut guard) = http_cache().write() {
        match &data {
            Some(v) => {
                guard.insert(
                    cache_key,
                    CachedValue {
                        expires: Instant::now() + CACHE_TTL,
                        data: v.clone(),
                    },
                );
            }
            None => {
                guard.insert(
                    cache_key,
                    CachedValue {
                        expires: Instant::now() + Duration::from_secs(60),
                        data: Value::Null,
                    },
                );
            }
        }
    }
    data
}
