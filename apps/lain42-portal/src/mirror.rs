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

#[derive(Clone)]
struct CachedValue {
    expires: Instant,
    data: Value,
}

fn http_cache() -> &'static RwLock<HashMap<String, CachedValue>> {
    static CACHE: OnceLock<RwLock<HashMap<String, CachedValue>>> = OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(HashMap::new()))
}

fn fetch_json_cached(url: &str) -> Option<Value> {
    if let Ok(guard) = http_cache().read() {
        if let Some(entry) = guard.get(url) {
            if Instant::now() < entry.expires {
                return Some(entry.data.clone());
            }
        }
    }
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(6)))
        .build()
        .new_agent();
    let mut resp = agent
        .get(url)
        .header("User-Agent", "lain42-mirror-hook/1.0")
        .header("Accept", "application/json")
        .call()
        .ok()?;
    let val: Value = resp.body_mut().read_json().ok()?;
    if let Ok(mut guard) = http_cache().write() {
        guard.insert(
            url.to_string(),
            CachedValue {
                expires: Instant::now() + CACHE_TTL,
                data: val.clone(),
            },
        );
    }
    Some(val)
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

fn score_asset_name(name: &str, plat: &ClientPlatform) -> i32 {
    let skip = [
        ".sig", ".asc", ".sha256", ".sha512", ".blockmap", ".yml", ".yaml",
        ".txt", ".json", ".zsync", ".ddeb", "pdb", "sources.tar", "source.tar",
    ];
    let low = name.to_ascii_lowercase();
    if skip.iter().any(|s| low.contains(s)) || low.ends_with('/') {
        return -1;
    }
    let mut score = 0i32;
    match plat.os.as_str() {
        "win" => {
            if low.ends_with(".msi") || low.ends_with(".exe") { score += 55; }
            if low.contains("win") || low.contains("windows") || low.contains("64-bit") { score += 25; }
            if low.ends_with(".zip") || low.ends_with(".7z") { score += 15; }
        }
        "mac" => {
            if low.ends_with(".dmg") || low.ends_with(".pkg") { score += 55; }
            if low.contains("mac") || low.contains("darwin") || low.contains("osx") { score += 25; }
        }
        "android" => {
            if low.ends_with(".apk") { score += 80; }
            if low.contains("android") { score += 20; }
        }
        _ => {
            if low.ends_with(".appimage") || low.ends_with(".deb") || low.ends_with(".tar.gz") || low.ends_with(".tar.xz") { score += 45; }
            if low.contains("linux") || low.contains("ubuntu") { score += 25; }
        }
    }
    if plat.arch == "arm64" {
        if low.contains("arm64") || low.contains("aarch64") { score += 25; }
    } else if low.contains("x64") || low.contains("x86_64") || low.contains("amd64") || low.contains("64-bit") {
        score += 25;
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
        let mut best_score = -1i32;
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

/// 解析 GitHub 最新 Release 并按客户端 OS/Arch 匹配最合适安装包
fn resolve_github_latest(repo: &str, plat: &ClientPlatform) -> Option<(String, String, String)> {
    let url = format!("https://api.github.com/repos/{repo}/releases/latest");
    let data = fetch_json_cached(&url)?;
    let tag = data.get("tag_name").and_then(Value::as_str)?.to_string();
    let assets = data.get("assets").and_then(Value::as_array)?;

    let mut best_score = -1i32;
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

#[derive(Debug, Deserialize)]
pub struct MirrorQuery {
    pub mirror: Option<String>,
    pub ghproxy: Option<String>,
    pub os: Option<String>,
    pub arch: Option<String>,
}

fn sanitize_ghproxy(raw: Option<&str>) -> &'static str {
    match raw.unwrap_or("cdn.gh-proxy.org").trim() {
        "gh-proxy.org" => "gh-proxy.org",
        "ghfast.top" => "ghfast.top",
        "wget.la" => "wget.la",
        _ => "cdn.gh-proxy.org",
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
            "ustc" => ("https://mirrors.ustc.edu.cn/fdroid/F-Droid.apk".to_string(), "中科大 USTC F-Droid 手机源"),
            "bfsu" => ("https://mirrors.bfsu.edu.cn/fdroid/F-Droid.apk".to_string(), "北外 BFSU F-Droid 手机源"),
            "official" => ("https://f-droid.org/F-Droid.apk".to_string(), "F-Droid 官方源"),
            _ => ("https://mirrors.tuna.tsinghua.edu.cn/fdroid/F-Droid.apk".to_string(), "清华大学 TUNA F-Droid 手机源"),
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

    // 2. 夸克网盘优先模式
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
    if mirror != "official" && item.repo.contains('/') && !item.repo.starts_with("http") {
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

    // 2.8 Cloudflare Wrangler 专属直链 / 夸克高速盘（未进高校同步池的热门软件优先走满速网盘/R2）
    if mirror != "official" && slug == "clash-verge-rev" {
        return MirrorResolution {
            slug: item.slug.clone(),
            mirror: "wrangler".to_string(),
            mirror_label: "Cloudflare Wrangler 高速直链 (dl.lilyco42.top)".to_string(),
            os: plat.os.clone(),
            arch: plat.arch.clone(),
            version: item.version.clone(),
            asset_name: item.asset_name.clone(),
            target_url: "https://dl.lilyco42.top/d/clash-verge-rev".to_string(),
            official_url: item.github_download_url.clone(),
            quark_url: item.quark_url.clone(),
            chsrc_cmd: String::new(),
        };
    }
    if mirror != "official" && !item.quark_url.is_empty() {
        return MirrorResolution {
            slug: item.slug.clone(),
            mirror: "quark".to_string(),
            mirror_label: "夸克网盘国内满速镜像".to_string(),
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

    let (target_url, mirror_label) = if mirror == "official" {
        (raw_dl.clone(), "GitHub 官方直链".to_string())
    } else if is_github_binary {
        (
            format!("https://{proxy_host}/{raw_dl}"),
            format!("GitHub 国内加速 ({proxy_host})"),
        )
    } else if !item.quark_url.is_empty() {
        (item.quark_url.clone(), "夸克网盘高速镜像".to_string())
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
