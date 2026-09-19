//! mpkg 最小格式 + 内容寻址存储
//!
//! 对齐 lystack mpkg v0.1 的**最小实现**，字段名与 cache-node 验证器保持一致
//! （`mpkg`/`name`/`version`/`intent`/`steps`/`verify` + `files` 哈希表）：
//!
//! - **包文件**：单个 UTF-8 JSON，即 mpkg v0.1 content-id 的计算输入
//!   `canon_json({"manifest": …, "files": {相对路径: sha256}})` 直接落盘。
//!   与 zip 容器的差异（最小子集）：`files` 只存内容哈希引用，不内嵌文件本体。
//! - **content-id**：`id = "sha256:" + sha256_hex(canon_json(包文件))`，
//!   与 cache-node `package_id` 逐字节一致。
//! - **存储**：内容寻址 KV（对齐 cache-node）——blob 以原始字节的 sha256 命名，
//!   另存名字索引（name → blob），按哈希或按包名取出皆可。
//! - **回放边界**：`steps`/`verify` 只做结构校验，不真实执行——执行任意外部
//!   命令属 T3 级动作，交由 cache-node verify 在受控环境完成（本工具只管
//!   "包是否完整可信"，不管"包是否可回放成功"）。
//!
//! ```ignore
//! let store = Store::new("./mpkg-store");
//! let info = store.put(&bytes)?;                 // 内容寻址落盘 + 名字索引
//! let r = store.resolve(None, Some("demo"))?;    // 按名定位 blob
//! let report = verify_pack(&bytes, Some(&r.blob), None)?;  // 完整性校验
//! ```

use std::path::PathBuf;

use lilyco::__core::AppError;
use serde_json::{json, Value};

use crate::sha256::sha256_hex;

// ── canon 与 content-id（与 cache-node 验证器逐字节对齐） ──

/// canon JSON：键排序 + 紧凑分隔符 + UTF-8 原样，与 Python
/// `json.dumps(sort_keys=True, separators=(",",":"), ensure_ascii=False)`
/// 及 cache-node `canon` 逐字节一致。
///
/// 显式递归排序后序列化——无论 serde_json 是否被其他 crate 启用
/// `preserve_order`（Map 换成 IndexMap），输出键序恒为字典序
pub fn canon(pack: &Value) -> String {
    fn sorted(v: &Value) -> Value {
        match v {
            Value::Object(m) => {
                let mut keys: Vec<&String> = m.keys().collect();
                keys.sort();
                let mut out = serde_json::Map::new();
                for k in keys {
                    out.insert(k.clone(), sorted(&m[k]));
                }
                Value::Object(out)
            }
            Value::Array(a) => Value::Array(a.iter().map(sorted).collect()),
            other => other.clone(),
        }
    }
    serde_json::to_string(&sorted(pack)).expect("canon_json 序列化不失败（Value 恒可序列化）")
}

/// content-id：`sha256:<hex>`，对 canon_json(包文件) 整体取哈希
/// （cache-node `package_id` 同式：`{"manifest": …, "files": {rel: sha256}}`）
pub fn package_id(pack: &Value) -> String {
    format!("sha256:{}", sha256_hex(canon(pack).as_bytes()))
}

// ── 结构校验（字段清单与 cache-node 验证器 validate 一致） ──

/// 64 位小写 sha256 hex
fn is_sha256_hex(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// 包名合法性：kebab-case（`[A-Za-z0-9_-]+`）——同时杜绝名字索引的路径穿越
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// 结构校验：必需字段齐全 + 类型正确 + `files` 相对路径与哈希格式合法
pub fn validate(pack: &Value) -> Result<(), AppError> {
    let obj = pack
        .as_object()
        .ok_or_else(|| AppError::InvalidInput("包文件必须是 JSON 对象".into()))?;
    // 顶层只允许 manifest / files——多出的字段会悄悄改变 content-id 语义
    for k in obj.keys() {
        if k != "manifest" && k != "files" {
            return Err(AppError::InvalidInput(format!(
                "包文件顶层字段 `{k}` 不在允许清单（manifest/files）"
            )));
        }
    }
    let m = pack
        .get("manifest")
        .ok_or_else(|| AppError::InvalidInput("manifest missing: manifest".into()))?;
    for k in ["mpkg", "name", "version", "intent", "steps", "verify"] {
        if m.get(k).is_none() {
            return Err(AppError::InvalidInput(format!("manifest missing: {k}")));
        }
    }
    if m["mpkg"] != "0.1" {
        return Err(AppError::InvalidInput(format!(
            "unsupported mpkg version: {}",
            m["mpkg"]
        )));
    }
    let name = m["name"]
        .as_str()
        .ok_or_else(|| AppError::InvalidInput("manifest.name 须为字符串".into()))?;
    if !valid_name(name) {
        return Err(AppError::InvalidInput(
            "manifest.name 须为 kebab-case（[A-Za-z0-9_-]+，防路径穿越）".into(),
        ));
    }
    for k in ["version", "intent"] {
        let v = m[k]
            .as_str()
            .ok_or_else(|| AppError::InvalidInput(format!("manifest.{k} 须为字符串")))?;
        if v.is_empty() {
            return Err(AppError::InvalidInput(format!("manifest.{k} 不可为空")));
        }
    }
    let steps = m["steps"]
        .as_array()
        .ok_or_else(|| AppError::InvalidInput("steps must be non-empty list of {run}".into()))?;
    if steps.is_empty() {
        return Err(AppError::InvalidInput(
            "steps must be non-empty list of {run}".into(),
        ));
    }
    for (i, s) in steps.iter().enumerate() {
        if s.get("run").and_then(|r| r.as_str()).is_none() {
            return Err(AppError::InvalidInput(format!(
                "steps[{i}] 缺少字符串字段 run"
            )));
        }
    }
    let verify = m["verify"]
        .as_array()
        .ok_or_else(|| AppError::InvalidInput("verify must be non-empty".into()))?;
    if verify.is_empty() {
        return Err(AppError::InvalidInput("verify must be non-empty".into()));
    }
    for (i, v) in verify.iter().enumerate() {
        if v.as_str().is_none() {
            return Err(AppError::InvalidInput(format!(
                "verify[{i}] 须为字符串命令"
            )));
        }
    }
    // files（可选）：相对路径 → 64 位小写 sha256 hex
    if let Some(files) = pack.get("files") {
        let f = files
            .as_object()
            .ok_or_else(|| AppError::InvalidInput("files 须为对象（相对路径 → sha256）".into()))?;
        for (rel, hash) in f {
            let segs: Vec<&str> = rel.split('/').collect();
            if rel.is_empty()
                || rel.starts_with('/')
                || rel.contains('\\')
                || segs.iter().any(|s| *s == ".." || s.is_empty())
            {
                return Err(AppError::InvalidInput(format!(
                    "files 路径非法（须为不带 .. 的相对路径）: {rel}"
                )));
            }
            let hex = hash
                .as_str()
                .ok_or_else(|| AppError::InvalidInput(format!("files[{rel}] 须为字符串哈希")))?;
            if !is_sha256_hex(hex) {
                return Err(AppError::InvalidInput(format!(
                    "files[{rel}] 须为 64 位小写 sha256 hex"
                )));
            }
        }
    }
    Ok(())
}

// ── 内容寻址存储 ──

/// 定位结果：blob 内容寻址 + 文件路径 + 名字索引项（按名解析时 Some）
pub struct Resolved {
    /// blob 内容寻址（64 位小写 hex，即原始字节的 sha256）
    pub blob: String,
    /// blob 文件路径（`<dir>/blobs/<blob>.mpkg`）
    pub path: PathBuf,
    /// 名字索引项（按名解析时 Some，含 id/name/version/intent）
    pub index: Option<Value>,
}

/// put 的返回信息
pub struct PutInfo {
    /// 包名（manifest.name）
    pub name: String,
    /// 包版本（manifest.version）
    pub version: String,
    /// content-id（sha256: 前缀）
    pub id: String,
    /// blob 内容寻址（64 位 hex）
    pub blob: String,
    /// 包文件字节数（遥测上报用）
    pub bytes: usize,
}

/// 内容寻址存储：`<dir>/blobs/<sha256hex>.mpkg`（唯一数据源）+ `<dir>/index/<name>.json`（名字索引）
pub struct Store {
    root: PathBuf,
}

impl Store {
    /// 以 `root` 为存储根目录
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// 写入：解析 + 结构校验 → 算 blob 哈希 → 内容寻址落盘 → 更新名字索引。
    /// 同内容重复写入幂等（blob 已存在则跳过）；重名 = 升级，后写者胜。
    pub fn put(&self, bytes: &[u8]) -> Result<PutInfo, AppError> {
        let pack: Value = serde_json::from_slice(bytes)
            .map_err(|e| AppError::InvalidInput(format!("包文件 JSON 解析失败: {e}")))?;
        validate(&pack)?;

        let blob = sha256_hex(bytes);
        let id = package_id(&pack);
        let name = manifest_str(&pack, "name")?.to_string();
        let version = manifest_str(&pack, "version")?.to_string();

        let blobs = self.root.join("blobs");
        std::fs::create_dir_all(&blobs)?;
        let blob_path = blobs.join(format!("{blob}.mpkg"));
        if !blob_path.exists() {
            std::fs::write(&blob_path, bytes)?;
        }

        let entry = json!({
            "name": name,
            "version": version,
            "id": id,
            "blob": blob,
        });
        let index_dir = self.root.join("index");
        std::fs::create_dir_all(&index_dir)?;
        std::fs::write(
            index_dir.join(format!("{name}.json")),
            serde_json::to_string_pretty(&entry)?.as_bytes(),
        )?;

        Ok(PutInfo {
            name,
            version,
            id,
            blob,
            bytes: bytes.len(),
        })
    }

    /// 定位 blob：`hash`（裸 64 位 hex 或 `sha256:` 前缀）或 `name`（名字索引）二选一
    pub fn resolve(&self, hash: Option<&str>, name: Option<&str>) -> Result<Resolved, AppError> {
        match (hash, name) {
            (Some(h), _) => {
                let hex = normalize_hash(h)?;
                let path = self.blob_path(&hex);
                if !path.is_file() {
                    return Err(AppError::InvalidArg(format!("blob 不存在: {hex}")));
                }
                Ok(Resolved {
                    blob: hex,
                    path,
                    index: None,
                })
            }
            (None, Some(n)) => {
                if !valid_name(n) {
                    return Err(AppError::InvalidArg(format!(
                        "包名非法（[A-Za-z0-9_-]+）: {n}"
                    )));
                }
                let idx = self.root.join("index").join(format!("{n}.json"));
                if !idx.is_file() {
                    return Err(AppError::InvalidArg(format!("名字索引不存在: {n}")));
                }
                let entry: Value = serde_json::from_str(&std::fs::read_to_string(&idx)?)?;
                let blob = entry["blob"]
                    .as_str()
                    .ok_or_else(|| AppError::InvalidInput("名字索引项缺 blob 字段".into()))?
                    .to_string();
                let path = self.blob_path(&blob);
                if !path.is_file() {
                    return Err(AppError::InvalidArg(format!(
                        "名字索引指向的 blob 缺失: {blob}"
                    )));
                }
                Ok(Resolved {
                    blob,
                    path,
                    index: Some(entry),
                })
            }
            _ => Err(AppError::InvalidArg("hash 与 name 须二选一传入".into())),
        }
    }

    fn blob_path(&self, hex: &str) -> PathBuf {
        self.root.join("blobs").join(format!("{hex}.mpkg"))
    }
}

/// 归一化哈希参数：接受裸 64 位 hex 或 `sha256:` 前缀，统一为裸小写 hex
pub fn normalize_hash(input: &str) -> Result<String, AppError> {
    let hex = input
        .strip_prefix("sha256:")
        .unwrap_or(input)
        .to_ascii_lowercase();
    if !is_sha256_hex(&hex) {
        return Err(AppError::InvalidArg(format!(
            "哈希格式非法（须 64 位 sha256 hex）: {input}"
        )));
    }
    Ok(hex)
}

/// 完整性校验：blob 字节哈希重算比对 + 结构字段检查 + content-id 重算比对。
/// 全部通过返回校验报告；任一失败返回 Err（原因合并可读）。
/// `expected_blob` / `expected_id` 传 `None` 表示跳过对应比对（如裸文件校验）。
pub fn verify_pack(
    bytes: &[u8],
    expected_blob: Option<&str>,
    expected_id: Option<&str>,
) -> Result<Value, AppError> {
    let blob = sha256_hex(bytes);
    let blob_match = expected_blob.map(|e| e == blob).unwrap_or(true);

    // 内容被破坏时也可能连 JSON 都不再是合法包——两类失败都要给出可读原因
    let pack: Value = serde_json::from_slice(bytes)
        .map_err(|e| AppError::InvalidInput(format!("包文件 JSON 解析失败: {e}")))?;
    let structure_err = validate(&pack).err();
    let id = package_id(&pack);
    let id_match = expected_id.map(|e| e == id).unwrap_or(true);

    let mut reasons: Vec<String> = Vec::new();
    if !blob_match {
        reasons.push("内容 sha256 与存储地址不符（内容被篡改）".into());
    }
    if let Some(e) = structure_err {
        reasons.push(e.to_string());
    }
    if !id_match {
        reasons.push("content-id 重算不符".into());
    }
    if !reasons.is_empty() {
        return Err(AppError::Runtime(format!(
            "mpkg 完整性校验失败: {}",
            reasons.join("; ")
        )));
    }

    let m = &pack["manifest"];
    let files = pack
        .get("files")
        .and_then(|f| f.as_object())
        .map(|f| f.len())
        .unwrap_or(0);
    Ok(json!({
        "ok": true,
        "id": id,
        "blob": blob,
        "name": m["name"],
        "version": m["version"],
        "files": files,
    }))
}

/// 读 manifest 顶层字符串字段（validate 之后调用，缺字段仍兜底报错）
fn manifest_str<'a>(pack: &'a Value, key: &str) -> Result<&'a str, AppError> {
    pack["manifest"][key]
        .as_str()
        .ok_or_else(|| AppError::InvalidInput(format!("manifest.{key} 须为字符串")))
}
