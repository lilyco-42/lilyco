//! mpkg 记忆包端到端（生态融合 Track 1）
//!
//! 链路：put（T2 门控）→ 内容寻址落盘 → get（T0）按哈希/名取出 → verify（T0）
//! 完整性校验，全程 tempfile 临时目录，云端可跑。

use std::sync::Arc;

use lilyco::__core::safety::{GateDecision, GateRequest};
use lilyco::prelude::*;
use lilyco_mpkg::{MpkgGet, MpkgPut, MpkgVerify, Store};

fn handler_of(reg: &Registry, name: &str) -> Handler {
    reg.get(name).unwrap().handler.clone().unwrap()
}

/// 最小合法包（字段名与 cache-node 验证器一致）
fn sample_pack() -> serde_json::Value {
    serde_json::json!({
        "manifest": {
            "mpkg": "0.1",
            "name": "make-snake-game",
            "version": "0.1.0",
            "intent": "回放后得到可玩的贪吃蛇",
            "steps": [ { "run": "echo snake", "expect": { "exit": 0 } } ],
            "verify": [ "test -f snake.py" ]
        },
        "files": {
            "artifacts/snake.py": "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        }
    })
}

fn sample_pack_bytes() -> Vec<u8> {
    serde_json::to_vec_pretty(&sample_pack()).unwrap()
}

/// 注册三命令的默认 Registry（DenyElevated：仅 T0 直通）
fn full_registry() -> Registry {
    let mut reg = Registry::new();
    reg.register(RegisteredCommand::from_app::<MpkgGet>())
        .unwrap();
    reg.register(RegisteredCommand::from_app::<MpkgVerify>())
        .unwrap();
    reg
}

// ── canon 与 content-id（对齐 cache-node） ────────────────

#[test]
fn canon_is_sorted_compact_utf8_verbatim() {
    // 顶层 files < manifest；manifest 键字典序；step 内 expect < run —— 全部紧凑
    assert_eq!(
        lilyco_mpkg::canon(&sample_pack()),
        concat!(
            r#"{"files":{"artifacts/snake.py":"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"},"#,
            r#""manifest":{"intent":"回放后得到可玩的贪吃蛇","mpkg":"0.1","name":"make-snake-game","#,
            r#""steps":[{"expect":{"exit":0},"run":"echo snake"}],"verify":["test -f snake.py"],"version":"0.1.0"}}"#
        )
    );
}

#[test]
fn package_id_is_sha256_prefixed_and_content_addressed() {
    let id = lilyco_mpkg::package_id(&sample_pack());
    assert!(id.starts_with("sha256:"), "{id}");
    assert_eq!(id.len(), "sha256:".len() + 64, "{id}");
    // 改一个字节 = 另一个包（content-addressed 免费防篡改）
    let mut tampered = sample_pack();
    tampered["manifest"]["intent"] = serde_json::json!("改一个字");
    assert_ne!(lilyco_mpkg::package_id(&tampered), id);
}

// ── 结构校验 ──────────────────────────────────────────────

#[test]
fn validate_rejects_bad_packs() {
    // 版本不符（必需字段齐全，才能走到版本检查）
    let mut bad_version = sample_pack();
    bad_version["manifest"]["mpkg"] = serde_json::json!("9.9");
    // 名字带路径穿越（需字段齐全才能走到名字检查）
    let mut bad_name = sample_pack();
    bad_name["manifest"]["name"] = serde_json::json!("../evil");
    let cases: Vec<(serde_json::Value, &str)> = vec![
        // 顶层未知字段
        (
            serde_json::json!({ "manifest": {}, "files": {}, "extra": 1 }),
            "不在允许清单",
        ),
        // 缺必需字段
        (
            serde_json::json!({ "manifest": { "mpkg": "0.1" } }),
            "missing",
        ),
        // 版本不符
        (bad_version, "unsupported mpkg version"),
        // 名字非法
        (bad_name, "kebab-case"),
    ];
    for (pack, expect) in cases {
        let err = lilyco_mpkg::validate(&pack).unwrap_err();
        assert!(err.to_string().contains(expect), "{err}");
    }
    // steps 非空且每条有 run
    let base = serde_json::json!({
        "manifest": {
            "mpkg": "0.1", "name": "ok", "version": "0.1.0", "intent": "x",
            "steps": [], "verify": [ "true" ]
        }
    });
    assert!(lilyco_mpkg::validate(&base)
        .unwrap_err()
        .to_string()
        .contains("non-empty"));
    let mut no_run = base.clone();
    no_run["manifest"]["steps"] = serde_json::json!([ { "cmd": "ls" } ]);
    assert!(lilyco_mpkg::validate(&no_run)
        .unwrap_err()
        .to_string()
        .contains("run"));
    // files 哈希非法
    let mut bad_hash = sample_pack();
    bad_hash["files"]["artifacts/snake.py"] = serde_json::json!("NOT-A-HASH");
    assert!(lilyco_mpkg::validate(&bad_hash)
        .unwrap_err()
        .to_string()
        .contains("sha256 hex"));
    // 合法包通过
    assert!(lilyco_mpkg::validate(&sample_pack()).is_ok());
}

// ── put → get → verify 全链 ───────────────────────────────

#[test]
fn put_get_verify_full_chain() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("store");
    let bytes = sample_pack_bytes();

    // put（库层直调——同一逻辑被 T2 App 包着）：内容寻址落盘 + 名字索引
    let store = Store::new(&dir);
    let info = store.put(&bytes).unwrap();
    assert_eq!(info.name, "make-snake-game");
    assert_eq!(info.bytes, bytes.len());
    assert!(dir
        .join("blobs")
        .join(format!("{}.mpkg", info.blob))
        .is_file());
    assert!(dir.join("index").join("make-snake-game.json").is_file());

    // 幂等：同内容重复 put → 同 blob 地址
    let again = store.put(&bytes).unwrap();
    assert_eq!(again.blob, info.blob);
    assert_eq!(again.id, info.id);

    let reg = full_registry();

    // get by hash（T0 直通）：返回内容与元数据
    let outcome = execute(
        handler_of(&reg, "mpkg-get"),
        serde_json::json!({ "dir": dir.to_str().unwrap(), "hash": info.blob }),
    );
    let got = outcome.result.expect("T0 取包必须放行");
    assert_eq!(got["name"], "make-snake-game");
    assert_eq!(got["version"], "0.1.0");
    assert_eq!(got["manifest"]["intent"], "回放后得到可玩的贪吃蛇");
    assert_eq!(got["id"].as_str().unwrap(), info.id);
    assert_eq!(
        got["files"]["artifacts/snake.py"].as_str().unwrap(),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );

    // get by name：同一份包
    let outcome = execute(
        handler_of(&reg, "mpkg-get"),
        serde_json::json!({ "dir": dir.to_str().unwrap(), "name": "make-snake-game" }),
    );
    assert_eq!(outcome.result.unwrap()["id"].as_str().unwrap(), info.id);

    // get 支持带 sha256: 前缀的哈希写法（--hash 定位的是存储层 blob 地址）
    let outcome = execute(
        handler_of(&reg, "mpkg-get"),
        serde_json::json!({
            "dir": dir.to_str().unwrap(),
            "hash": format!("sha256:{}", info.blob),
        }),
    );
    assert_eq!(outcome.result.unwrap()["blob"].as_str().unwrap(), info.blob);

    // verify（T0 直通）：完整性通过 + 遥测上报验证结果
    let outcome = execute(
        handler_of(&reg, "mpkg-verify"),
        serde_json::json!({ "dir": dir.to_str().unwrap(), "hash": info.blob }),
    );
    let report = outcome.result.expect("完整包必须通过校验");
    assert_eq!(report["ok"], true);
    assert_eq!(report["id"].as_str().unwrap(), info.id);
    assert_eq!(report["files"].as_u64(), Some(1));
    assert!(outcome.events.iter().any(|e| matches!(
        e,
        Progress::Telemetry { key, value }
            if key == "verify.ok" && value.as_bool() == Some(true)
    )));

    // verify by name：顺带用名字索引里的 content-id 做二次比对
    let outcome = execute(
        handler_of(&reg, "mpkg-verify"),
        serde_json::json!({ "dir": dir.to_str().unwrap(), "name": "make-snake-game" }),
    );
    assert_eq!(outcome.result.unwrap()["ok"], true);
}

#[test]
fn get_and_verify_reject_missing_inputs() {
    let tmp = tempfile::tempdir().unwrap();
    let reg = full_registry();

    // hash / name 都缺 → 参数错误
    let outcome = execute(
        handler_of(&reg, "mpkg-get"),
        serde_json::json!({ "dir": tmp.path().to_str().unwrap() }),
    );
    let err = outcome.result.unwrap_err();
    assert!(matches!(err, AppError::InvalidArg(_)), "{err}");

    // 哈希格式非法
    let outcome = execute(
        handler_of(&reg, "mpkg-verify"),
        serde_json::json!({ "dir": tmp.path().to_str().unwrap(), "hash": "zz" }),
    );
    assert!(matches!(
        outcome.result.unwrap_err(),
        AppError::InvalidArg(_)
    ));

    // 名字不存在
    let outcome = execute(
        handler_of(&reg, "mpkg-get"),
        serde_json::json!({ "dir": tmp.path().to_str().unwrap(), "name": "no-such-pack" }),
    );
    let err = outcome.result.unwrap_err();
    assert!(err.to_string().contains("名字索引不存在"), "{err}");
}

// ── 安全门 + 遥测（T2 写路径） ────────────────────────────

#[test]
fn put_blocked_by_default_policy_before_any_write() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("store");
    let pack_file = tmp.path().join("snake.mpkg.json");
    std::fs::write(&pack_file, sample_pack_bytes()).unwrap();

    let mut reg = Registry::new(); // 默认 DenyElevated
    reg.register(RegisteredCommand::from_app::<MpkgPut>())
        .unwrap();

    let outcome = execute(
        handler_of(&reg, "mpkg-put"),
        serde_json::json!({
            "dir": dir.to_str().unwrap(),
            "file": pack_file.to_str().unwrap(),
        }),
    );
    // T2：自动化面默认拒绝，且拒绝发生在写盘之前（门在 handler 外层，dir 从未被创建）
    let err = outcome.result.unwrap_err();
    assert!(matches!(err, AppError::Safety(_)), "{err}");
    assert!(err.to_string().contains("mpkg-put"), "{err}");
    assert!(!dir.exists(), "安全门拒绝必须发生在任何写盘之前");
}

/// 能力令牌策略：T2 命令须携带正确 token 才放行（P0 SafetyPolicy 扩展点实战）
struct TokenPolicy {
    token: &'static str,
}

impl SafetyPolicy for TokenPolicy {
    fn check(&self, req: GateRequest<'_>) -> GateDecision {
        match req.tier {
            SafetyTier::ReadOnly => GateDecision::Allow,
            SafetyTier::Token => {
                let ok = req.args.get("token").and_then(|v| v.as_str()) == Some(self.token);
                if ok {
                    GateDecision::Allow
                } else {
                    GateDecision::Deny("能力令牌缺失或不匹配".into())
                }
            }
            other => GateDecision::Deny(format!("分级 {} 在此面不允许", other.tag())),
        }
    }
}

#[test]
fn put_with_token_writes_and_reports_telemetry() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("store");
    let pack_file = tmp.path().join("snake.mpkg.json");
    let bytes = sample_pack_bytes();
    std::fs::write(&pack_file, &bytes).unwrap();

    let mut reg = Registry::new().with_policy(Arc::new(TokenPolicy {
        token: "MPKG-TOKEN-42",
    }));
    reg.register(RegisteredCommand::from_app::<MpkgPut>())
        .unwrap();
    let h = handler_of(&reg, "mpkg-put");

    // 无 token → 门拒
    let denied = execute(
        h.clone(),
        serde_json::json!({ "dir": dir.to_str().unwrap(), "file": pack_file.to_str().unwrap() }),
    );
    assert!(
        matches!(denied.result, Err(AppError::Safety(_))),
        "{:?}",
        denied.result
    );

    // 错 token → 门拒
    let bad = execute(
        h.clone(),
        serde_json::json!({
            "dir": dir.to_str().unwrap(),
            "file": pack_file.to_str().unwrap(),
            "token": "WRONG",
        }),
    );
    assert!(matches!(bad.result, Err(AppError::Safety(_))));

    // 对 token → 放行：内容寻址落盘 + 遥测上报 pack bytes
    let ok = execute(
        h,
        serde_json::json!({
            "dir": dir.to_str().unwrap(),
            "file": pack_file.to_str().unwrap(),
            "token": "MPKG-TOKEN-42",
        }),
    );
    let r = ok.result.expect("正确 token 必须放行");
    assert_eq!(r["name"], "make-snake-game");
    assert_eq!(r["bytes"].as_u64(), Some(bytes.len() as u64));
    assert!(r["id"].as_str().unwrap().starts_with("sha256:"));
    assert!(dir
        .join("blobs")
        .join(format!("{}.mpkg", r["blob"].as_str().unwrap()))
        .is_file());
    assert!(ok.events.iter().any(|e| matches!(
        e,
        Progress::Telemetry { key, value }
            if key == "pack.bytes" && value.as_u64() == Some(bytes.len() as u64)
    )));
    assert!(ok.events.iter().any(|e| matches!(
        e,
        Progress::Telemetry { key, value }
            if key == "pack.name" && value.as_str() == Some("make-snake-game")
    )));
}

// ── 篡改检测 ──────────────────────────────────────────────

#[test]
fn verify_detects_tampered_blob_and_reports_telemetry() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("store");
    let store = Store::new(&dir);
    let info = store.put(&sample_pack_bytes()).unwrap();

    // 原地篡改 blob（文件名/地址不变，内容被换掉）
    let blob_path = dir.join("blobs").join(format!("{}.mpkg", info.blob));
    std::fs::write(&blob_path, b"{\"manifest\":{\"mpkg\":\"0.9\"}}").unwrap();

    let reg = full_registry();
    let outcome = execute(
        handler_of(&reg, "mpkg-verify"),
        serde_json::json!({ "dir": dir.to_str().unwrap(), "hash": info.blob }),
    );
    // 校验失败：Err + verify.ok=false 遥测（Agent 实时看到完整性被破坏）
    let err = outcome.result.unwrap_err();
    assert!(
        err.to_string().contains("完整性校验失败"),
        "篡改必须被拒: {err}"
    );
    assert!(outcome.events.iter().any(|e| matches!(
        e,
        Progress::Telemetry { key, value }
            if key == "verify.ok" && value.as_bool() == Some(false)
    )));
}

#[test]
fn verify_by_name_catches_swapped_index_id() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("store");
    let store = Store::new(&dir);
    store.put(&sample_pack_bytes()).unwrap();

    // 换掉名字索引里的 content-id（索引与内容不一致）
    let idx = dir.join("index").join("make-snake-game.json");
    let mut entry: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&idx).unwrap()).unwrap();
    entry["id"] = serde_json::json!(
        "sha256:0000000000000000000000000000000000000000000000000000000000000000"
    );
    std::fs::write(&idx, serde_json::to_vec_pretty(&entry).unwrap()).unwrap();

    let reg = full_registry();
    let outcome = execute(
        handler_of(&reg, "mpkg-verify"),
        serde_json::json!({ "dir": dir.to_str().unwrap(), "name": "make-snake-game" }),
    );
    let err = outcome.result.unwrap_err();
    assert!(err.to_string().contains("content-id"), "{err}");
}
