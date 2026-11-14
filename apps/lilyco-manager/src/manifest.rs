use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// 应用清单文件名。每个 lilyco 系应用在自己的仓库根目录提交这一份文件。
pub const MANIFEST_FILE: &str = "lilyco.app.json";
const MAX_MANIFEST_BYTES: u64 = 64 * 1024;
const MAX_ARGS: usize = 32;
const MAX_ARG_BYTES: usize = 512;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Manifest {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub repository: Option<String>,
    /// 可执行文件基名，只允许 PATH 中查找；不允许带路径分隔符。
    pub binary: String,
    #[serde(default)]
    pub capabilities: Capabilities,
    #[serde(default)]
    pub run: Option<RunSpec>,
    #[serde(default)]
    pub mcp: Option<McpSpec>,
    #[serde(default)]
    pub schema: Option<SchemaSpec>,
    #[serde(default)]
    pub release: Option<ReleaseSpec>,
    #[serde(default)]
    pub dev: Option<DevSpec>,
    /// 清单所在仓库目录，由加载器填充，不写入 JSON。
    #[serde(skip)]
    pub source_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Capabilities {
    #[serde(default)]
    pub run: bool,
    #[serde(default)]
    pub mcp: bool,
    #[serde(default)]
    pub schema: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RunSpec {
    #[serde(default)]
    pub args: Vec<String>,
    /// 启动时注入的固定环境变量（例如端口与“不要自动开浏览器”）。
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// 仅用于 UI 的“打开”按钮，必须指向本机回环地址。
    #[serde(default)]
    pub url: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct McpSpec {
    #[serde(default = "default_mcp_args")]
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SchemaSpec {
    #[serde(default = "default_schema_args")]
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ReleaseSpec {
    /// GitHub 仓库，`owner/name`。
    pub repo: String,
    /// 平台键 -> 发布资产文件名。平台键如 `linux-x64`、`windows-x64`、`macos-universal`。
    #[serde(default)]
    pub assets: BTreeMap<String, String>,
    /// 压缩包内可执行文件路径；缺省时按 `binary` 文件名递归查找。
    #[serde(default)]
    pub binary_path: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DevSpec {
    /// 任务名 -> 固定 argv。任务在清单所在仓库目录执行。
    #[serde(default)]
    pub tasks: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

fn default_mcp_args() -> Vec<String> {
    vec!["--mcp".to_string()]
}

fn default_schema_args() -> Vec<String> {
    vec!["--schema".to_string()]
}

impl Manifest {
    /// 清单缺省 capabilities 时按是否存在对应段落推断，减少手写错误。
    fn infer_capabilities(&mut self) {
        if self.run.is_some() {
            self.capabilities.run = true;
        }
        if self.mcp.is_some() {
            self.capabilities.mcp = true;
        }
        if self.schema.is_some() {
            self.capabilities.schema = true;
        }
    }

    pub fn run_args(&self) -> &[String] {
        self.run.as_ref().map(|r| r.args.as_slice()).unwrap_or(&[])
    }

    pub fn mcp_args(&self) -> Vec<String> {
        self.mcp
            .as_ref()
            .map(|m| m.args.clone())
            .unwrap_or_else(default_mcp_args)
    }

    pub fn schema_args(&self) -> Vec<String> {
        self.schema
            .as_ref()
            .map(|s| s.args.clone())
            .unwrap_or_else(default_schema_args)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1 {
            return Err(format!("不支持的 schema_version: {}", self.schema_version));
        }
        validate_id(&self.id)?;
        if self.name.trim().is_empty() || self.name.len() > 120 {
            return Err("name 必须为 1-120 字符".into());
        }
        if !valid_binary(&self.binary) {
            return Err("binary 必须是 PATH 中的可执行文件名，不能含路径分隔符".into());
        }
        for (field, args) in [
            ("run.args", self.run.as_ref().map(|r| r.args.as_slice())),
            ("mcp.args", self.mcp.as_ref().map(|m| m.args.as_slice())),
            (
                "schema.args",
                self.schema.as_ref().map(|s| s.args.as_slice()),
            ),
        ] {
            if let Some(args) = args {
                validate_args(field, args)?;
            }
        }
        if let Some(run) = self.run.as_ref() {
            validate_env("run.env", &run.env)?;
            if let Some(url) = run.url.as_deref() {
                if !loopback_url(url) {
                    return Err("run.url 只允许 http://127.0.0.1、localhost 或 [::1]".into());
                }
            }
        }
        if let Some(repo) = self.repository.as_deref() {
            if !repo.starts_with("https://") || repo.len() > 300 {
                return Err("repository 必须是 https:// 链接".into());
            }
        }
        if let Some(release) = self.release.as_ref() {
            validate_repo(&release.repo)?;
            if release.assets.len() > 16 {
                return Err("release.assets 最多 16 项".into());
            }
            for (platform, asset) in &release.assets {
                if !valid_platform_key(platform) {
                    return Err(format!("release.assets 平台键不合法: {platform}"));
                }
                if !valid_asset_name(asset) {
                    return Err(format!("release.assets.{platform} 资产名不合法"));
                }
            }
            if let Some(path) = release.binary_path.as_deref() {
                validate_relative_path("release.binary_path", path)?;
            }
        }
        if let Some(dev) = self.dev.as_ref() {
            if dev.tasks.len() > 8 {
                return Err("dev.tasks 最多 8 个任务".into());
            }
            for (name, argv) in &dev.tasks {
                if !valid_task_name(name) {
                    return Err(format!("dev 任务名不合法: {name}"));
                }
                if argv.is_empty() {
                    return Err(format!("dev.tasks.{name} 不能为空"));
                }
                validate_args(&format!("dev.tasks.{name}"), argv)?;
            }
            validate_env("dev.env", &dev.env)?;
        }
        Ok(())
    }
}

pub fn validate_id(id: &str) -> Result<(), String> {
    let mut chars = id.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() || c.is_ascii_digit() => {}
        _ => return Err("id 必须以小写字母或数字开头".into()),
    }
    if id.len() > 64 {
        return Err("id 最长 64 字符".into());
    }
    if !id
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '.')
    {
        return Err("id 只允许小写字母、数字、- 和 .".into());
    }
    Ok(())
}

fn valid_binary(binary: &str) -> bool {
    !binary.is_empty()
        && binary.len() <= 128
        && !binary.starts_with('-')
        && binary != "."
        && binary != ".."
        && !binary.contains('/')
        && !binary.contains('\\')
        && !binary.contains('\0')
}

fn validate_args(field: &str, args: &[String]) -> Result<(), String> {
    if args.len() > MAX_ARGS {
        return Err(format!("{field} 最多 {MAX_ARGS} 个参数"));
    }
    for arg in args {
        if arg.len() > MAX_ARG_BYTES {
            return Err(format!("{field} 单个参数最长 {MAX_ARG_BYTES} 字节"));
        }
        if arg.contains('\0') {
            return Err(format!("{field} 参数不能包含 NUL"));
        }
    }
    Ok(())
}

fn valid_env_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 64
        && key
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
        && key.chars().next().is_some_and(|c| c.is_ascii_uppercase())
}

fn validate_env(field: &str, env: &BTreeMap<String, String>) -> Result<(), String> {
    if env.len() > 16 {
        return Err(format!("{field} 最多 16 个变量"));
    }
    for (key, value) in env {
        if !valid_env_key(key) {
            return Err(format!("{field} 变量名不合法: {key}"));
        }
        if value.len() > 256 || value.contains('\0') {
            return Err(format!("{field}.{key} 值不合法"));
        }
    }
    Ok(())
}

fn validate_repo(repo: &str) -> Result<(), String> {
    let mut parts = repo.split('/');
    let owner = parts.next().unwrap_or_default();
    let name = parts.next().unwrap_or_default();
    if owner.is_empty() || name.is_empty() || parts.next().is_some() {
        return Err("release.repo 必须是 owner/name".into());
    }
    let valid = |part: &str| {
        !part.is_empty()
            && part.len() <= 100
            && part
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    };
    if !valid(owner) || !valid(name) {
        return Err("release.repo 含有非法字符".into());
    }
    Ok(())
}

fn valid_platform_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 40
        && key
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn valid_asset_name(asset: &str) -> bool {
    !asset.is_empty()
        && asset.len() <= 200
        && !asset.contains('/')
        && !asset.contains('\\')
        && asset != "."
        && asset != ".."
        && !asset.contains('\0')
}

fn validate_relative_path(field: &str, path: &str) -> Result<(), String> {
    if path.is_empty() || path.len() > 200 || path.starts_with('/') || path.starts_with('\\') {
        return Err(format!("{field} 必须是相对路径"));
    }
    // 拒绝 Windows 盘符（含 `C:relative` 形式），跨平台清单不能依赖宿主解释。
    let bytes = path.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return Err(format!("{field} 必须是相对路径"));
    }
    for part in path.split(['/', '\\']) {
        if part == ".." {
            return Err(format!("{field} 不允许包含 .."));
        }
    }
    Ok(())
}

fn valid_task_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 32
        && name.chars().next().is_some_and(|c| c.is_ascii_lowercase())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn loopback_url(url: &str) -> bool {
    ["http://127.0.0.1:", "http://localhost:", "http://[::1]:"]
        .iter()
        .any(|prefix| url.starts_with(prefix))
}

#[derive(Debug, Default)]
pub struct Registry {
    pub apps: Vec<Manifest>,
    pub warnings: Vec<String>,
}

/// 默认清单目录：`$LILYCO_MANAGER_APPS`（路径列表），否则 `$XDG_CONFIG_HOME/lilyco/apps`
/// 或 `$HOME/.config/lilyco/apps`。目录内既可直接放 `*.json`，也可放子目录 + `lilyco.app.json`。
pub fn default_dirs() -> Vec<PathBuf> {
    if let Some(raw) = std::env::var_os("LILYCO_MANAGER_APPS") {
        let dirs: Vec<PathBuf> = std::env::split_paths(&raw)
            .filter(|path| !path.as_os_str().is_empty())
            .collect();
        if !dirs.is_empty() {
            return dirs;
        }
    }
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));
    vec![base.join("lilyco").join("apps")]
}

pub fn load() -> Registry {
    load_from(&default_dirs())
}

pub fn load_from(dirs: &[PathBuf]) -> Registry {
    let mut registry = Registry::default();
    let mut seen = BTreeSet::new();
    for dir in dirs {
        let entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(err) => {
                if err.kind() != std::io::ErrorKind::NotFound {
                    registry
                        .warnings
                        .push(format!("无法读取清单目录 {}: {err}", dir.display()));
                }
                continue;
            }
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let candidate = if path.is_dir() {
                let manifest = path.join(MANIFEST_FILE);
                if !manifest.is_file() {
                    continue;
                }
                manifest
            } else if path.extension().and_then(|ext| ext.to_str()) == Some("json") {
                path
            } else {
                continue;
            };
            match load_file(&candidate) {
                Ok(manifest) => {
                    if !seen.insert(manifest.id.clone()) {
                        registry.warnings.push(format!(
                            "忽略重复应用 id {} ({})",
                            manifest.id,
                            candidate.display()
                        ));
                        continue;
                    }
                    registry.apps.push(manifest);
                }
                Err(err) => registry
                    .warnings
                    .push(format!("忽略 {}: {err}", candidate.display())),
            }
        }
    }
    registry.apps.sort_by(|a, b| a.name.cmp(&b.name));
    registry
}

pub fn load_file(path: &Path) -> Result<Manifest, String> {
    let meta = std::fs::metadata(path).map_err(|err| err.to_string())?;
    if meta.len() > MAX_MANIFEST_BYTES {
        return Err("清单超过 64KB".into());
    }
    let raw = std::fs::read_to_string(path).map_err(|err| err.to_string())?;
    let mut manifest: Manifest = serde_json::from_str(&raw).map_err(|err| err.to_string())?;
    manifest.infer_capabilities();
    manifest.validate()?;
    manifest.source_dir = std::fs::canonicalize(path)
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf));
    Ok(manifest)
}

/// 管理器托管安装根目录：`$XDG_DATA_HOME/lilyco/apps`、`$HOME/.local/share/lilyco/apps`
/// 或 Windows 的 `%LOCALAPPDATA%`。
pub fn data_root() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local").join("share"))
        })
        .or_else(|| std::env::var_os("LOCALAPPDATA").map(PathBuf::from))?;
    Some(base.join("lilyco").join("apps"))
}

pub fn managed_dir(id: &str) -> Option<PathBuf> {
    Some(data_root()?.join(id))
}

/// 读取 `<data>/lilyco/apps/<id>/current.json` 指向的托管二进制。
pub fn managed_binary(manifest: &Manifest) -> Option<PathBuf> {
    let dir = managed_dir(&manifest.id)?;
    let raw = std::fs::read_to_string(dir.join("current.json")).ok()?;
    let value: Value = serde_json::from_str(&raw).ok()?;
    let tag = value.get("tag").and_then(Value::as_str)?;
    let binary = value.get("binary").and_then(Value::as_str)?;
    validate_relative_path("current.binary", binary).ok()?;
    let path = dir.join("releases").join(tag).join(binary);
    path.is_file().then_some(path)
}

/// 解析可执行文件：`LILYCO_MANAGER_BIN_<ID>` 覆盖 > 托管安装 > PATH。
pub fn resolve_binary(manifest: &Manifest) -> Option<PathBuf> {
    let key = format!(
        "LILYCO_MANAGER_BIN_{}",
        manifest.id.to_ascii_uppercase().replace(['-', '.'], "_")
    );
    if let Some(path) = std::env::var_os(key) {
        let path = PathBuf::from(path);
        if path.is_absolute() && path.is_file() {
            return Some(path);
        }
    }
    if let Some(path) = managed_binary(manifest) {
        return Some(path);
    }
    which(&manifest.binary)
}

pub fn which(binary: &str) -> Option<PathBuf> {
    if !valid_binary(binary) {
        return None;
    }
    let path_env = std::env::var_os("PATH")?;
    let suffixes: &[&str] = if cfg!(windows) { &[".exe"] } else { &[""] };
    for directory in std::env::split_paths(&path_env) {
        for suffix in suffixes {
            let path = directory.join(format!("{binary}{suffix}"));
            if path.is_file() {
                return Some(path);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Manifest {
        Manifest {
            schema_version: 1,
            id: "lycomc".into(),
            name: "Lycomc".into(),
            description: String::new(),
            repository: Some("https://github.com/lilyco-42/lycomc".into()),
            binary: "lycomc".into(),
            capabilities: Capabilities::default(),
            run: Some(RunSpec {
                args: vec![],
                env: BTreeMap::new(),
                url: Some("http://127.0.0.1:8766".into()),
            }),
            mcp: None,
            schema: None,
            release: None,
            dev: None,
            source_dir: None,
        }
    }

    #[test]
    fn accepts_valid_manifest() {
        let mut manifest = sample();
        manifest.infer_capabilities();
        assert!(manifest.validate().is_ok());
        assert!(manifest.capabilities.run);
        // sample has no mcp section, infer_capabilities does not invent one
        assert!(!manifest.capabilities.mcp);
    }

    #[test]
    fn rejects_path_in_binary() {
        let mut manifest = sample();
        manifest.binary = "../evil".into();
        assert!(manifest.validate().is_err());
    }

    #[test]
    fn rejects_non_loopback_url() {
        let mut manifest = sample();
        manifest.run.as_mut().unwrap().url = Some("http://example.com:8080".into());
        assert!(manifest.validate().is_err());
    }

    #[test]
    fn rejects_bad_ids() {
        for id in ["", "-x", "Has Space", "a/b", &"a".repeat(65)] {
            assert!(validate_id(id).is_err(), "{id} should be invalid");
        }
        for id in ["a", "lycomc", "lilyco.manager", "x-1"] {
            assert!(validate_id(id).is_ok(), "{id} should be valid");
        }
    }

    #[test]
    fn validates_release_spec() {
        let mut manifest = sample();
        manifest.release = Some(ReleaseSpec {
            repo: "lilyco-42/lycodl".into(),
            assets: BTreeMap::from([("linux-x64".into(), "lycodl-linux-x64.tar.gz".into())]),
            binary_path: Some("lycodl".into()),
        });
        assert!(manifest.validate().is_ok());

        manifest.release.as_mut().unwrap().repo = "no-slash".into();
        assert!(manifest.validate().is_err());
        manifest.release.as_mut().unwrap().repo = "lilyco-42/lycodl".into();
        manifest.release.as_mut().unwrap().binary_path = Some("../evil".into());
        assert!(manifest.validate().is_err());
        manifest.release.as_mut().unwrap().binary_path = Some("lycodl".into());
        manifest.release.as_mut().unwrap().assets =
            BTreeMap::from([("linux-x64".into(), "../escape.tar.gz".into())]);
        assert!(manifest.validate().is_err());
    }

    #[test]
    fn validates_dev_tasks() {
        let mut manifest = sample();
        manifest.dev = Some(DevSpec {
            tasks: BTreeMap::from([("check".into(), vec!["cargo".into(), "check".into()])]),
            env: BTreeMap::new(),
        });
        assert!(manifest.validate().is_ok());

        manifest.dev.as_mut().unwrap().tasks =
            BTreeMap::from([("check".into(), Vec::<String>::new())]);
        assert!(manifest.validate().is_err());
        manifest.dev.as_mut().unwrap().tasks =
            BTreeMap::from([("Bad Name".into(), vec!["cargo".into()])]);
        assert!(manifest.validate().is_err());
    }

    #[test]
    fn infer_mcp_capability_when_present() {
        let mut manifest = sample();
        manifest.mcp = Some(McpSpec { args: vec![] });
        manifest.infer_capabilities();
        assert!(manifest.capabilities.mcp);
        assert!(manifest.mcp_args().is_empty());
        manifest.mcp = None;
        assert_eq!(manifest.mcp_args(), vec!["--mcp".to_string()]);
    }

    #[test]
    fn rejects_windows_style_path_traversal() {
        for path in [
            "..\\evil.exe",
            "pkg\\..\\evil.exe",
            "C:\\Windows\\evil.exe",
            "\\\\server\\share\\evil.exe",
            "\\evil.exe",
        ] {
            assert!(
                validate_relative_path("binary_path", path).is_err(),
                "{path} should be rejected"
            );
        }
        assert!(validate_relative_path("binary_path", "pkg/app.exe").is_ok());
    }

    #[test]
    fn validates_windows_platform_asset_names() {
        let mut manifest = sample();
        manifest.release = Some(ReleaseSpec {
            repo: "lilyco-42/lycodl".into(),
            assets: BTreeMap::from([
                ("windows-x64".into(), "lycodl-windows-x64.zip".into()),
                ("windows-arm64".into(), "lycodl-windows-arm64.zip".into()),
                (
                    "macos-universal".into(),
                    "lycodl-macos-universal.tar.gz".into(),
                ),
            ]),
            binary_path: Some("bin/lycodl.exe".into()),
        });
        assert!(manifest.validate().is_ok());
        manifest.release.as_mut().unwrap().assets =
            BTreeMap::from([("windows-x64".into(), "pkg\\lycodl.zip".into())]);
        assert!(manifest.validate().is_err());
    }

    #[test]
    fn rejects_mixed_separator_traversal() {
        for path in ["pkg\\../evil", "pkg/..\\evil", "..\\..\\evil"] {
            assert!(
                validate_relative_path("binary_path", path).is_err(),
                "{path} should be rejected"
            );
        }
    }
}
