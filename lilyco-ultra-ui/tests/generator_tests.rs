//! React HTML 生成器的稳定性回归测试。

use lilyco_ultra_ui::{generator::generate_react_html, spec::default_example_json, UiSpec};

fn html() -> String {
    let spec = UiSpec::from_json(&default_example_json()).unwrap();
    generate_react_html(&spec)
}

#[test]
fn cdn_versions_are_pinned_with_sri() {
    let h = html();
    assert!(h.contains("react@18.3.1/umd/react.production.min.js"));
    assert!(h.contains("react-dom@18.3.1/umd/react-dom.production.min.js"));
    assert!(h.contains("@babel/standalone@7.28.4/babel.min.js"));
    assert_eq!(h.matches("integrity=\"sha384-").count(), 3);
    // 未锁版本会随 CDN 浮动，是 2026-11-14 白屏事故的根因。
    assert!(!h.contains("unpkg.com/react@18/"));
    assert!(!h.contains("@babel/standalone/babel.min.js\""));
}

#[test]
fn jsx_uses_explicit_classic_runtime() {
    let h = html();
    assert!(h.contains("Babel.registerPreset(\"lilyco-react\""));
    assert!(h.contains("runtime: \"classic\""));
    assert!(h.contains("data-presets=\"lilyco-react\""));
    assert!(!h.contains("data-presets=\"react\""));
}

#[test]
fn generator_covers_calculator_variant() {
    let h = html();
    assert!(h.contains("case '计算器'"), "生成器漏了计算器分支");
    assert!(h.contains("el-calc"));
}

#[test]
fn window_size_maps_to_css_class() {
    // 生成器曾直接把中文字段拼进 class（"window 大"），而样式表只定义了
    // .window.large 等英文类，导致窗口尺寸样式完全失效。
    let h = html();
    assert!(h.contains("SIZE[w.大小]"), "生成器缺少中文大小到 CSS class 的映射");
    assert!(h.contains("大: 'large'"), "映射表缺少 大 → large");
    assert!(h.contains("小: 'small'"));
    assert!(h.contains("中等: 'medium'"));
    assert!(h.contains("全屏: 'fullscreen'"));
    assert!(!h.contains("'window ' + w.大小"), "仍在直接拼接中文字段作为 class");
}
