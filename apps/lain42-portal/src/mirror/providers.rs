//! 各类上游镜像站（阿里云 npmmirror、高校 TUNA/NJU/BFSU、GitHub Release）解析器。

use serde_json::Value;

use super::http::{fetch_github_latest_release, fetch_json_cached, fetch_text_cached};
use super::scoring::score_asset_name;
use super::ClientPlatform;

/// 解析 Node.js 最新版本（实时查阿里云 npmmirror，自动匹配 Windows .msi / macOS .pkg / Linux .tar.xz）
pub(super) fn resolve_nodejs(branch: &str, mirror: &str, plat: &ClientPlatform) -> Option<(String, String, String, String)> {
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


/// 优先查询清华 TUNA / 南京大学 NJU / 北外 BFSU 高校官方 github-release 同步镜像（真 10~50 MB/s 不限速直连）
pub(super) fn resolve_university_github_release(
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


/// 解析 GitHub 最新 Release 并按客户端 OS/Arch 匹配最合适安装包
pub(super) fn resolve_github_latest(repo: &str, plat: &ClientPlatform) -> Option<(String, String, String)> {
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

