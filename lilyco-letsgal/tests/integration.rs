//! 集成测试：DSL 解析 → 工程写入 → 校验（对齐 Node 版语义 + 确定性验收）
use lilyco_letsgal::{
    init_project, parse_story, stable_id, validate_project, write_chapters, Story,
};
use tempfile::tempdir;

/// Story 未派生 Serialize（字段全是 Vec<Value>），测试就地组装 Value——
/// 不为测试给 lib 加依赖面。与 to_value(&story) 语义等价。
fn story_json(s: &Story) -> serde_json::Value {
    serde_json::json!({ "chapters": s.chapters, "characters": s.characters, "scenes": s.scenes })
}

const DEMO_DSL: &str = r#"
# 序章
!scene 雪域 backgrounds/户外雪地.png
!bgm bgm/雪之华.mp3 vol=45 loop
穗(微笑): 我们还会再见的，对吧？
旁白：时间的河流缓缓流向彼方。
!wait 800
!curtain close dur=400

# 终章
!scene 路演 backgrounds/BG02323A.jpg
投资人：有什么独特核心技术么？
!choice 讲框架 -> qa1 | 讲案例 -> qa2

## qa1
我：我们通过 lilyco 框架开发。
## qa2
我：我们把医院系统 CLI 化。
"#;

/// 递归剥掉随机 id 字段（块 id / effectId / branchId 与 Node 版一样是随机的，
/// 不参与确定性对比）。branchId 在 props 里，递归会进 props 对象层所以同法可剥。
fn strip_ids(v: &mut serde_json::Value) {
    match v {
        serde_json::Value::Object(m) => {
            m.remove("id");
            m.remove("effectId");
            m.remove("branchId");
            for child in m.values_mut() {
                strip_ids(child);
            }
        }
        serde_json::Value::Array(a) => {
            for child in a {
                strip_ids(child);
            }
        }
        _ => {}
    }
}

#[test]
fn dsl_parse_and_build() {
    let story = parse_story(DEMO_DSL);
    assert_eq!(story.chapters.len(), 2);
    assert_eq!(story.characters.len(), 3, "穗/投资人/我 应自动注册");

    // 章节结构
    let ch0 = &story.chapters[0];
    assert_eq!(ch0["name"], "序章");
    let blocks = ch0["fragments"][0]["blocks"].as_array().unwrap();
    assert!(blocks.iter().any(|b| b["type"] == "scene"));
    assert!(blocks.iter().any(|b| b["type"] == "sound"));

    // 角色流：序章 穗 末句退场
    let dlg: Vec<&serde_json::Value> = blocks.iter().filter(|b| b["type"] == "dialogue").collect();
    assert_eq!(dlg.len(), 1);
    assert_eq!(dlg[0]["props"]["keepCharacter"], false);

    // 终章分支：fragmentId 解析为 qa1 的**确定性** fragment id
    let ch1 = &story.chapters[1];
    let qa1_id = ch1["fragments"][1]["id"].as_str().unwrap();
    let br = ch1["fragments"][0]["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["type"] == "branch")
        .unwrap();
    let opts: Vec<serde_json::Value> =
        serde_json::from_str(br["props"]["optionsJson"].as_str().unwrap()).unwrap();
    assert_eq!(opts.len(), 2);
    assert_eq!(
        opts[0]["fragmentId"].as_str().unwrap(),
        qa1_id,
        "choice 的目标必须解析为「章节名::片段名」的 stable id"
    );
}

#[test]
fn project_roundtrip() {
    let dir = tempdir().unwrap();
    init_project(dir.path(), "测试").unwrap();
    let story = parse_story(DEMO_DSL);
    for c in &story.characters {
        let _ = lilyco_letsgal::upsert_character(dir.path(), c["name"].as_str().unwrap());
    }
    for s in &story.scenes {
        let _ = lilyco_letsgal::upsert_scene(dir.path(), s["name"].as_str().unwrap());
    }
    write_chapters(dir.path(), &story.chapters, Some("测试")).unwrap();

    let (issues, warnings) = validate_project(dir.path()).unwrap();
    assert!(issues.is_empty(), "结构问题应为空: {issues:?}");
    // 资产未登记 → warnings（非致命）
    assert!(!warnings.is_empty());

    // project.json 章节顺序
    let project: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.path().join("project.json")).unwrap())
            .unwrap();
    assert_eq!(project["chapterOrder"].as_array().unwrap().len(), 2);
}

// ---------- 确定性 / Node 对齐验收（任务 #21 ③） ----------

/// 黄金向量由 Node letsgal-ai 同算法（md5("{ns}:{name}")，lib/project.js 的
/// CHAPTER_NS/FRAGMENT_NS 常量）独立计算。注意：D:/Code/gal/groundtruth 的 id
/// 是随机 uuid 时代的旧基准，**不作**逐字 id 对比；对齐基准 = Node 现源码。
#[test]
fn stable_id_matches_node_reference_vectors() {
    assert_eq!(
        stable_id("letsgal-ai:chapter", "序章"),
        "6ec1ac53-32a7-4e3d-8b52-bbef1c58b298"
    );
    assert_eq!(
        stable_id("letsgal-ai:fragment", "序章::main"),
        "beaaf751-9574-44e2-8203-a144181c6c21"
    );
    assert_eq!(
        stable_id("letsgal-ai:character", "穗"),
        "c473a4d4-645a-4e41-8000-71612d877cb2"
    );
    assert_eq!(
        stable_id("letsgal-ai:scene", "河边草地"),
        "28296c3b-7869-4330-88e6-462d659ba671"
    );
}

/// 同一 DSL 两次解析：chapter/fragment/character/scene 的 id 必须确定
/// （曾经 chapter/fragment 用 uid() 时间戳随机 —— 跨次构建 id 全漂，
/// choice/call 的引用锚点也就不稳定），只有块级 id（与 Node 同为随机）除外。
#[test]
fn same_dsl_parses_deterministically() {
    let a = parse_story(DEMO_DSL);
    let b = parse_story(DEMO_DSL);
    let mut va = story_json(&a);
    let mut vb = story_json(&b);
    strip_ids(&mut va);
    strip_ids(&mut vb);
    assert_eq!(va, vb, "同输入两次解析，除块 id 外必须逐字一致");
    // 章节锚点显式对账：id == stable_id(CHAPTER_NS, 章节名)
    let want = stable_id("letsgal-ai:chapter", "终章");
    assert_eq!(a.chapters[1]["id"].as_str().unwrap(), want);
}

/// 真实手稿端到端（任务 #21 ①）：demo.txt 的句子逐字 DSL 化（fixture 是
/// 真实生产者文件）→ build → validate 零 issues → 两次构建确定性。
#[test]
fn demo_story_builds_end_to_end_with_zero_issues() {
    let dsl = include_str!("fixtures/demo-story.txt");
    let story = parse_story(dsl);
    assert_eq!(story.chapters.len(), 2, "序章/清晨");
    let names: Vec<&str> = story
        .characters
        .iter()
        .filter_map(|c| c["name"].as_str())
        .collect();
    // 「心理」是 DSL 的旁白别名（旁白|心理|n|narration）→ 进 narration 块，不注册角色
    for want in ["穗", "游戏菜单", "广告"] {
        assert!(
            names.contains(&want),
            "手稿角色 `{want}` 应自动注册: {names:?}"
        );
    }

    let dir = tempdir().unwrap();
    init_project(dir.path(), "回忆序章").unwrap();
    for c in &story.characters {
        let _ = lilyco_letsgal::upsert_character(dir.path(), c["name"].as_str().unwrap());
    }
    for s in &story.scenes {
        let _ = lilyco_letsgal::upsert_scene(dir.path(), s["name"].as_str().unwrap());
    }
    write_chapters(dir.path(), &story.chapters, Some("回忆序章")).unwrap();

    let (issues, warnings) = validate_project(dir.path()).unwrap();
    assert!(issues.is_empty(), "demo 真实剧本必须零 issues: {issues:?}");
    assert!(!warnings.is_empty(), "资产未登记应有 warnings");

    // 章节文件可读且 fragment 锚点稳定
    for ch in &story.chapters {
        let fname = format!("{}.json", ch["name"].as_str().unwrap());
        let on_disk: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(dir.path().join("chapters").join(fname)).unwrap(),
        )
        .unwrap();
        assert_eq!(
            on_disk["id"].as_str().unwrap(),
            ch["id"].as_str().unwrap(),
            "盘上章节 id 与解析树一致"
        );
    }

    // 两次构建：除块 id 外逐字一致（同 stable_id_matches_node_reference_vectors 的算法锚点）
    let again = parse_story(dsl);
    let (mut va, mut vb) = (story_json(&story), story_json(&again));
    strip_ids(&mut va);
    strip_ids(&mut vb);
    assert_eq!(va, vb, "真实手稿两次构建必须确定性");
}
