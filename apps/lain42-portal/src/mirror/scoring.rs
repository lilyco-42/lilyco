//! 安装包 OS/架构分类、跨平台负分惩罚与多渠道镜像链接构造。

use super::{ClientPlatform, MirrorLink};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct AssetClassification {
    pub(super) is_ignored: bool,
    pub(super) os: &'static str,
    pub(super) arch: &'static str,
    pub(super) ext: String,
    pub(super) is_auxiliary: bool,
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

pub(super) fn classify_asset(name: &str) -> AssetClassification {
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

pub(super) fn score_asset_name(name: &str, plat: &ClientPlatform) -> i32 {
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


pub(super) fn build_asset_mirrors(raw_dl: &str, primary_proxy: &'static str) -> Vec<MirrorLink> {
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

pub(super) fn os_sort_rank(os: &str) -> u8 {
    match os {
        "win" => 0,
        "mac" => 1,
        "linux" => 2,
        "android" => 3,
        "ios" => 4,
        _ => 5,
    }
}

pub(super) fn arch_sort_rank(arch: &str) -> u8 {
    match arch {
        "x64" => 0,
        "arm64" => 1,
        "universal" => 2,
        "x86" => 3,
        _ => 4,
    }
}

pub(super) fn ext_sort_rank(ext: &str) -> u8 {
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


pub(super) fn sanitize_ghproxy(raw: Option<&str>) -> &'static str {
    match raw.unwrap_or("ghfast.top").trim() {
        "gh-proxy.org" => "gh-proxy.org",
        "cdn.gh-proxy.org" => "cdn.gh-proxy.org",
        _ => "ghfast.top",
    }
}

