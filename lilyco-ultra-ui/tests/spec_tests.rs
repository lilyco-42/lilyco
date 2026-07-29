//! Spec 解析和生成测试

use lilyco_ultra_ui::spec::{default_example_json, ElementSpec, UiSpec, WindowSize};

#[test]
fn parse_default_example() {
    let json = default_example_json();
    let spec = UiSpec::from_json(&json).unwrap();
    assert_eq!(spec.window.title, "图片压缩工具");
    assert_eq!(spec.window.size, WindowSize::中等);
    assert_eq!(spec.window.elements.len(), 10);
}

#[test]
fn default_size_is_medium() {
    let json = r#"{"窗口":{"标题":"T","元素":[{"类型":"文本","内容":"hi"}]}}"#;
    let spec = UiSpec::from_json(json).unwrap();
    assert_eq!(spec.window.size, WindowSize::中等);
}

#[test]
fn parse_all_element_types() {
    let json = r#"{"窗口":{"标题":"All","大小":"大","元素":[
      {"类型":"文本","内容":"text"},
      {"类型":"标题","内容":"head","级别":1},
      {"类型":"按钮","文本":"btn","动作":"go","样式":"danger"},
      {"类型":"输入框","标签":"l1","变量":"v1","占位符":"p"},
      {"类型":"数字框","标签":"l2","变量":"v2","默认":50,"最小":0,"最大":100},
      {"类型":"选择框","标签":"l3","变量":"v3","选项":["a","b"],"默认":"a"},
      {"类型":"复选框","标签":"l4","变量":"v4","默认":true},
      {"类型":"文本域","标签":"l5","变量":"v5","行数":6},
      {"类型":"图片","链接":"http://x.png","宽度":"50%"},
      {"类型":"分隔线"},
      {"类型":"进度条","进度":0.7,"标签":"loading"},
      {"类型":"链接","文本":"click","链接":"http://a.com"}
    ]}}"#;
    let spec = UiSpec::from_json(json).unwrap();
    assert_eq!(spec.window.elements.len(), 12);
    assert!(matches!(spec.window.elements[0], ElementSpec::Text { .. }));
    assert!(matches!(spec.window.elements[2], ElementSpec::Button { .. }));
    assert!(matches!(spec.window.elements[9], ElementSpec::Divider));
}

#[test]
fn validate_detects_empty_title() {
    let json = r#"{"窗口":{"标题":"","元素":[{"类型":"文本","内容":"x"}]}}"#;
    let spec = UiSpec::from_json(json).unwrap();
    let errors = spec.validate();
    assert!(errors.iter().any(|e| e.contains("标题不能为空")));
}

#[test]
fn validate_detects_duplicate_vars() {
    let json = r#"{"窗口":{"标题":"T","元素":[
      {"类型":"输入框","标签":"a","变量":"dup"},
      {"类型":"输入框","标签":"b","变量":"dup"}
    ]}}"#;
    let spec = UiSpec::from_json(json).unwrap();
    let errors = spec.validate();
    assert!(errors.iter().any(|e| e.contains("重复")));
}

#[test]
fn validate_detects_empty_select_options() {
    let json = r#"{"窗口":{"标题":"T","元素":[
      {"类型":"选择框","标签":"s","变量":"v","选项":[]}
    ]}}"#;
    let spec = UiSpec::from_json(json).unwrap();
    let errors = spec.validate();
    assert!(errors.iter().any(|e| e.contains("选项列表不能为空")));
}

#[test]
fn validate_passes_for_valid_spec() {
    let spec = UiSpec::from_json(&default_example_json()).unwrap();
    assert!(spec.validate().is_empty());
}

#[test]
fn var_name_extraction() {
    let json = r#"{"窗口":{"标题":"T","元素":[
      {"类型":"输入框","标签":"l","变量":"myvar"},
      {"类型":"文本","内容":"no var"},
      {"类型":"分隔线"}
    ]}}"#;
    let spec = UiSpec::from_json(json).unwrap();
    assert_eq!(spec.window.elements[0].var_name(), Some("myvar"));
    assert_eq!(spec.window.elements[1].var_name(), None);
    assert_eq!(spec.window.elements[2].var_name(), None);
}

#[test]
fn json_roundtrip() {
    let json = default_example_json();
    let spec = UiSpec::from_json(&json).unwrap();
    let json2 = spec.to_json_pretty();
    let spec2 = UiSpec::from_json(&json2).unwrap();
    assert_eq!(spec.window.title, spec2.window.title);
    assert_eq!(spec.window.elements.len(), spec2.window.elements.len());
}

#[test]
fn window_size_css_class() {
    assert_eq!(WindowSize::小.css_class(), "small");
    assert_eq!(WindowSize::中等.css_class(), "medium");
    assert_eq!(WindowSize::大.css_class(), "large");
    assert_eq!(WindowSize::全屏.css_class(), "fullscreen");
}

#[test]
fn invalid_json_returns_error() {
    let result = UiSpec::from_json("not json");
    assert!(result.is_err());
}

#[test]
fn missing_window_field_returns_error() {
    let result = UiSpec::from_json(r#"{"foo":"bar"}"#);
    assert!(result.is_err());
}
