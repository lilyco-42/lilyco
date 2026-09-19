//! mpkg 三个派生应用——同一 struct 派生 CLI/TUI/Web/MCP，天生受安全门控
//!
//! - [`MpkgPut`]：T2 能力令牌——写记忆是敏感操作（污染 agent 的长期记忆比
//!   写 PLC 寄存器更难回滚），自动化面默认拒绝（P0 安全门），
//!   部署侧注入校验 token 的 `SafetyPolicy` 后放行
//! - [`MpkgGet`]：T0 只读，按哈希或包名取出内容与元数据
//! - [`MpkgVerify`]：T0 只读，完整性校验（重算 sha256 比对 + 结构字段检查），
//!   遥测上报验证结果

use std::path::PathBuf;

use lilyco::prelude::*;

use crate::pack::{self, Store};

/// 写入记忆包（T2 需能力令牌——自动化面默认拒绝）
#[derive(App)]
#[app(
    name = "mpkg-put",
    about = "写入记忆包到内容寻址存储（sha256 命名），需能力令牌，自动化面默认拒绝",
    run = "run_put",
    safety = "t2"
)]
pub struct MpkgPut {
    /// 记忆包存储目录（不存在则创建）
    dir: PathBuf,
    /// 包文件路径（mpkg v0.1 最小格式 JSON）
    file: PathBuf,
}

pub fn run_put(app: &MpkgPut, ctx: &Context) -> Result<serde_json::Value, AppError> {
    let bytes = std::fs::read(&app.file)?;
    let store = Store::new(&app.dir);
    let info = store.put(&bytes)?;
    // P1 遥测：写入字节数 + 包名，Agent 实时看到记忆包体量与去向
    ctx.telemetry("pack.bytes", serde_json::json!(info.bytes));
    ctx.telemetry("pack.name", serde_json::json!(info.name));
    let r = serde_json::json!({
        "id": info.id,
        "blob": info.blob,
        "name": info.name,
        "version": info.version,
        "bytes": info.bytes,
    });
    ctx.done(r.clone(), 0);
    Ok(r)
}

/// 取出记忆包（T0 只读）：按哈希或包名返回内容与元数据
#[derive(App)]
#[app(
    name = "mpkg-get",
    about = "按哈希或包名取出记忆包，返回清单元数据与文件哈希表（只读）",
    run = "run_get"
)]
pub struct MpkgGet {
    /// 记忆包存储目录
    dir: PathBuf,
    /// 内容哈希（64 位 hex 或 sha256: 前缀），与 name 二选一
    hash: Option<String>,
    /// 按包名取出（读名字索引），与 hash 二选一
    name: Option<String>,
}

pub fn run_get(app: &MpkgGet, ctx: &Context) -> Result<serde_json::Value, AppError> {
    let store = Store::new(&app.dir);
    let r = store.resolve(app.hash.as_deref(), app.name.as_deref())?;
    let bytes = std::fs::read(&r.path)?;
    let pack: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::InvalidInput(format!("包文件 JSON 解析失败: {e}")))?;
    pack::validate(&pack)?;

    // P1 遥测：取出的包体量
    ctx.telemetry("get.bytes", serde_json::json!(bytes.len()));
    let m = &pack["manifest"];
    let files = pack.get("files").cloned().unwrap_or(serde_json::json!({}));
    let result = serde_json::json!({
        "name": m["name"],
        "version": m["version"],
        "intent": m["intent"],
        "id": pack::package_id(&pack),
        "blob": r.blob,
        "manifest": m,
        "files": files,
    });
    ctx.done(result.clone(), 0);
    Ok(result)
}

/// 校验记忆包完整性（T0 只读）：重算 sha256 比对 + 结构字段检查
#[derive(App)]
#[app(
    name = "mpkg-verify",
    about = "校验记忆包完整性：重算 sha256 比对内容寻址 + 结构字段检查（只读）",
    run = "run_verify"
)]
pub struct MpkgVerify {
    /// 记忆包存储目录
    dir: PathBuf,
    /// 内容哈希（64 位 hex 或 sha256: 前缀），与 name 二选一
    hash: Option<String>,
    /// 按包名定位（读名字索引），与 hash 二选一
    name: Option<String>,
}

pub fn run_verify(app: &MpkgVerify, ctx: &Context) -> Result<serde_json::Value, AppError> {
    let store = Store::new(&app.dir);
    let r = store.resolve(app.hash.as_deref(), app.name.as_deref())?;
    let bytes = std::fs::read(&r.path)?;
    let expected_id = r.index.as_ref().and_then(|e| e["id"].as_str());
    match pack::verify_pack(&bytes, Some(&r.blob), expected_id) {
        Ok(report) => {
            // P1 遥测：验证结果 + 校验的文件引用数
            ctx.telemetry("verify.ok", serde_json::json!(true));
            ctx.telemetry("verify.files", report["files"].clone());
            ctx.done(report.clone(), 0);
            Ok(report)
        }
        Err(e) => {
            // P1 遥测：验证失败也要上报，Agent 立刻看到完整性被破坏
            ctx.telemetry("verify.ok", serde_json::json!(false));
            Err(e)
        }
    }
}
