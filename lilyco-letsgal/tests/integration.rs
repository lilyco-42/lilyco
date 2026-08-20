//! 集成测试：DSL 解析 → 工程写入 → 校验（对齐 Node 版 smoke test）
use lilyco_letsgal::{init_project, parse_story, validate_project, write_chapters};
use tempfile::tempdir;

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

    // 终章分支：fragmentId 解析为 frag- id
    let ch1 = &story.chapters[1];
    let br = ch1["fragments"][0]["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["type"] == "branch")
        .unwrap();
    let opts: Vec<serde_json::Value> =
        serde_json::from_str(br["props"]["optionsJson"].as_str().unwrap()).unwrap();
    assert_eq!(opts.len(), 2);
    assert!(opts
        .iter()
        .all(|o| o["fragmentId"].as_str().unwrap().starts_with("frag-")));
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
