//! lchatgpt 自己的凭证仓库 —— 只写自己的文件，绝不改写 codex 的 auth.json。
//!
//! 位置：`$LCHATGPT_STORE` → `$XDG_DATA_HOME/lchatgpt/auth.json` →
//! `~/.local/share/lchatgpt/auth.json`。文件权限 0600：凭证文件不该被同机其他用户读到。
//!
//! 形状刻意兼容 codex 的 auth.json（顶层 `access_token` / `refresh_token` / `account_id`），
//! 这样 `auth::load` 用同一套解析就能读两种文件，用户不会遇到两套格式。

use lilyco::prelude::AppError;
use serde_json::{json, Value};
use std::path::PathBuf;

/// 凭证仓库路径。三级回落，任何一级给了空串就当没给。
pub fn store_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("LCHATGPT_STORE") {
        let p = PathBuf::from(p);
        if !p.as_os_str().is_empty() {
            return Some(p);
        }
    }
    if let Some(x) = std::env::var_os("XDG_DATA_HOME") {
        let p = PathBuf::from(x);
        if !p.as_os_str().is_empty() {
            return Some(p.join("lchatgpt").join("auth.json"));
        }
    }
    std::env::var_os("HOME").map(|h| {
        PathBuf::from(h)
            .join(".local")
            .join("share")
            .join("lchatgpt")
            .join("auth.json")
    })
}

/// 读仓库。不存在 / 坏 JSON 都返回 None —— 仓库坏了不该让只读命令一起崩，
/// 后面还有 codex auth.json 和浏览器登录态可以回落。
/// 从指定文件读；读不到就是 None（不区分「没有」和「坏了」，两者都只能回落）
pub fn read_at(path: &std::path::Path) -> Option<Value> {
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

/// 写仓库：父目录自动建，落盘后改 0600。
pub fn save(entry: &Value) -> Result<PathBuf, AppError> {
    let path = store_path().ok_or_else(|| {
        AppError::InvalidInput(
            "拿不到 HOME / XDG_DATA_HOME，无法确定凭证仓库位置（可用 LCHATGPT_STORE 指定）"
                .to_string(),
        )
    })?;
    save_at(&path, entry)?;
    Ok(path)
}

/// 写到指定文件（测试用这个，避免踩环境变量并发）
pub fn save_at(path: &std::path::Path, entry: &Value) -> Result<(), AppError> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| AppError::Runtime(format!("建目录失败 {}: {e}", dir.display())))?;
    }
    let body = serde_json::to_string_pretty(entry)
        .map_err(|e| AppError::Runtime(format!("序列化凭证失败: {e}")))?;
    std::fs::write(path, body)
        .map_err(|e| AppError::Runtime(format!("写凭证失败 {}: {e}", path.display())))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

/// 当前 epoch 秒
pub fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// 组装一条仓库记录。`refresh_token` 为 None 时不写这一项 —— 免得空串覆盖掉旧的。
pub fn entry(
    access_token: &str,
    refresh_token: Option<&str>,
    account_id: Option<&str>,
    email: Option<&str>,
    expires_at: Option<i64>,
    source: &str,
) -> Value {
    let mut v = json!({
        "access_token": access_token,
        "account_id": account_id,
        "email": email,
        "expires_at": expires_at,
        "expires_at_iso": expires_at.map(crate::util::iso8601_from_epoch_secs).unwrap_or_default(),
        "source": source,
        "saved_at": crate::util::iso8601_from_epoch_secs(now_secs()),
    });
    if let Some(r) = refresh_token {
        v["refresh_token"] = json!(r);
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    /// LCHATGPT_STORE 优先于 XDG / HOME，CI 里能指到临时目录
    #[test]
    fn the_explicit_store_path_wins() {
        let _env = crate::testenv::scoped();
        std::env::set_var("LCHATGPT_STORE", "/tmp/x/auth.json");
        std::env::set_var("XDG_DATA_HOME", "/tmp/xdg");
        let p = store_path().unwrap();
        assert_eq!(p, PathBuf::from("/tmp/x/auth.json"));
    }

    /// 记录里必须能看出「这份凭证哪来的、什么时候到期」—— 排查时全靠这两个字段
    #[test]
    fn an_entry_carries_its_provenance() {
        let e = entry(
            "tok",
            Some("ref"),
            Some("acct"),
            Some("a@b.c"),
            Some(1_796_383_114),
            "test",
        );
        assert_eq!(e["access_token"], "tok");
        assert_eq!(e["refresh_token"], "ref");
        assert_eq!(e["expires_at_iso"], "2026-12-04T11:18:34Z");
        assert!(e["saved_at"].as_str().unwrap().ends_with('Z'));
        // 没有 refresh_token 时不要写空串
        let e2 = entry("tok", None, None, None, None, "test");
        assert!(e2.get("refresh_token").is_none(), "{e2}");
    }

    /// 落盘 + 读回 + 权限 0600
    #[test]
    fn a_saved_store_can_be_read_back() {
        let path = std::env::temp_dir()
            .join("lchatgpt-store-test")
            .join("auth.json");
        let _ = std::fs::remove_file(&path);
        save_at(&path, &entry("tok", None, None, None, None, "test")).unwrap();
        let back = read_at(&path).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "凭证文件必须只有本人可读");
        }
        let _ = std::fs::remove_file(&path);
        assert_eq!(back["access_token"], "tok");
        assert!(read_at(&path).is_none(), "文件删了就该读不到");
    }
}
