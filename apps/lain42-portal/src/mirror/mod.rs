//! 国内镜像源智能重定向与最新版本解析引擎（参考 RubyMetric/chsrc）。
//!
//! 零本站存储原则：所有安装包均直连国内官方开源镜像站（阿里云 / 清华 TUNA / 中科大 USTC /
//! 北外 BFSU / 腾讯云 / 字节跳动 rsproxy / F-Droid 手机镜像 / ghfast）、夸克网盘或 Cloudflare Wrangler。

mod http;
mod providers;
mod scoring;

use axum::http::HeaderMap;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::db::DownloadItem;

use self::http::fetch_github_latest_release;
use self::providers::{resolve_github_latest, resolve_nodejs, resolve_university_github_release};
use self::scoring::{
    arch_sort_rank, build_asset_mirrors, classify_asset, ext_sort_rank, os_sort_rank,
    sanitize_ghproxy,
};

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
    use super::scoring::{build_asset_mirrors, classify_asset, score_asset_name};

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
