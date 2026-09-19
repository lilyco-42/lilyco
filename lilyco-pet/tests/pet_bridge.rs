//! lilyco-pet 测试：ValueEnum 变体 / schema 断言 / 安全门控行为（全部离线可跑）
//!
//! 网络路径（真实 EdgeTTS 合成）依赖外网 + 微软 DRM 时钟窗口（Sec-MS-GEC，
//! 5 分钟粒度），一律标 `#[ignore]`——CI 默认跳过，需要验证时手动：
//!
//! ```bash
//! cargo test -p lilyco-pet -- --ignored
//! ```

use lilyco::prelude::*;
use lilyco_core::safety::{GateDecision, GateRequest, SafetyTier};
use lilyco_pet::{edge_tts, PetAct, PetAction, PetSay};

fn handler_of(reg: &Registry, name: &str) -> Handler {
    reg.get(name).unwrap().handler.clone().unwrap()
}

// ── ValueEnum：动作词表与 pet 源码事件面对齐 ─────────────────

#[test]
fn pet_action_variants_align_with_pet_vocabulary() {
    // 变体清单锁定：pet 侧 IPC 协议定稿前，这里就是跨仓契约（漂移即测试失败）
    assert_eq!(
        PetAction::variants(),
        vec![
            "face_default",
            "face_smile",
            "face_confused",
            "face_surprised",
            "face_troubled",
            "face_angry",
            "face_childish",
            "face_gloomy",
            "face_furious",
            "toggle_dress",
            "toggle_diff",
            "quick_speak",
        ]
    );
}

#[test]
fn pet_action_name_method_matches_variants_list() {
    for v in PetAction::variants() {
        let a = PetAction::from_str(v).unwrap_or_else(|| panic!("变体 {v} 应可解析"));
        assert_eq!(a.name(), v, "name() 与 ValueEnum 规范名不一致");
    }
}

#[test]
fn pet_action_from_str_rejects_unknown() {
    assert!(PetAction::from_str("fly_to_moon").is_none());
    assert!(PetAction::from_str("").is_none());
    // 大小写敏感：与 lilyco ValueEnum 派生语义一致（snake_case 精确匹配）
    assert!(PetAction::from_str("Face_Smile").is_none());
}

#[test]
fn face_and_keypad_mapping_matches_pet_face_id_table() {
    // pet/src/app.rs: `const FACES = ["01","03","04","13","14","19","21","02","20"]`，
    // face_id(idx) 按 0 起下标取表；此处复刻该表验证 keypad 反查一致
    const FACES: [&str; 9] = ["01", "03", "04", "13", "14", "19", "21", "02", "20"];
    for v in PetAction::variants() {
        let a = PetAction::from_str(v).unwrap();
        match a.face() {
            Some(face) => {
                let key = a.keypad().expect("表情动作必有数字键序号");
                let idx = (key - 1) as usize;
                assert!(idx < FACES.len(), "keypad {key} 越界");
                assert_eq!(FACES[idx], face, "{v}: keypad {key} 应映射到 face {face}");
            }
            None => assert!(a.keypad().is_none(), "非表情动作不应有 keypad"),
        }
    }
}

// ── schema 断言：pet-say / pet-act 签名与安全分级 ────────────

#[test]
fn pet_say_schema_marks_t0_and_expected_args() {
    let s = PetSay::schema();
    assert_eq!(s.name, "pet-say");
    assert_eq!(s.safety, SafetyTier::ReadOnly, "pet-say 必须标 T0");

    let text = s.args.iter().find(|a| a.name == "text").expect("text 参数");
    assert!(text.required, "text 必填");
    assert!(matches!(text.kind, lilyco_core::schema::ArgKind::Text));

    let voice = s
        .args
        .iter()
        .find(|a| a.name == "voice")
        .expect("voice 参数");
    assert!(!voice.required);
    // 默认音色与 edge_tts::DEFAULT_VOICE 常量锁同步（宏属性只收字面量）
    assert_eq!(
        voice.default,
        Some(serde_json::json!(edge_tts::DEFAULT_VOICE))
    );

    let rate = s.args.iter().find(|a| a.name == "rate").expect("rate 参数");
    assert!(!rate.required);
    assert_eq!(rate.default, Some(serde_json::json!("+0%")));

    let output = s
        .args
        .iter()
        .find(|a| a.name == "output")
        .expect("output 参数");
    assert!(!output.required, "output 缺省落临时目录");
}

#[test]
fn pet_act_schema_marks_t0_and_enum_args() {
    let s = PetAct::schema();
    assert_eq!(s.name, "pet-act");
    assert_eq!(s.safety, SafetyTier::ReadOnly, "pet-act 必须标 T0");

    let action = s
        .args
        .iter()
        .find(|a| a.name == "action")
        .expect("action 参数");
    assert!(action.required, "action 必填");
    match &action.kind {
        lilyco_core::schema::ArgKind::Enum { values } => {
            assert!(values.contains(&"face_smile".to_string()));
            assert!(values.contains(&"toggle_dress".to_string()));
            assert_eq!(values.len(), PetAction::variants().len());
        }
        other => panic!("action 应为 Enum kind，实际 {other:?}"),
    }

    let intensity = s
        .args
        .iter()
        .find(|a| a.name == "intensity")
        .expect("intensity 参数");
    assert!(!intensity.required, "intensity 可选");
    assert!(matches!(
        intensity.kind,
        lilyco_core::schema::ArgKind::Number { .. }
    ));
}

// ── 门控行为：默认策略（DenyElevated）放行 T0，且不触网 ──────

#[test]
fn default_gate_allows_pet_act_end_to_end_without_network() {
    let mut reg = Registry::new(); // 默认 DenyElevated：T0 直通
    reg.register(RegisteredCommand::from_app::<PetAct>())
        .unwrap();

    let outcome = execute(
        handler_of(&reg, "pet-act"),
        serde_json::json!({ "action": "face_smile", "intensity": 1.5 }),
    );
    let result = outcome.result.expect("T0 动作指令必须被默认策略放行");
    assert_eq!(result["status"], "ok");
    assert_eq!(result["action"], "face_smile");
    assert_eq!(result["intensity"], 1.5);
    // 结构化指令：微笑 = face 02，数字键 8
    assert_eq!(result["instruction"]["event"], "Face");
    assert_eq!(result["instruction"]["face"], "02");
    assert_eq!(result["instruction"]["keypad"], 8);

    // P1 遥测：动作名上报
    assert!(outcome.events.iter().any(|e| matches!(
        e,
        Progress::Telemetry { key, value }
            if key == "pet.action" && value == "face_smile"
    )));
}

#[test]
fn pet_act_intensity_defaults_and_clamps() {
    let mut reg = Registry::new();
    reg.register(RegisteredCommand::from_app::<PetAct>())
        .unwrap();

    // 缺省强度 → 1.0
    let r = execute(
        handler_of(&reg, "pet-act"),
        serde_json::json!({ "action": "toggle_dress" }),
    )
    .result
    .expect("非表情动作也应放行");
    assert_eq!(r["intensity"], 1.0);
    assert_eq!(r["instruction"]["event"], "ToggleDress");

    // 越界强度 → 夹到 0.0..=2.0
    let r = execute(
        handler_of(&reg, "pet-act"),
        serde_json::json!({ "action": "quick_speak", "intensity": 99.0 }),
    )
    .result
    .expect("quick_speak 应放行");
    assert_eq!(r["intensity"], 2.0);
}

#[test]
fn pet_say_passes_gate_but_fails_on_empty_text_without_network() {
    let mut reg = Registry::new();
    reg.register(RegisteredCommand::from_app::<PetSay>())
        .unwrap();

    // 空 text：错误必须是参数校验（InvalidArg）而非安全门拒绝（Safety）——
    // 证明 DenyElevated 放行了 T0，且全程未发起网络请求
    let outcome = execute(handler_of(&reg, "pet-say"), serde_json::json!({}));
    let err = outcome.result.unwrap_err();
    assert!(
        !matches!(err, AppError::Safety(_)),
        "T0 命令不得被默认门拒绝: {err}"
    );
    assert!(matches!(err, AppError::InvalidArg(_)), "{err}");
}

#[test]
fn invalid_action_rejected_as_arg_error_not_gate_error() {
    let mut reg = Registry::new();
    reg.register(RegisteredCommand::from_app::<PetAct>())
        .unwrap();

    let outcome = execute(
        handler_of(&reg, "pet-act"),
        serde_json::json!({ "action": "fly_to_moon" }),
    );
    let err = outcome.result.unwrap_err();
    assert!(!matches!(err, AppError::Safety(_)), "{err}");
    assert!(matches!(err, AppError::InvalidArg(_)), "{err}");
    assert!(err.to_string().contains("fly_to_moon"), "{err}");
}

#[test]
fn default_policy_still_denies_elevated_tiers() {
    // 健全性检查：门本身仍在——T2 探针被默认策略拒绝
    let reg = Registry::new();
    let probe = GateRequest {
        command: "pet-hypothetical-t2",
        tier: SafetyTier::Token,
        args: &serde_json::json!({}),
    };
    assert!(matches!(reg.policy().check(probe), GateDecision::Deny(_)));
}

// ── 真实网络合成（手动验证：cargo test -p lilyco-pet -- --ignored）──

#[test]
#[ignore = "需要网络 + 微软 DRM 时钟窗口（Sec-MS-GEC 5 分钟），CI 默认跳过"]
fn real_edge_tts_synthesizes_mp3_bytes() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let audio = edge_tts::synthesize(
        "哥哥，丛雨在这里哦。",
        edge_tts::DEFAULT_VOICE,
        "+0%",
        "+0Hz",
    )
    .expect("EdgeTTS 真实合成");
    assert!(audio.len() > 1000, "mp3 字节过小: {}", audio.len());
}

#[test]
#[ignore = "需要网络 + 微软 DRM 时钟窗口（Sec-MS-GEC 5 分钟），CI 默认跳过"]
fn real_pet_say_run_writes_file_and_reports_telemetry() {
    let mut reg = Registry::new();
    reg.register(RegisteredCommand::from_app::<PetSay>())
        .unwrap();

    let out = std::env::temp_dir()
        .join("lilyco-pet")
        .join(format!("pet_say_test_{}.mp3", std::process::id()));
    let outcome = execute(
        handler_of(&reg, "pet-say"),
        serde_json::json!({
            "text": "测试：丛雨语音桥已接通。",
            "output": out.to_string_lossy(),
        }),
    );
    let result = outcome.result.expect("真实合成应成功");
    assert_eq!(result["status"], "ok");

    // 产物落盘且字节数一致
    let written = std::fs::metadata(&out).expect("mp3 应已写出").len();
    assert!(written > 1000, "mp3 过小: {written}");
    assert_eq!(result["bytes"], serde_json::json!(written));
    assert_eq!(
        result["estimate_seconds"],
        edge_tts::estimate_seconds(written as usize)
    );

    // 遥测：voice / bytes / seconds 三点齐全
    for key in ["pet.say.voice", "pet.say.bytes", "pet.say.seconds"] {
        assert!(
            outcome
                .events
                .iter()
                .any(|e| matches!(e, Progress::Telemetry { key: k, .. } if k == key)),
            "缺少遥测点 {key}"
        );
    }

    let _ = std::fs::remove_file(&out);
}
