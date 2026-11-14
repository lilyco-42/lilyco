use crate::manifest::{self, Manifest};
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

const MAX_DOWNLOAD_BYTES: u64 = 256 * 1024 * 1024;
const MAX_EXTRACT_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
pub struct ReleaseStatus {
    pub configured: bool,
    pub repo: Option<String>,
    pub platform: &'static str,
    pub current_tag: Option<String>,
    pub latest_tag: Option<String>,
    pub asset: Option<String>,
    pub update_available: bool,
    pub error: Option<String>,
}

#[derive(Debug)]
pub struct InstallOutcome {
    pub tag: String,
    pub binary: String,
    pub updated: bool,
}

#[derive(Debug, Clone)]
struct Asset {
    name: String,
    digest: Option<String>,
    url: String,
}

pub fn current_tag(manifest: &Manifest) -> Option<String> {
    let dir = manifest::managed_dir(&manifest.id)?;
    let raw = std::fs::read_to_string(dir.join("current.json")).ok()?;
    let value: Value = serde_json::from_str(&raw).ok()?;
    value.get("tag").and_then(Value::as_str).map(str::to_string)
}

pub fn platform_key() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => "linux-x64",
        ("linux", "aarch64") => "linux-aarch64",
        ("windows", "x86_64") => "windows-x64",
        ("windows", "aarch64") => "windows-arm64",
        ("macos", _) => "macos-universal",
        _ => "unknown",
    }
}

fn platform_aliases(key: &'static str) -> &'static [&'static str] {
    match key {
        "linux-x64" => &["linux-x64", "linux-amd64", "linux-x86_64"],
        "linux-aarch64" => &["linux-aarch64", "linux-arm64"],
        "windows-x64" => &["windows-x64", "windows-amd64"],
        "windows-arm64" => &["windows-arm64", "windows-aarch64"],
        "macos-universal" => &["macos-universal", "macos-arm64", "macos-x64"],
        _ => &[],
    }
}

pub async fn status(manifest: &Manifest) -> ReleaseStatus {
    let current = current_tag(manifest);
    let Some(spec) = manifest.release.as_ref() else {
        return ReleaseStatus {
            configured: false,
            repo: None,
            platform: platform_key(),
            current_tag: current,
            latest_tag: None,
            asset: None,
            update_available: false,
            error: None,
        };
    };
    match latest_release(&spec.repo).await {
        Ok((tag, assets)) => {
            let asset = pick_asset(&spec.assets, &assets);
            ReleaseStatus {
                configured: true,
                repo: Some(spec.repo.clone()),
                platform: platform_key(),
                current_tag: current.clone(),
                latest_tag: Some(tag.clone()),
                asset: asset.map(|asset| asset.name.clone()),
                update_available: current.as_deref() != Some(tag.as_str()),
                error: asset
                    .is_none()
                    .then(|| "发布中没有匹配当前平台的资产".to_string()),
            }
        }
        Err(error) => ReleaseStatus {
            configured: true,
            repo: Some(spec.repo.clone()),
            platform: platform_key(),
            current_tag: current,
            latest_tag: None,
            asset: None,
            update_available: false,
            error: Some(error),
        },
    }
}

pub async fn install(manifest: &Manifest) -> Result<InstallOutcome, String> {
    let spec = manifest
        .release
        .as_ref()
        .ok_or("该应用未配置 release 发布源")?;
    let (tag, assets) = latest_release(&spec.repo).await?;
    validate_tag(&tag)?;
    let asset = pick_asset(&spec.assets, &assets)
        .ok_or_else(|| format!("发布 {tag} 没有匹配 {} 的资产", platform_key()))?;

    let dir = manifest::managed_dir(&manifest.id).ok_or("无法确定本机数据目录")?;
    let downloads = dir.join("downloads");
    let releases = dir.join("releases");
    std::fs::create_dir_all(&downloads).map_err(|err| format!("创建目录失败: {err}"))?;
    std::fs::create_dir_all(&releases).map_err(|err| format!("创建目录失败: {err}"))?;

    let archive = downloads.join(&asset.name);
    download(asset, &archive).await?;

    let tmp = releases.join(format!(".tmp-{tag}-{}", std::process::id()));
    if tmp.exists() {
        std::fs::remove_dir_all(&tmp).map_err(|err| format!("清理临时目录失败: {err}"))?;
    }
    std::fs::create_dir_all(&tmp).map_err(|err| format!("创建临时目录失败: {err}"))?;

    if let Err(error) = extract(&archive, &tmp, &manifest.binary) {
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(error);
    }
    let binary_rel = locate_binary(manifest, spec.binary_path.as_deref(), &tmp)?;

    let final_dir = releases.join(&tag);
    if final_dir.exists() {
        std::fs::remove_dir_all(&final_dir).map_err(|err| format!("替换旧版本失败: {err}"))?;
    }
    std::fs::rename(&tmp, &final_dir).map_err(|err| format!("安装目录落位失败: {err}"))?;
    let binary_abs = final_dir.join(&binary_rel);
    make_executable(&binary_abs)?;

    let previous = current_tag(manifest);
    let pointer = json!({
        "tag": tag,
        "binary": binary_rel.to_string_lossy(),
        "installed_at": now_secs(),
    });
    atomic_write(
        &dir.join("current.json"),
        &serde_json::to_vec_pretty(&pointer).map_err(|err| err.to_string())?,
    )?;
    let _ = std::fs::remove_file(&archive);

    Ok(InstallOutcome {
        updated: previous.as_deref() != Some(tag.as_str()),
        tag,
        binary: binary_abs.to_string_lossy().to_string(),
    })
}

/// 只删除管理器托管的安装目录；不触碰 PATH 中的版本或源码仓库。
pub fn uninstall(manifest: &Manifest) -> Result<bool, String> {
    let dir = manifest::managed_dir(&manifest.id).ok_or("无法确定本机数据目录")?;
    if !dir.exists() {
        return Ok(false);
    }
    std::fs::remove_dir_all(&dir).map_err(|err| format!("删除托管安装失败: {err}"))?;
    Ok(true)
}

fn pick_asset<'a>(
    configured: &std::collections::BTreeMap<String, String>,
    assets: &'a [Asset],
) -> Option<&'a Asset> {
    pick_asset_for(platform_key(), configured, assets)
}

fn pick_asset_for<'a>(
    key: &'static str,
    configured: &std::collections::BTreeMap<String, String>,
    assets: &'a [Asset],
) -> Option<&'a Asset> {
    for platform in platform_aliases(key) {
        let Some(name) = configured.get(*platform) else {
            continue;
        };
        if let Some(asset) = assets.iter().find(|asset| &asset.name == name) {
            return Some(asset);
        }
    }
    None
}

async fn latest_release(repo: &str) -> Result<(String, Vec<Asset>), String> {
    let url = format!("https://api.github.com/repos/{repo}/releases/latest");
    let mut request = http_client()?
        .get(&url)
        .header("Accept", "application/vnd.github+json");
    if let Ok(token) = std::env::var("GITHUB_TOKEN").or_else(|_| std::env::var("GH_TOKEN")) {
        request = request.bearer_auth(token);
    }
    let response = request
        .send()
        .await
        .map_err(|err| format!("请求 GitHub 失败: {err}"))?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(format!("{repo} 没有公开 Release"));
    }
    if !response.status().is_success() {
        return Err(format!("GitHub API 返回 {}", response.status()));
    }
    let body: Value = response
        .json()
        .await
        .map_err(|err| format!("解析 Release 失败: {err}"))?;
    let tag = body
        .get("tag_name")
        .and_then(Value::as_str)
        .ok_or("Release 缺少 tag_name")?
        .to_string();
    let mut assets = Vec::new();
    if let Some(list) = body.get("assets").and_then(Value::as_array) {
        for asset in list {
            let Some(name) = asset.get("name").and_then(Value::as_str) else {
                continue;
            };
            let Some(url) = asset.get("browser_download_url").and_then(Value::as_str) else {
                continue;
            };
            assets.push(Asset {
                name: name.to_string(),
                digest: asset
                    .get("digest")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                url: url.to_string(),
            });
        }
    }
    Ok((tag, assets))
}

async fn download(asset: &Asset, destination: &Path) -> Result<(), String> {
    let digest = asset
        .digest
        .as_deref()
        .ok_or("发布资产没有 sha256 digest，拒绝安装")?;
    let expected = digest
        .strip_prefix("sha256:")
        .ok_or("发布资产 digest 不是 sha256")?;

    let response = http_client()?
        .get(&asset.url)
        .send()
        .await
        .map_err(|err| format!("下载失败: {err}"))?;
    if !response.status().is_success() {
        return Err(format!("下载返回 {}", response.status()));
    }
    if let Some(length) = response.content_length() {
        if length > MAX_DOWNLOAD_BYTES {
            return Err("发布资产超过 256MB 上限".into());
        }
    }

    let part = destination.with_extension("part");
    let mut file = tokio::fs::File::create(&part)
        .await
        .map_err(|err| format!("创建下载文件失败: {err}"))?;
    let mut written: u64 = 0;
    let mut response = response;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|err| format!("下载中断: {err}"))?
    {
        written += chunk.len() as u64;
        if written > MAX_DOWNLOAD_BYTES {
            let _ = tokio::fs::remove_file(&part).await;
            return Err("发布资产超过 256MB 上限".into());
        }
        tokio::io::AsyncWriteExt::write_all(&mut file, &chunk)
            .await
            .map_err(|err| format!("写入下载文件失败: {err}"))?;
    }
    tokio::io::AsyncWriteExt::flush(&mut file)
        .await
        .map_err(|err| format!("写入下载文件失败: {err}"))?;
    drop(file);

    let actual = sha256_file(&part)?;
    if !actual.eq_ignore_ascii_case(expected) {
        let _ = std::fs::remove_file(&part);
        return Err(format!(
            "SHA-256 校验失败（期望 {expected}，实际 {actual}）"
        ));
    }
    std::fs::rename(&part, destination).map_err(|err| format!("保存下载失败: {err}"))?;
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file = std::fs::File::open(path).map_err(|err| format!("读取下载失败: {err}"))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|err| format!("读取下载失败: {err}"))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn extract(archive: &Path, destination: &Path, binary: &str) -> Result<(), String> {
    let name = archive
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        let file = std::fs::File::open(archive).map_err(|err| format!("打开压缩包失败: {err}"))?;
        let decoder = flate2::read::GzDecoder::new(file);
        let mut tar = tar::Archive::new(decoder);
        tar.unpack(destination)
            .map_err(|err| format!("解压 tar.gz 失败: {err}"))?;
        return Ok(());
    }
    if name.ends_with(".zip") {
        let file = std::fs::File::open(archive).map_err(|err| format!("打开压缩包失败: {err}"))?;
        let mut zip = zip::ZipArchive::new(file).map_err(|err| format!("读取 zip 失败: {err}"))?;
        let mut written: u64 = 0;
        for index in 0..zip.len() {
            let mut entry = zip
                .by_index(index)
                .map_err(|err| format!("读取 zip 条目失败: {err}"))?;
            let Some(relative) = entry.enclosed_name() else {
                return Err("zip 包含越界路径".into());
            };
            let target = destination.join(relative);
            if entry.is_dir() {
                std::fs::create_dir_all(&target).map_err(|err| format!("创建目录失败: {err}"))?;
                continue;
            }
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(|err| format!("创建目录失败: {err}"))?;
            }
            written += entry.size();
            if written > MAX_EXTRACT_BYTES {
                return Err("解压后超过 512MB 上限".into());
            }
            let mut output =
                std::fs::File::create(&target).map_err(|err| format!("写入解压文件失败: {err}"))?;
            std::io::copy(&mut entry, &mut output).map_err(|err| format!("解压写入失败: {err}"))?;
            #[cfg(unix)]
            if let Some(mode) = entry.unix_mode() {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&target, std::fs::Permissions::from_mode(mode));
            }
        }
        return Ok(());
    }
    // 裸二进制：按 binary 名字落盘。
    let target = destination.join(binary);
    std::fs::copy(archive, &target).map_err(|err| format!("复制二进制失败: {err}"))?;
    Ok(())
}

fn locate_binary(
    manifest: &Manifest,
    configured: Option<&str>,
    root: &Path,
) -> Result<PathBuf, String> {
    if let Some(relative) = configured {
        let path = root.join(relative);
        if path.is_file() {
            return Ok(PathBuf::from(relative));
        }
        return Err(format!("压缩包内缺少 release.binary_path: {relative}"));
    }
    let wanted = if cfg!(windows) {
        format!("{}.exe", manifest.binary)
    } else {
        manifest.binary.clone()
    };
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    let mut visited = 0usize;
    while let Some((dir, depth)) = stack.pop() {
        if depth > 4 {
            continue;
        }
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            visited += 1;
            if visited > 5000 {
                return Err("压缩包条目过多".into());
            }
            let path = entry.path();
            if path.is_dir() {
                stack.push((path, depth + 1));
            } else if path.file_name().and_then(|name| name.to_str()) == Some(wanted.as_str()) {
                return path
                    .strip_prefix(root)
                    .map(Path::to_path_buf)
                    .map_err(|_| "无法计算可执行文件相对路径".to_string());
            }
        }
    }
    Err(format!("压缩包内未找到可执行文件 {}", manifest.binary))
}

fn make_executable(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
            .map_err(|err| format!("设置可执行权限失败: {err}"))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

fn atomic_write(path: &Path, body: &[u8]) -> Result<(), String> {
    let tmp = path.with_extension("tmp");
    let mut file = std::fs::File::create(&tmp).map_err(|err| format!("写入指针失败: {err}"))?;
    file.write_all(body)
        .map_err(|err| format!("写入指针失败: {err}"))?;
    file.flush().map_err(|err| format!("写入指针失败: {err}"))?;
    std::fs::rename(&tmp, path).map_err(|err| format!("保存指针失败: {err}"))
}

fn validate_tag(tag: &str) -> Result<(), String> {
    if tag.is_empty()
        || tag.len() > 64
        || !tag
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_')
    {
        return Err(format!("发布 tag 不合法: {tag}"));
    }
    Ok(())
}

fn http_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(120))
        .user_agent(format!("lilyco-manager/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|err| format!("创建 HTTP 客户端失败: {err}"))
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_manifest() -> Manifest {
        Manifest {
            schema_version: 1,
            id: "test-app".into(),
            name: "Test App".into(),
            description: String::new(),
            repository: None,
            binary: "app".into(),
            capabilities: Default::default(),
            run: None,
            mcp: None,
            schema: None,
            release: None,
            dev: None,
            source_dir: None,
        }
    }

    fn temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "lilyco-manager-test-{}-{label}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn rejects_unsafe_tags() {
        assert!(validate_tag("v0.1.0").is_ok());
        assert!(validate_tag("../x").is_err());
        assert!(validate_tag("v1/2").is_err());
        assert!(validate_tag("").is_err());
    }

    #[test]
    fn platform_aliases_cover_every_supported_target() {
        for key in [
            "linux-x64",
            "linux-aarch64",
            "windows-x64",
            "windows-arm64",
            "macos-universal",
        ] {
            assert!(
                !platform_aliases(key).is_empty(),
                "missing aliases for {key}"
            );
        }
        assert!(platform_aliases("unknown").is_empty());
    }

    #[test]
    fn picks_platform_alias_for_each_target() {
        let configured = std::collections::BTreeMap::from([
            ("linux-x64".to_string(), "tool-linux-amd64".to_string()),
            ("linux-aarch64".to_string(), "tool-linux-arm64".to_string()),
            ("windows-x64".to_string(), "tool-windows-amd64".to_string()),
            (
                "windows-arm64".to_string(),
                "tool-windows-aarch64".to_string(),
            ),
            (
                "macos-universal".to_string(),
                "tool-macos-universal".to_string(),
            ),
        ]);
        let assets = vec![
            Asset {
                name: "tool-linux-amd64".into(),
                digest: None,
                url: String::new(),
            },
            Asset {
                name: "tool-linux-arm64".into(),
                digest: None,
                url: String::new(),
            },
            Asset {
                name: "tool-windows-amd64".into(),
                digest: None,
                url: String::new(),
            },
            Asset {
                name: "tool-windows-aarch64".into(),
                digest: None,
                url: String::new(),
            },
            Asset {
                name: "tool-macos-universal".into(),
                digest: None,
                url: String::new(),
            },
        ];
        for (key, expected) in [
            ("linux-x64", "tool-linux-amd64"),
            ("linux-aarch64", "tool-linux-arm64"),
            ("windows-x64", "tool-windows-amd64"),
            ("windows-arm64", "tool-windows-aarch64"),
            ("macos-universal", "tool-macos-universal"),
        ] {
            assert_eq!(
                pick_asset_for(key, &configured, &assets).map(|asset| asset.name.as_str()),
                Some(expected),
                "unexpected asset for {key}"
            );
        }
    }

    #[test]
    fn does_not_pick_other_platforms() {
        let configured =
            std::collections::BTreeMap::from([("windows-x64".to_string(), "tool.exe".to_string())]);
        let assets = vec![Asset {
            name: "tool.exe".into(),
            digest: None,
            url: String::new(),
        }];
        assert!(pick_asset_for("linux-x64", &configured, &assets).is_none());
    }

    #[test]
    fn locates_configured_and_discovered_binaries() {
        let root = temp_dir("locate");
        std::fs::create_dir_all(root.join("pkg")).unwrap();
        let name = if cfg!(windows) { "app.exe" } else { "app" };
        std::fs::write(root.join("pkg").join(name), b"binary").unwrap();

        let manifest = sample_manifest();
        assert_eq!(
            locate_binary(&manifest, Some("pkg/app"), &root).unwrap(),
            PathBuf::from("pkg/app")
        );
        assert!(locate_binary(&manifest, Some("pkg/missing"), &root).is_err());
        assert_eq!(
            locate_binary(&manifest, None, &root).unwrap(),
            PathBuf::from("pkg").join(name)
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn extracts_raw_binary_archive() {
        let root = temp_dir("raw");
        let archive = root.join("download");
        std::fs::write(&archive, b"#!/bin/sh\n").unwrap();
        let out = root.join("out");
        std::fs::create_dir_all(&out).unwrap();

        extract(&archive, &out, "app").unwrap();
        assert_eq!(std::fs::read(out.join("app")).unwrap(), b"#!/bin/sh\n");
        std::fs::remove_dir_all(&root).unwrap();
    }
}
