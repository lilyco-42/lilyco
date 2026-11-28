//! 国内镜像源智能重定向与最新版本解析引擎（参考 RubyMetric/chsrc）。
//!
//! 零本站存储原则：所有安装包均直连国内官方开源镜像站（阿里云 / 清华 TUNA / 中科大 USTC /
//! 北外 BFSU / 腾讯云 / 字节跳动 rsproxy / F-Droid 手机镜像 / ghfast）、夸克网盘或 Cloudflare Wrangler。

use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};
use std::time::{Duration, Instant};

use axum::http::HeaderMap;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::db::DownloadItem;

const CACHE_TTL: Duration = Duration::from_secs(1800);

#[derive(Debug, Clone, Serialize)]
pub struct ClientPlatform {
    pub os: String,   // win | mac | linux | android | ios
    pub arch: String, // x64 | arm64
}

#[derive(Debug, Clone, Serialize)]
pub struct MirrorResolution {
    pub slug: String,
    pub mirror: String,
    pub mirror_label: String,
    pub os: String,
    pub arch: String,
    pub version: String,
    pub asset_name: String,
    pub target_url: String,
    pub official_url: String,
    pub quark_url: String,
    pub chsrc_cmd: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct MirrorLink {
    pub id: String,
    pub label: String,
    pub url: String,
    pub recommended: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResolvedAsset {
    pub name: String,
    pub os: String,
    pub arch: String,
    pub ext: String,
    pub size_bytes: u64,
    pub is_auxiliary: bool,
    pub mirrors: Vec<MirrorLink>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReleaseAssetsResponse {
    pub slug: String,
    pub name: String,
    pub repo: String,
    pub version: String,
    pub release_url: String,
    pub quark_url: String,
    pub source: String,
    pub assets: Vec<ResolvedAsset>,
}

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

fn fetch_json_cached(url: &str) -> Option<Value> {
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

/// 自动识别客户端操作系统与 CPU 架构（优先显式参数，回退 User-Agent / Client Hints）
pub fn detect_platform(
    headers: &HeaderMap,
    q_os: Option<&str>,
    q_arch: Option<&str>,
) -> ClientPlatform {
    let ua = headers
        .get("user-agent")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    let ch_plat = headers
        .get("sec-ch-ua-platform")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    let ch_arch = headers
        .get("sec-ch-ua-arch")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();

    let os = match q_os.unwrap_or("auto") {
        "win" | "windows" => "win",
        "mac" | "macos" | "darwin" => "mac",
        "linux" => "linux",
        "android" => "android",
        "ios" => "ios",
        _ => {
            if ua.contains("android") || ch_plat.contains("android") {
                "android"
            } else if ua.contains("iphone") || ua.contains("ipad") {
                "ios"
            } else if ua.contains("windows") || ch_plat.contains("windows") {
                "win"
            } else if ua.contains("mac os") || ua.contains("macintosh") || ch_plat.contains("mac") {
                "mac"
            } else {
                "linux"
            }
        }
    };

    let arch = match q_arch.unwrap_or("auto") {
        "arm64" | "aarch64" => "arm64",
        "x64" | "amd64" | "x86_64" => "x64",
        _ => {
            if os == "android"
                || os == "ios"
                || ua.contains("arm64")
                || ua.contains("aarch64")
                || ch_arch.contains("arm")
            {
                "arm64"
            } else {
                "x64"
            }
        }
    };

    ClientPlatform {
        os: os.to_string(),
        arch: arch.to_string(),
    }
}

/// 全部支持的国内镜像源注册表（桌面端 + 手机端 + chsrc 一键换源）
pub fn supported_mirrors_registry() -> Value {
    json!({
        "storage_policy": "零本站存储：安装包全部直连国内官方镜像站、夸克网盘（kuake cli）或 Cloudflare Wrangler（dl.lilyco42.top）",
        "chsrc": {
            "repo": "https://github.com/RubyMetric/chsrc",
            "linux_macos": "curl https://chsrc.run/posix | bash",
            "windows_ps": "irm https://chsrc.run/windows | iex",
            "examples": [
                "chsrc set nodejs",
                "chsrc set python",
                "chsrc set rust",
                "chsrc set go",
                "chsrc set pip",
                "chsrc set ubuntu"
            ]
        },
        "mirrors": [
            { "id": "aliyun", "name": "阿里云 / npmmirror", "scope": "桌面 / 开发环境", "url": "https://registry.npmmirror.com/-/binary/", "supports": ["Node.js", "Python", "Git for Windows", "Go", "Electron", "PyPI"] },
            { "id": "tuna", "name": "清华大学 TUNA 镜像站", "scope": "校园网 / 全平台 / Android", "url": "https://mirrors.tuna.tsinghua.edu.cn/", "supports": ["Node.js", "GitHub Release", "F-Droid 手机源", "Rustup", "Anaconda", "Homebrew"] },
            { "id": "ustc", "name": "中科大 USTC 镜像站", "scope": "校园网 / 全平台 / Android", "url": "https://mirrors.ustc.edu.cn/", "supports": ["F-Droid 手机源", "Rust", "Debian/Ubuntu", "Homebrew"] },
            { "id": "bfsu", "name": "北外 BFSU 镜像站", "scope": "校园网 / 全平台 / Android", "url": "https://mirrors.bfsu.edu.cn/", "supports": ["F-Droid 手机源", "Node.js", "GitHub Release"] },
            { "id": "tencent", "name": "腾讯云软件源", "scope": "桌面 / 云主机", "url": "https://mirrors.cloud.tencent.com/", "supports": ["Node.js", "Python", "Go", "PyPI", "Docker"] },
            { "id": "bytedance", "name": "字节跳动 / rsproxy", "scope": "Rust / 火山引擎", "url": "https://rsproxy.cn/", "supports": ["Rustup", "Cargo Crates", "Volcengine Mirrors"] },
            { "id": "fdroid", "name": "F-Droid 国内开源安卓商店镜像", "scope": "Android 手机端", "url": "https://mirrors.tuna.tsinghua.edu.cn/fdroid/repo", "supports": ["F-Droid", "Termux", "NewPipe", "LocalSend", "Android 开源 APK"] },
            { "id": "ghfast", "name": "GitHub Release 国内加速代理", "scope": "全站 1000+ GitHub 项目", "url": "https://ghfast.top/", "supports": ["OBS Studio", "SnowShot", "Umi-OCR", "Clash Verge Rev", "10k+ 星 GitHub Releases"] },
            { "id": "quark", "name": "夸克网盘高速镜像（kuake cli）", "scope": "国内免代理满速", "url": "https://pan.quark.cn/", "supports": ["云枢自研全家桶", "Clash Verge Rev", "精选软件打包"] },
            { "id": "wrangler", "name": "Cloudflare Wrangler (dl.lilyco42.top)", "scope": "全球边缘 CDN 直链", "url": "https://dl.lilyco42.top/", "supports": ["Clash Verge Rev", "高频直链分发"] }
        ]
    })
}

/// 解析 Node.js 最新版本（实时查阿里云 npmmirror，自动匹配 Windows .msi / macOS .pkg / Linux .tar.xz）
fn resolve_nodejs(branch: &str, mirror: &str, plat: &ClientPlatform) -> Option<(String, String, String, String)> {
    let api_url = format!("https://registry.npmmirror.com/-/binary/node/{branch}/");
    let list = fetch_json_cached(&api_url)?;
    let arr = list.as_array()?;
    let suffix = match (plat.os.as_str(), plat.arch.as_str()) {
        ("win", "arm64") => "-arm64.msi",
        ("win", _) => "-x64.msi",
        ("mac", _) => ".pkg",
        (_, "arm64") => "-linux-arm64.tar.xz",
        _ => "-linux-x64.tar.xz",
    };
    let item = arr.iter().rfind(|x| {
        x.get("name")
            .and_then(Value::as_str)
            .is_some_and(|n| n.starts_with("node-v") && n.ends_with(suffix))
    })?;
    let fname = item.get("name")?.as_str()?.to_string();
    let version = fname
        .strip_prefix("node-")
        .and_then(|s| s.split('-').next())
        .unwrap_or(branch)
        .trim_end_matches(".pkg")
        .to_string();
    let official = format!("https://nodejs.org/dist/{branch}/{fname}");
    let (target, label) = match mirror {
        "tuna" => (
            format!("https://mirrors.tuna.tsinghua.edu.cn/nodejs-release/{branch}/{fname}"),
            "清华大学 TUNA 镜像站",
        ),
        "bfsu" => (
            format!("https://mirrors.bfsu.edu.cn/nodejs-release/{branch}/{fname}"),
            "北外 BFSU 镜像站",
        ),
        "nju" => (
            format!("https://mirrors.nju.edu.cn/nodejs-release/{branch}/{fname}"),
            "南京大学 NJU 镜像站",
        ),
        "ustc" => (
            format!("https://mirrors.ustc.edu.cn/node/{branch}/{fname}"),
            "中科大 USTC 镜像站",
        ),
        "tencent" => (
            format!("https://mirrors.cloud.tencent.com/nodejs-release/{branch}/{fname}"),
            "腾讯云软件源",
        ),
        "official" => (official.clone(), "Node.js 官方源"),
        _ => (
            format!("https://registry.npmmirror.com/-/binary/node/{branch}/{fname}"),
            "阿里云 npmmirror 镜像",
        ),
    };
    Some((version, fname, target, format!("{label}|{official}")))
}

fn fetch_text_cached(url: &str) -> Option<String> {
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct AssetClassification {
    is_ignored: bool,
    os: &'static str,
    arch: &'static str,
    ext: String,
    is_auxiliary: bool,
}

fn detect_asset_ext(low: &str) -> String {
    for ext in [
        ".tar.gz",
        ".tar.xz",
        ".tar.bz2",
        ".tar.zst",
        ".pkg.tar.zst",
        ".appimage",
        ".exe",
        ".msi",
        ".msix",
        ".dmg",
        ".pkg",
        ".deb",
        ".rpm",
        ".apk",
        ".aab",
        ".ipa",
        ".zip",
        ".7z",
        ".flatpak",
        ".snap",
    ] {
        if low.ends_with(ext) {
            return if ext == ".appimage" {
                ".AppImage".to_string()
            } else {
                ext.to_string()
            };
        }
    }
    String::new()
}

fn classify_asset(name: &str) -> AssetClassification {
    let skip = [
        ".sig", ".asc", ".sha256", ".sha512", ".sha1", ".md5", ".blockmap",
        ".yml", ".yaml", ".txt", ".json", ".jsonl", ".md", ".rst", ".xml",
        ".license", ".sbom", ".spdx", ".zsync", ".ddeb", ".pdb", ".minisig",
        ".provenance", ".intoto", ".pem", ".pub", "checksums", "sources.tar",
        "source.tar",
    ];
    let low = name.to_ascii_lowercase();
    let is_ignored = skip.iter().any(|s| low.contains(s)) || low.ends_with('/');
    let ext = detect_asset_ext(&low);

    let os = if low.ends_with(".exe")
        || low.ends_with(".msi")
        || low.ends_with(".msix")
        || low.ends_with(".appx")
    {
        "win"
    } else if low.ends_with(".dmg") || low.ends_with(".pkg") {
        "mac"
    } else if low.ends_with(".apk") || low.ends_with(".aab") {
        "android"
    } else if low.ends_with(".ipa") {
        "ios"
    } else if low.ends_with(".appimage")
        || low.ends_with(".deb")
        || low.ends_with(".rpm")
        || low.ends_with(".flatpak")
        || low.ends_with(".snap")
        || low.ends_with(".pkg.tar.zst")
    {
        "linux"
    } else if low.contains("windows")
        || low.contains("win64")
        || low.contains("win32")
        || low.contains("-win")
        || low.contains("_win")
        || low.contains(".win")
        || low.contains("pc-windows")
        || low.contains("msvc")
        || low.contains("mingw")
    {
        "win"
    } else if low.contains("darwin")
        || low.contains("macos")
        || low.contains("osx")
        || low.contains("apple-")
        || low.contains("-mac")
        || low.contains("_mac")
        || low.contains(".mac")
    {
        "mac"
    } else if low.contains("android") {
        "android"
    } else if low.contains("ios") || low.contains("iphone") || low.contains("ipad") {
        "ios"
    } else if low.contains("linux")
        || low.contains("ubuntu")
        || low.contains("debian")
        || low.contains("fedora")
        || low.contains("musl")
        || low.contains("unknown-linux")
    {
        "linux"
    } else {
        "other"
    };

    let arch = if low.contains("universal") || low.contains("-fat") {
        "universal"
    } else if low.contains("arm64")
        || low.contains("aarch64")
        || low.contains("apple-silicon")
        || low.contains("arm64-v8a")
    {
        "arm64"
    } else if low.contains("x86_64")
        || low.contains("x64")
        || low.contains("amd64")
        || low.contains("win64")
        || low.contains("64-bit")
        || low.contains("64bit")
        || low.contains("intel")
    {
        "x64"
    } else if (low.contains("x86") && !low.contains("x86_64"))
        || low.contains("i386")
        || low.contains("i686")
        || low.contains("win32")
        || low.contains("ia32")
        || low.contains("32-bit")
        || low.contains("armeabi")
    {
        "x86"
    } else {
        "universal"
    };

    let auxiliary_keywords = [
        "remote-server",
        "remote_server",
        "bwrap",
        "debug",
        "symbols",
        "dsym",
        "headless",
        "chromedriver",
        "geckodriver",
        "sandbox",
    ];
    let is_auxiliary = auxiliary_keywords.iter().any(|k| low.contains(k));

    AssetClassification {
        is_ignored,
        os,
        arch,
        ext,
        is_auxiliary,
    }
}

fn score_asset_name(name: &str, plat: &ClientPlatform) -> i32 {
    let cls = classify_asset(name);
    if cls.is_ignored {
        return -1000;
    }
    let low = name.to_ascii_lowercase();
    let mut score = 0i32;

    if cls.os == plat.os {
        score += 60;
    } else if cls.os != "other" {
        score -= 120;
    }

    match plat.os.as_str() {
        "win" => {
            if low.ends_with(".exe") {
                score += 40;
            } else if low.ends_with(".msi") {
                score += 38;
            } else if low.ends_with(".msix") || low.ends_with(".appx") {
                score += 30;
            } else if low.ends_with(".zip") || low.ends_with(".7z") {
                score += 15;
            }
            if low.contains("setup") || low.contains("installer") {
                score += 5;
            }
        }
        "mac" => {
            if low.ends_with(".dmg") {
                score += 40;
            } else if low.ends_with(".pkg") {
                score += 38;
            } else if low.ends_with(".zip") || low.ends_with(".tar.gz") {
                score += 15;
            }
        }
        "android" => {
            if low.ends_with(".apk") {
                score += 50;
            }
        }
        _ => {
            if low.ends_with(".appimage") {
                score += 40;
            } else if low.ends_with(".deb") {
                score += 35;
            } else if low.ends_with(".tar.gz") || low.ends_with(".tar.xz") {
                score += 30;
            } else if low.ends_with(".rpm") {
                score += 25;
            }
        }
    }

    if cls.arch == plat.arch {
        score += 35;
    } else if cls.arch == "universal" {
        score += 15;
    } else {
        score -= 50;
    }

    if cls.is_auxiliary {
        score -= 80;
    }

    score
}

/// 优先查询清华 TUNA / 南京大学 NJU / 北外 BFSU 高校官方 github-release 同步镜像（真 10~50 MB/s 不限速直连）
fn resolve_university_github_release(
    repo: &str,
    preferred_mirror: &str,
    plat: &ClientPlatform,
) -> Option<(String, String, String)> {
    let hosts: Vec<(&str, &str)> = match preferred_mirror {
        "nju" => vec![
            ("https://mirrors.nju.edu.cn", "南京大学 NJU 真高速镜像"),
            ("https://mirrors.tuna.tsinghua.edu.cn", "清华大学 TUNA 真高速镜像"),
            ("https://mirrors.bfsu.edu.cn", "北外 BFSU 真高速镜像"),
        ],
        "bfsu" => vec![
            ("https://mirrors.bfsu.edu.cn", "北外 BFSU 真高速镜像"),
            ("https://mirrors.tuna.tsinghua.edu.cn", "清华大学 TUNA 真高速镜像"),
            ("https://mirrors.nju.edu.cn", "南京大学 NJU 真高速镜像"),
        ],
        _ => vec![
            ("https://mirrors.tuna.tsinghua.edu.cn", "清华大学 TUNA 真高速镜像"),
            ("https://mirrors.nju.edu.cn", "南京大学 NJU 真高速镜像"),
            ("https://mirrors.bfsu.edu.cn", "北外 BFSU 真高速镜像"),
        ],
    };

    for (base, label) in hosts {
        let dir_url = format!("{base}/github-release/{repo}/LatestRelease/");
        let Some(html) = fetch_text_cached(&dir_url) else {
            continue;
        };
        let mut best_score = 0i32;
        let mut best_file: Option<String> = None;
        for part in html.split("href=\"").skip(1) {
            let Some(href) = part.split('"').next() else { continue };
            if href.starts_with("http") || href.starts_with('/') || href.starts_with('?') || href.starts_with("..") {
                continue;
            }
            let sc = score_asset_name(href, plat);
            if sc > best_score {
                best_score = sc;
                best_file = Some(href.to_string());
            }
        }
        if let Some(fname) = best_file {
            let dl = format!("{dir_url}{fname}");
            return Some((fname, dl, label.to_string()));
        }
    }
    None
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

    let mut seen = std::collections::HashSet::new();
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
fn fetch_github_latest_release(repo: &str) -> Option<Value> {
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

/// 解析 GitHub 最新 Release 并按客户端 OS/Arch 匹配最合适安装包
fn resolve_github_latest(repo: &str, plat: &ClientPlatform) -> Option<(String, String, String)> {
    let data = fetch_github_latest_release(repo)?;
    let tag = data.get("tag_name").and_then(Value::as_str)?.to_string();
    let assets = data.get("assets").and_then(Value::as_array)?;

    let mut best_score = 0i32;
    let mut best: Option<(String, String)> = None;
    for a in assets {
        let Some(name) = a.get("name").and_then(Value::as_str) else { continue };
        let Some(dl) = a.get("browser_download_url").and_then(Value::as_str) else { continue };
        let score = score_asset_name(name, plat);
        if score > best_score {
            best_score = score;
            best = Some((name.to_string(), dl.to_string()));
        }
    }
    let (fname, dl) = best?;
    Some((tag, fname, dl))
}

fn build_asset_mirrors(raw_dl: &str, primary_proxy: &'static str) -> Vec<MirrorLink> {
    let is_github_binary = raw_dl.starts_with("https://github.com/")
        && (raw_dl.contains("/releases/download/") || raw_dl.contains("/archive/"));
    let mut links = Vec::new();
    if is_github_binary {
        let backup_proxy = if primary_proxy == "gh-proxy.org" {
            "ghfast.top"
        } else {
            "gh-proxy.org"
        };
        links.push(MirrorLink {
            id: primary_proxy.to_string(),
            label: format!("国内加速 ({primary_proxy})"),
            url: format!("https://{primary_proxy}/{raw_dl}"),
            recommended: true,
        });
        links.push(MirrorLink {
            id: backup_proxy.to_string(),
            label: format!("备用加速 ({backup_proxy})"),
            url: format!("https://{backup_proxy}/{raw_dl}"),
            recommended: false,
        });
        links.push(MirrorLink {
            id: "official".to_string(),
            label: "GitHub 官方直链".to_string(),
            url: raw_dl.to_string(),
            recommended: false,
        });
    } else if !raw_dl.is_empty() {
        links.push(MirrorLink {
            id: "official".to_string(),
            label: "官方直链".to_string(),
            url: raw_dl.to_string(),
            recommended: true,
        });
    }
    links
}

fn os_sort_rank(os: &str) -> u8 {
    match os {
        "win" => 0,
        "mac" => 1,
        "linux" => 2,
        "android" => 3,
        "ios" => 4,
        _ => 5,
    }
}

fn arch_sort_rank(arch: &str) -> u8 {
    match arch {
        "x64" => 0,
        "arm64" => 1,
        "universal" => 2,
        "x86" => 3,
        _ => 4,
    }
}

fn ext_sort_rank(ext: &str) -> u8 {
    match ext {
        ".exe" => 0,
        ".msi" => 1,
        ".dmg" => 2,
        ".pkg" => 3,
        ".AppImage" => 4,
        ".deb" => 5,
        ".apk" => 6,
        ".tar.gz" | ".tar.xz" => 7,
        ".zip" | ".7z" => 8,
        _ => 9,
    }
}

/// 获取某个软件最新 Release 的全部平台/架构安装包与多渠道下载链接
pub fn resolve_release_assets(
    item: &DownloadItem,
    ghproxy_raw: Option<&str>,
) -> ReleaseAssetsResponse {
    let primary_proxy = sanitize_ghproxy(ghproxy_raw);
    let mut version = item.version.clone();
    let mut release_url = item.github_release_url.clone();
    let mut assets: Vec<ResolvedAsset> = Vec::new();
    let mut source = "catalog_fallback".to_string();

    if item.repo.contains('/') && !item.repo.starts_with("http") {
        if let Some(data) = fetch_github_latest_release(&item.repo) {
            if let Some(tag) = data.get("tag_name").and_then(Value::as_str) {
                version = tag.to_string();
            }
            if let Some(html_url) = data.get("html_url").and_then(Value::as_str) {
                release_url = html_url.to_string();
            }
            if let Some(raw_assets) = data.get("assets").and_then(Value::as_array) {
                source = "github_live".to_string();
                for a in raw_assets {
                    let Some(name) = a.get("name").and_then(Value::as_str) else { continue };
                    let Some(dl) = a.get("browser_download_url").and_then(Value::as_str) else { continue };
                    let cls = classify_asset(name);
                    if cls.is_ignored {
                        continue;
                    }
                    let size_bytes = a.get("size").and_then(Value::as_u64).unwrap_or(0);
                    assets.push(ResolvedAsset {
                        name: name.to_string(),
                        os: cls.os.to_string(),
                        arch: cls.arch.to_string(),
                        ext: cls.ext,
                        size_bytes,
                        is_auxiliary: cls.is_auxiliary,
                        mirrors: build_asset_mirrors(dl, primary_proxy),
                    });
                }
            }
        }
    }

    // Fallback: if live GitHub API unavailable or returned 0 assets, surface catalog item's stored binary URL
    if assets.is_empty() && !item.github_download_url.is_empty() {
        let fname = if !item.asset_name.is_empty() {
            item.asset_name.clone()
        } else {
            item.github_download_url
                .rsplit('/')
                .next()
                .unwrap_or(&item.name)
                .to_string()
        };
        let cls = classify_asset(&fname);
        if !cls.is_ignored {
            assets.push(ResolvedAsset {
                name: fname,
                os: cls.os.to_string(),
                arch: cls.arch.to_string(),
                ext: cls.ext,
                size_bytes: item.asset_size_bytes.max(0) as u64,
                is_auxiliary: cls.is_auxiliary,
                mirrors: build_asset_mirrors(&item.github_download_url, primary_proxy),
            });
        }
    }

    assets.sort_by(|a, b| {
        a.is_auxiliary
            .cmp(&b.is_auxiliary)
            .then_with(|| os_sort_rank(&a.os).cmp(&os_sort_rank(&b.os)))
            .then_with(|| arch_sort_rank(&a.arch).cmp(&arch_sort_rank(&b.arch)))
            .then_with(|| ext_sort_rank(&a.ext).cmp(&ext_sort_rank(&b.ext)))
            .then_with(|| a.name.cmp(&b.name))
    });

    ReleaseAssetsResponse {
        slug: item.slug.clone(),
        name: item.name.clone(),
        repo: item.repo.clone(),
        version,
        release_url,
        quark_url: item.quark_url.clone(),
        source,
        assets,
    }
}

#[derive(Debug, Deserialize)]
pub struct MirrorQuery {
    pub mirror: Option<String>,
    pub ghproxy: Option<String>,
    pub os: Option<String>,
    pub arch: Option<String>,
}

fn sanitize_ghproxy(raw: Option<&str>) -> &'static str {
    match raw.unwrap_or("ghfast.top").trim() {
        "gh-proxy.org" => "gh-proxy.org",
        "cdn.gh-proxy.org" => "cdn.gh-proxy.org",
        _ => "ghfast.top",
    }
}

/// 主入口：根据 slug、镜像源选择、客户端 OS/Arch 计算最终最新高速重定向 URL
pub fn resolve_download_mirror(
    item: &DownloadItem,
    mirror_raw: &str,
    ghproxy_raw: Option<&str>,
    plat: &ClientPlatform,
) -> MirrorResolution {
    let mirror = if mirror_raw.is_empty() { "auto" } else { mirror_raw };
    let slug = item.slug.as_str();

    // 1. 特殊官方运行环境 Hook（Node.js / Rust / Go / Python / Git / F-Droid）
    if slug == "nodejs" || slug == "nodejs-v24" || slug == "nodejs-lts" {
        let branch = if slug == "nodejs-lts" { "latest-v22.x" } else { "latest-v24.x" };
        if let Some((ver, fname, target, meta)) = resolve_nodejs(branch, mirror, plat) {
            let mut parts = meta.splitn(2, '|');
            let label = parts.next().unwrap_or("阿里云 npmmirror").to_string();
            let official = parts.next().unwrap_or(&item.github_download_url).to_string();
            return MirrorResolution {
                slug: item.slug.clone(),
                mirror: mirror.to_string(),
                mirror_label: label,
                os: plat.os.clone(),
                arch: plat.arch.clone(),
                version: ver,
                asset_name: fname,
                target_url: target,
                official_url: official,
                quark_url: item.quark_url.clone(),
                chsrc_cmd: "chsrc set nodejs".to_string(),
            };
        }
    }

    if slug == "rustup" {
        let (target, label) = match mirror {
            "tuna" => ("https://mirrors.tuna.tsinghua.edu.cn/rustup/".to_string(), "清华大学 TUNA Rustup"),
            "ustc" => ("https://mirrors.ustc.edu.cn/rust-static/".to_string(), "中科大 USTC Rust"),
            "official" => ("https://sh.rustup.rs".to_string(), "Rust 官方源"),
            _ => (
                if plat.os == "win" {
                    "https://rsproxy.cn/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe".to_string()
                } else {
                    "https://rsproxy.cn/rustup-init.sh".to_string()
                },
                "字节跳动 rsproxy.cn 镜像",
            ),
        };
        return MirrorResolution {
            slug: item.slug.clone(),
            mirror: mirror.to_string(),
            mirror_label: label.to_string(),
            os: plat.os.clone(),
            arch: plat.arch.clone(),
            version: item.version.clone(),
            asset_name: if plat.os == "win" { "rustup-init.exe".into() } else { "rustup-init.sh".into() },
            target_url: target,
            official_url: "https://rustup.rs/".to_string(),
            quark_url: item.quark_url.clone(),
            chsrc_cmd: "chsrc set rust".to_string(),
        };
    }

    if slug == "golang" {
        let (target, label) = match mirror {
            "tencent" => ("https://mirrors.cloud.tencent.com/golang/".to_string(), "腾讯云 Go 镜像"),
            "official" => ("https://go.dev/dl/".to_string(), "Go 官方下载"),
            _ => ("https://mirrors.aliyun.com/golang/".to_string(), "阿里云 Go 镜像站"),
        };
        return MirrorResolution {
            slug: item.slug.clone(),
            mirror: mirror.to_string(),
            mirror_label: label.to_string(),
            os: plat.os.clone(),
            arch: plat.arch.clone(),
            version: item.version.clone(),
            asset_name: item.asset_name.clone(),
            target_url: target,
            official_url: "https://go.dev/dl/".to_string(),
            quark_url: item.quark_url.clone(),
            chsrc_cmd: "chsrc set go".to_string(),
        };
    }

    if slug == "python" {
        let (target, label) = match mirror {
            "tencent" => ("https://mirrors.cloud.tencent.com/python/".to_string(), "腾讯云 Python 镜像"),
            "official" => ("https://www.python.org/downloads/".to_string(), "Python 官方源"),
            _ => ("https://registry.npmmirror.com/-/binary/python/".to_string(), "阿里云 npmmirror Python"),
        };
        return MirrorResolution {
            slug: item.slug.clone(),
            mirror: mirror.to_string(),
            mirror_label: label.to_string(),
            os: plat.os.clone(),
            arch: plat.arch.clone(),
            version: item.version.clone(),
            asset_name: item.asset_name.clone(),
            target_url: target,
            official_url: "https://www.python.org/downloads/".to_string(),
            quark_url: item.quark_url.clone(),
            chsrc_cmd: "chsrc set python".to_string(),
        };
    }

    if slug == "f-droid" {
        let (target, label) = match mirror {
            "ustc" => ("https://mirrors.ustc.edu.cn/help/fdroid.html".to_string(), "中科大 USTC F-Droid 镜像指南"),
            "bfsu" => ("https://mirrors.bfsu.edu.cn/help/fdroid/".to_string(), "北外 BFSU F-Droid 镜像指南"),
            "tuna" => ("https://mirrors.tuna.tsinghua.edu.cn/help/fdroid/".to_string(), "清华大学 TUNA F-Droid 镜像指南"),
            _ => ("https://f-droid.org/F-Droid.apk".to_string(), "F-Droid 官方最新版 APK"),
        };
        return MirrorResolution {
            slug: item.slug.clone(),
            mirror: mirror.to_string(),
            mirror_label: label.to_string(),
            os: "android".to_string(),
            arch: "arm64".to_string(),
            version: item.version.clone(),
            asset_name: "F-Droid.apk".to_string(),
            target_url: target,
            official_url: "https://f-droid.org/F-Droid.apk".to_string(),
            quark_url: item.quark_url.clone(),
            chsrc_cmd: "F-Droid 镜像地址: https://mirrors.tuna.tsinghua.edu.cn/fdroid/repo".to_string(),
        };
    }

    // 2. 仅当显式选择 mirror=quark 时才返回夸克网盘，不再劫持镜像站/直链请求
    if mirror == "quark" && !item.quark_url.is_empty() {
        return MirrorResolution {
            slug: item.slug.clone(),
            mirror: "quark".to_string(),
            mirror_label: "夸克网盘高速镜像".to_string(),
            os: plat.os.clone(),
            arch: plat.arch.clone(),
            version: item.version.clone(),
            asset_name: item.asset_name.clone(),
            target_url: item.quark_url.clone(),
            official_url: item.github_download_url.clone(),
            quark_url: item.quark_url.clone(),
            chsrc_cmd: String::new(),
        };
    }

    // 2.5 高校真高速 github-release 镜像（清华 TUNA / 南京大学 NJU / 北外 BFSU — 10~50 MB/s 不限速！）
    if mirror != "official"
        && mirror != "ghfast"
        && item.repo.contains('/')
        && !item.repo.starts_with("http")
    {
        if let Some((fname, dl_url, label)) =
            resolve_university_github_release(&item.repo, mirror, plat)
        {
            return MirrorResolution {
                slug: item.slug.clone(),
                mirror: mirror.to_string(),
                mirror_label: label,
                os: plat.os.clone(),
                arch: plat.arch.clone(),
                version: item.version.clone(),
                asset_name: fname,
                target_url: dl_url,
                official_url: item.github_download_url.clone(),
                quark_url: item.quark_url.clone(),
                chsrc_cmd: String::new(),
            };
        }
    }

    // 3. 通用 GitHub Release 实时最新版本解析 + OS/Arch 资产匹配 + ghfast 国内加速 Hook
    let (version, asset_name, raw_dl) = if item.repo.contains('/') && !item.repo.starts_with("http") {
        resolve_github_latest(&item.repo, plat).unwrap_or_else(|| {
            (
                item.version.clone(),
                item.asset_name.clone(),
                if item.github_download_url.is_empty() {
                    item.github_release_url.clone()
                } else {
                    item.github_download_url.clone()
                },
            )
        })
    } else {
        (
            item.version.clone(),
            item.asset_name.clone(),
            item.github_download_url.clone(),
        )
    };

    let proxy_host = sanitize_ghproxy(ghproxy_raw);
    let is_github_binary = raw_dl.starts_with("https://github.com/")
        && (raw_dl.contains("/releases/download/") || raw_dl.contains("/archive/"));

    // 如果项目没有二进制 Release 附件（如 n8n / 纯源码项目），自动构造加速源码压缩包直链，避免跳回被墙的 github.com HTML 页
    let fallback_archive = if !is_github_binary && item.repo.contains('/') && !version.is_empty() && version != "latest" {
        format!("https://{proxy_host}/https://github.com/{}/archive/refs/tags/{version}.tar.gz", item.repo)
    } else {
        String::new()
    };

    let (target_url, mirror_label) = if mirror == "official" {
        (raw_dl.clone(), "GitHub 官方直链".to_string())
    } else if is_github_binary {
        (
            format!("https://{proxy_host}/{raw_dl}"),
            format!("GitHub 国内加速 ({proxy_host})"),
        )
    } else if !item.quark_url.is_empty() {
        (item.quark_url.clone(), "夸克网盘高速镜像".to_string())
    } else if !fallback_archive.is_empty() {
        (fallback_archive, format!("源码包国内加速 ({proxy_host})"))
    } else {
        (raw_dl.clone(), "GitHub Release 页面".to_string())
    };

    let chsrc_cmd = if slug == "chsrc" {
        if plat.os == "win" {
            "irm https://chsrc.run/windows | iex".to_string()
        } else {
            "curl https://chsrc.run/posix | bash".to_string()
        }
    } else {
        String::new()
    };

    MirrorResolution {
        slug: item.slug.clone(),
        mirror: mirror.to_string(),
        mirror_label: mirror_label.to_string(),
        os: plat.os.clone(),
        arch: plat.arch.clone(),
        version,
        asset_name,
        target_url,
        official_url: raw_dl,
        quark_url: item.quark_url.clone(),
        chsrc_cmd,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ZED_ASSETS: &[&str] = &[
        "Zed-aarch64.dmg",
        "Zed-aarch64.exe",
        "Zed-x86_64.dmg",
        "Zed-x86_64.exe",
        "bwrap-aarch64-linux-gnu.tar.gz",
        "bwrap-x86_64-linux-gnu.tar.gz",
        "zed-linux-aarch64.tar.gz",
        "zed-linux-x86_64.tar.gz",
        "zed-remote-server-linux-aarch64.gz",
        "zed-remote-server-linux-x86_64.gz",
        "zed-remote-server-macos-aarch64.gz",
        "zed-remote-server-macos-x86_64.gz",
        "zed-remote-server-windows-aarch64.zip",
        "zed-remote-server-windows-x86_64.zip",
    ];

    fn pick_best<'a>(assets: &[&'a str], os: &str, arch: &str) -> Option<&'a str> {
        let plat = ClientPlatform {
            os: os.to_string(),
            arch: arch.to_string(),
        };
        let mut best_score = 0i32;
        let mut best = None;
        for &a in assets {
            let sc = score_asset_name(a, &plat);
            if sc > best_score {
                best_score = sc;
                best = Some(a);
            }
        }
        best
    }

    #[test]
    fn zed_picks_exact_platform_installer() {
        assert_eq!(pick_best(ZED_ASSETS, "win", "x64"), Some("Zed-x86_64.exe"));
        assert_eq!(pick_best(ZED_ASSETS, "win", "arm64"), Some("Zed-aarch64.exe"));
        assert_eq!(pick_best(ZED_ASSETS, "mac", "arm64"), Some("Zed-aarch64.dmg"));
        assert_eq!(pick_best(ZED_ASSETS, "mac", "x64"), Some("Zed-x86_64.dmg"));
        assert_eq!(pick_best(ZED_ASSETS, "linux", "x64"), Some("zed-linux-x86_64.tar.gz"));
        assert_eq!(pick_best(ZED_ASSETS, "linux", "arm64"), Some("zed-linux-aarch64.tar.gz"));
        // Android has no Zed binary; must NOT return a Windows/Linux/macOS binary
        assert_eq!(pick_best(ZED_ASSETS, "android", "arm64"), None);
    }

    #[test]
    fn auxiliary_binaries_are_flagged_and_penalized() {
        let cls_srv = classify_asset("zed-remote-server-windows-x86_64.zip");
        assert!(cls_srv.is_auxiliary);
        assert_eq!(cls_srv.os, "win");
        assert_eq!(cls_srv.arch, "x64");

        let cls_bwrap = classify_asset("bwrap-x86_64-linux-gnu.tar.gz");
        assert!(cls_bwrap.is_auxiliary);

        let cls_exe = classify_asset("Zed-x86_64.exe");
        assert!(!cls_exe.is_auxiliary);
        assert_eq!(cls_exe.os, "win");
        assert_eq!(cls_exe.arch, "x64");
        assert_eq!(cls_exe.ext, ".exe");
    }

    #[test]
    fn build_asset_mirrors_produces_multi_channel_links() {
        let links = build_asset_mirrors(
            "https://github.com/zed-industries/zed/releases/download/v1.30.2/Zed-x86_64.exe",
            "ghfast.top",
        );
        assert_eq!(links.len(), 3);
        assert_eq!(links[0].id, "ghfast.top");
        assert!(links[0].recommended);
        assert!(links[0].url.starts_with("https://ghfast.top/https://github.com/"));
        assert_eq!(links[1].id, "gh-proxy.org");
        assert_eq!(links[2].id, "official");
    }
}
