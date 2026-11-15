use lilyco_native_ui::render::COVERED;
use lilyco_ultra_ui::spec::{ElementSpec, UiSpec};

/// spec.rs 中 ElementSpec 的全部变体（type_name() 的取值集合）。
/// 新增变体时 spec.rs 的 type_name() 与 render.rs 的 match 都是穷尽的，
/// 编译器会同时报错；本测试再核对原生后端覆盖清单没有漏项或多余项。
const ALL: &[&str] = &[
    "文本",
    "标题",
    "按钮",
    "输入框",
    "数字框",
    "选择框",
    "复选框",
    "文本域",
    "图片",
    "分隔线",
    "进度条",
    "链接",
    "计算器",
];

#[test]
fn native_renderer_declares_every_variant() {
    let mut declared = COVERED.to_vec();
    declared.sort();
    declared.dedup();
    assert_eq!(declared.len(), COVERED.len(), "COVERED 有重复项");
    let mut all = ALL.to_vec();
    all.sort();
    all.dedup();
    assert_eq!(declared, all, "原生渲染器覆盖清单与 ElementSpec 变体不一致");
}

#[test]
fn one_spec_feeds_both_backends() {
    let spec = UiSpec::from_json(&lilyco_ultra_ui::spec::default_example_json()).unwrap();
    assert!(spec.validate().is_empty());

    // 后端 A：既有 React 生成器
    let html = lilyco_ultra_ui::generator::generate_react_html(&spec);
    assert!(html.contains("开始压缩"));
    assert!(html.contains("case '计算器'"), "React 生成器缺少计算器分支");

    // 后端 B：原生渲染器声明覆盖每一种元素类型（穷尽 match 由编译器保证）
    for el in &spec.window.elements {
        assert!(
            COVERED.contains(&el.type_name()),
            "未覆盖: {}",
            el.type_name()
        );
    }
}

#[test]
fn calculator_no_longer_drifts() {
    let spec = UiSpec::from_json(&lilyco_ultra_ui::spec::calculator_example_json()).unwrap();
    assert!(spec.validate().is_empty());
    assert!(
        spec.window
            .elements
            .iter()
            .any(|e| matches!(e, ElementSpec::Calculator { .. }))
    );
    assert!(COVERED.contains(&"计算器"));

    // React 后端以前会落到「未知元素类型」分支，现在有真实节点
    let html = lilyco_ultra_ui::generator::generate_react_html(&spec);
    assert!(html.contains("el-calc"), "React 后端仍漏掉计算器变体");
}

/// 同一份 JSON 可以同时被两个后端消费，且计算器示例不再被任一后端漏掉。
#[test]
fn both_examples_are_valid_specs() {
    for json in [
        lilyco_ultra_ui::spec::default_example_json(),
        lilyco_ultra_ui::spec::calculator_example_json(),
    ] {
        let spec = UiSpec::from_json(&json).unwrap();
        assert!(spec.validate().is_empty());
        let round = spec.to_json_pretty();
        assert!(UiSpec::from_json(&round).is_ok());
    }
}
