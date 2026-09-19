//! mpkg golden 契约向量对齐测试（生态融合路线图 P1）
//!
//! 契约源：lystack `proto/mpkg/`（`mpkg.schema.json` + `GOLDEN.md` + `golden/cases.json`）。
//! 本文件硬编码 cases.json 的三个 golden 用例与期望哈希，断言本实现（canon /
//! content-id / blob）与契约逐字节一致。两处必须同步：改格式先改 lystack 契约，
//! 再跑 `proto/mpkg/gen_golden.py` 重新生成向量，最后回来更新本文件。
//!
//! 两层哈希（易混淆，见 GOLDEN.md §3 显式警告）：
//! - content-id = `sha256:` + sha256(canon_json(包JSON))，语义身份，对空白不敏感；
//! - blob 地址 = sha256(原始字节)，存储地址，对每字节敏感。
//! golden 输入为 canon 形式 → 两者十六进制恰好相同；非 canon 字节落盘时
//! blob 变而 content-id 不变（见末尾测试）。

use lilyco_mpkg::{canon, package_id, validate, verify_pack, Store};

// ── golden 向量（契约源 golden/cases.json；勿手改，重新生成跑 gen_golden.py） ──

const CASE1_NAME: &str = "minimal-manifest-only";
const CASE1_CANON: &str = r#"{"manifest":{"intent":"最小合法包：仅 manifest，无 files 键","mpkg":"0.1","name":"hello-mpkg","steps":[{"run":"echo hello-mpkg"}],"verify":["true"],"version":"0.1.0"}}"#;
const CASE1_ID: &str = "sha256:830d8d92aa3a7ca9655468c5fc034e10902f9aad225825fdb7e88824a7eeda58";
const CASE1_BLOB: &str = "830d8d92aa3a7ca9655468c5fc034e10902f9aad225825fdb7e88824a7eeda58";

const CASE2_NAME: &str = "multi-file-pack";
const CASE2_CANON: &str = r#"{"files":{"README.md":"fa92edd9d1241161b07fce3404e6c8cceb23dd8790521e274923db0afdc14f14","artifacts/lib/util.py":"b1b3395bd5ce1ca48ec48abae5c25b1007bd4b0762717b811742579f93fe1a83","artifacts/main.py":"e1f295db15fc563c98997b625f807baf10d885df64a8ab5a4272cde6401afbd2"},"manifest":{"intent":"含 files 的多文件包：嵌套路径哈希引用","mpkg":"0.1","name":"multi-file-demo","steps":[{"expect":{"exit":0},"run":"echo multi"}],"verify":["test -f artifacts/main.py","test -f artifacts/lib/util.py"],"version":"0.2.0"}}"#;
const CASE2_ID: &str = "sha256:f501b4282a8dcb3d44b3a1978ac0643f280fa5926e131e3f087204e7355febd7";
const CASE2_BLOB: &str = "f501b4282a8dcb3d44b3a1978ac0643f280fa5926e131e3f087204e7355febd7";

const CASE3_NAME: &str = "full-replay-suite";
const CASE3_CANON: &str = r#"{"files":{"artifacts/out.txt":"084c799cd551dd1d8d5c5f9a5d593b2e931f5e36122ee5c793c1d08a19839cc0"},"manifest":{"author":"agent:lilyco","intent":"步骤齐全包：expect.exit / 多 verify / requirements / 溯源可选字段","license":"MIT","mpkg":"0.1","name":"full-replay-suite","requirements":{"tools":[{"min_version":"3.10","name":"python3"}]},"steps":[{"run":"echo step-1"},{"expect":{"exit":0},"run":"python3 -c 'print(6*7)'"},{"expect":{"exit":0},"run":"test -d {{work}}"}],"tags":["demo","replay"],"verify":["test -f artifacts/out.txt","grep -q 42 artifacts/out.txt"],"version":"1.0.0"}}"#;
const CASE3_ID: &str = "sha256:7bc60dd2e45e99cdd8b095f36b28234899495743001001d2c1169963bcf55086";
const CASE3_BLOB: &str = "7bc60dd2e45e99cdd8b095f36b28234899495743001001d2c1169963bcf55086";

// ── golden 输入包（与 cases.json 的 pack 字段逐字段一致；canon 会重排键序） ──

/// 用例 1：最小包 —— 仅 manifest，无 files 键（files 为可选字段）
fn case1_pack() -> serde_json::Value {
    serde_json::json!({
        "manifest": {
            "mpkg": "0.1",
            "name": "hello-mpkg",
            "version": "0.1.0",
            "intent": "最小合法包：仅 manifest，无 files 键",
            "steps": [ { "run": "echo hello-mpkg" } ],
            "verify": [ "true" ]
        }
    })
}

/// 用例 2：含 files 的多文件包 —— 嵌套相对路径哈希引用 + expect.exit
fn case2_pack() -> serde_json::Value {
    serde_json::json!({
        "manifest": {
            "mpkg": "0.1",
            "name": "multi-file-demo",
            "version": "0.2.0",
            "intent": "含 files 的多文件包：嵌套路径哈希引用",
            "steps": [ { "run": "echo multi", "expect": { "exit": 0 } } ],
            "verify": [ "test -f artifacts/main.py", "test -f artifacts/lib/util.py" ]
        },
        "files": {
            "README.md": "fa92edd9d1241161b07fce3404e6c8cceb23dd8790521e274923db0afdc14f14",
            "artifacts/lib/util.py": "b1b3395bd5ce1ca48ec48abae5c25b1007bd4b0762717b811742579f93fe1a83",
            "artifacts/main.py": "e1f295db15fc563c98997b625f807baf10d885df64a8ab5a4272cde6401afbd2"
        }
    })
}

/// 用例 3：步骤齐全包 —— 多步 expect.exit / 多 verify / requirements 等可选字段
fn case3_pack() -> serde_json::Value {
    serde_json::json!({
        "manifest": {
            "mpkg": "0.1",
            "name": "full-replay-suite",
            "version": "1.0.0",
            "intent": "步骤齐全包：expect.exit / 多 verify / requirements / 溯源可选字段",
            "author": "agent:lilyco",
            "requirements": { "tools": [ { "name": "python3", "min_version": "3.10" } ] },
            "steps": [
                { "run": "echo step-1" },
                { "run": "python3 -c 'print(6*7)'", "expect": { "exit": 0 } },
                { "run": "test -d {{work}}", "expect": { "exit": 0 } }
            ],
            "verify": [ "test -f artifacts/out.txt", "grep -q 42 artifacts/out.txt" ],
            "tags": [ "demo", "replay" ],
            "license": "MIT"
        },
        "files": {
            "artifacts/out.txt": "084c799cd551dd1d8d5c5f9a5d593b2e931f5e36122ee5c793c1d08a19839cc0"
        }
    })
}

// ── 对齐断言 ──────────────────────────────────────────────

/// 统一断言：结构校验通过 + canon 逐字节一致 + content-id/blob 一致 + verify_pack 全链
fn assert_golden(
    name: &str,
    pack: &serde_json::Value,
    expected_canon: &str,
    expected_id: &str,
    expected_blob: &str,
) {
    if let Err(e) = validate(pack) {
        panic!("{name}: golden 包必须通过结构校验: {e}");
    }
    let c = canon(pack);
    assert_eq!(c, expected_canon, "{name}: canon_json 逐字节不一致");
    let id = package_id(pack);
    assert_eq!(id, expected_id, "{name}: content-id 不一致");
    // blob = sha256(canon 字节)：canon 落盘形态下与 content-id hex 相同（GOLDEN.md §3）
    assert_eq!(
        lilyco_mpkg::sha256::sha256_hex(expected_canon.as_bytes()),
        expected_blob,
        "{name}: blob 地址不一致"
    );
    // 完整性校验全链：blob 比对 + 结构校验 + content-id 重算比对
    if let Err(e) = verify_pack(
        expected_canon.as_bytes(),
        Some(expected_blob),
        Some(expected_id),
    ) {
        panic!("{name}: verify_pack 必须通过: {e}");
    }
}

#[test]
fn golden_case1_minimal_manifest_only() {
    assert_golden(CASE1_NAME, &case1_pack(), CASE1_CANON, CASE1_ID, CASE1_BLOB);
}

#[test]
fn golden_case2_multi_file_pack() {
    assert_golden(CASE2_NAME, &case2_pack(), CASE2_CANON, CASE2_ID, CASE2_BLOB);
}

#[test]
fn golden_case3_full_replay_suite() {
    assert_golden(CASE3_NAME, &case3_pack(), CASE3_CANON, CASE3_ID, CASE3_BLOB);
}

/// 两层哈希陷阱（GOLDEN.md §3）：content-id 对序列化空白不敏感，blob 是原始字节地址。
/// 同一逻辑包以 canon 字节与 pretty 字节分别 put → 同 id、不同 blob。
#[test]
fn golden_two_layer_hash_pretty_bytes_change_blob_not_id() {
    let pack = case2_pack();
    let pretty = serde_json::to_vec_pretty(&pack).unwrap();
    let canon_bytes = CASE2_CANON.as_bytes();

    let tmp = tempfile::tempdir().unwrap();
    let store = Store::new(tmp.path().join("store"));
    let a = store.put(canon_bytes).unwrap(); // canon 字节落盘
    let b = store.put(&pretty).unwrap(); // pretty 字节落盘

    assert_eq!(a.id, b.id, "content-id 对序列化空白不敏感");
    assert_eq!(a.id, CASE2_ID, "content-id 与契约 golden 不一致");
    assert_eq!(
        a.blob, CASE2_BLOB,
        "canon 落盘时 blob 与 content-id hex 相同"
    );
    assert_ne!(
        a.blob, b.blob,
        "非 canon 字节落盘 → blob 变而 content-id 不变"
    );
}
