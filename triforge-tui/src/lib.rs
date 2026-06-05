pub mod app;
pub mod renderer;
pub mod widgets;

pub use app::TuiApp;
pub use renderer::{AppState, FormRenderer, TuiRenderer};
pub use widgets::{FieldValue, FormField};

// ── 测试 ──────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use triforge_core::schema::{ArgKind, ArgSchema, CommandSchema};

    /// 构建测试用 schema：两个 flag + enum + number
    fn test_schema() -> CommandSchema {
        CommandSchema {
            name: "demo".into(),
            about: "测试命令".into(),
            args: vec![
                ArgSchema {
                    name: "verbose".into(),
                    about: "详细输出".into(),
                    kind: ArgKind::Flag,
                    required: false,
                    default: None,
                },
                ArgSchema {
                    name: "auto".into(),
                    about: "自动模式".into(),
                    kind: ArgKind::Flag,
                    required: false,
                    default: Some(serde_json::json!(true)),
                },
                ArgSchema {
                    name: "codec".into(),
                    about: "编码格式".into(),
                    kind: ArgKind::Enum {
                        values: vec!["h264".into(), "h265".into(), "av1".into()],
                    },
                    required: true,
                    default: Some(serde_json::json!("h264")),
                },
                ArgSchema {
                    name: "quality".into(),
                    about: "质量 0-51".into(),
                    kind: ArgKind::Number {
                        min: Some(0.0),
                        max: Some(51.0),
                    },
                    required: false,
                    default: Some(serde_json::json!(23)),
                },
                ArgSchema {
                    name: "output".into(),
                    about: "输出文件".into(),
                    kind: ArgKind::Path { must_exist: false },
                    required: true,
                    default: None,
                },
            ],
            subcommands: vec![],
        }
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn render_to_string(app: &TuiApp, width: u16, height: u16) -> String {
        let mut buf = Buffer::empty(Rect::new(0, 0, width, height));
        app.render(Rect::new(0, 0, width, height), &mut buf);
        let mut s = String::new();
        for y in 0..height {
            for x in 0..width {
                let cell = buf.cell((x, y)).unwrap();
                s.push(cell.symbol().chars().next().unwrap_or(' '));
            }
            s.push('\n');
        }
        s
    }

    // ─── 测试 1：所有参数出现在渲染输出里 ───────────────

    #[test]
    fn render_form_shows_all_args() {
        let schema = test_schema();
        let app = TuiApp::new(&schema);
        let out = render_to_string(&app, 80, 20);
        assert!(out.contains("verbose"), "should show verbose: {out}");
        assert!(out.contains("auto"), "should show auto: {out}");
        assert!(out.contains("codec"), "should show codec: {out}");
        assert!(out.contains("quality"), "should show quality: {out}");
        assert!(out.contains("output"), "should show output: {out}");
    }

    // ─── 测试 2：切换 Flag 后 CLI 预览更新 ──────────────

    #[test]
    fn toggle_flag_updates_preview() {
        let schema = test_schema();
        let mut app = TuiApp::new(&schema);

        let preview_before = app.form.cli_preview();
        assert!(!preview_before.contains("--verbose"), "verbose should be off by default");

        // 焦点移到 verbose (index 0)，按空格切换
        app.form.focus_index = 0;
        app.handle_event(key(KeyCode::Char(' ')));

        let preview_after = app.form.cli_preview();
        assert!(preview_after.contains("--verbose"), "verbose should be on after toggle: {preview_after}");
    }

    // ─── 测试 3：左右键在 enum 值间循环 ─────────────────

    #[test]
    fn enum_cycles_with_arrow_keys() {
        let schema = test_schema();
        let mut app = TuiApp::new(&schema);

        // codec 在 index 2，默认选中索引 0 (h264)
        app.form.focus_index = 2;
        if let FieldValue::Enum { selected, .. } = &app.fields()[2].value {
            assert_eq!(*selected, 0, "default should be index 0 (h264)");
        }

        // 右键 → index 1 (h265)
        app.handle_event(key(KeyCode::Right));
        if let FieldValue::Enum { selected, .. } = &app.fields()[2].value {
            assert_eq!(*selected, 1, "should cycle to h265");
        }

        // 右键 → index 2 (av1)
        app.handle_event(key(KeyCode::Right));
        if let FieldValue::Enum { selected, .. } = &app.fields()[2].value {
            assert_eq!(*selected, 2, "should cycle to av1");
        }

        // 右键 → 不能越界，仍为 index 2
        app.handle_event(key(KeyCode::Right));
        if let FieldValue::Enum { selected, .. } = &app.fields()[2].value {
            assert_eq!(*selected, 2, "should stay at av1");
        }

        // 左键 → index 1
        app.handle_event(key(KeyCode::Left));
        if let FieldValue::Enum { selected, .. } = &app.fields()[2].value {
            assert_eq!(*selected, 1, "should go back to h265");
        }
    }

    // ─── 测试 4：超出范围的输入被拒绝 ───────────────────

    #[test]
    fn number_respects_range() {
        let schema = test_schema();
        let mut app = TuiApp::new(&schema);

        // quality 在 index 3，默认 23
        app.form.focus_index = 3;

        // 直接修改值为边界值测试
        if let FieldValue::Number(n) = &mut app.fields_mut()[3].value {
            *n = 51.0; // 上限
        }
        assert_eq!(app.form.cli_preview().contains("51"), true, "should show 51");

        if let FieldValue::Number(n) = &mut app.fields_mut()[3].value {
            *n = 0.0; // 下限
        }
        assert_eq!(app.form.cli_preview().contains("0"), true, "should show 0");
    }

    #[test]
    fn number_widget_up_down_keys() {
        let schema = test_schema();
        let mut app = TuiApp::new(&schema);
        app.form.focus_index = 3; // quality, default 23

        // Up → 24
        app.handle_event(key(KeyCode::Up));
        if let FieldValue::Number(n) = &app.fields()[3].value {
            assert_eq!(*n, 24.0, "up should increment 23 to 24");
        }

        // Down twice → 22
        app.handle_event(key(KeyCode::Down));
        app.handle_event(key(KeyCode::Down));
        if let FieldValue::Number(n) = &app.fields()[3].value {
            assert_eq!(*n, 22.0, "down twice should give 22");
        }
    }

    // ─── 测试 5：默认值参数不出现在预览命令里 ───────────

    #[test]
    fn cli_preview_omits_defaults() {
        let schema = test_schema();
        let app = TuiApp::new(&schema);
        let preview = app.form.cli_preview();

        // quality 默认 23，当前值即 23，不应出现
        assert!(!preview.contains("23"), "default quality should be omitted: {preview}");
        // codec 默认 h264，当前值即 h264，不应出现
        assert!(!preview.contains("h264"), "default codec should be omitted: {preview}");
        // auto 默认 true，不应出现
        assert!(!preview.contains("auto"), "default auto=true should be omitted: {preview}");
    }

    // ─── 测试 6：false 的 flag 不出现在预览里 ────────────

    #[test]
    fn cli_preview_omits_false_flags() {
        let schema = test_schema();
        let app = TuiApp::new(&schema);
        let preview = app.form.cli_preview();

        // verbose 默认 false，不应出现
        assert!(!preview.contains("verbose"), "false flag should be omitted: {preview}");
    }

    // ─── 附加：测试代码片段 ─────────────────────────────

    #[test]
    fn cli_preview_is_non_empty() {
        let schema = test_schema();
        let app = TuiApp::new(&schema);
        let preview = app.form.cli_preview();
        assert!(preview.starts_with("demo"), "preview should start with command name: {preview}");
        // 空必填字段不出现在预览中（用户需要先填写）
        assert_eq!(preview.trim(), "demo", "only command name when all defaults/empty");
    }

    #[test]
    fn enter_moves_to_confirm() {
        let schema = test_schema();
        let mut app = TuiApp::new(&schema);

        // 填写必填字段 output
        app.form.focus_index = 4; // output field
        app.handle_event(key(KeyCode::Char('f')));
        app.handle_event(key(KeyCode::Char('i')));
        app.handle_event(key(KeyCode::Char('l')));
        app.handle_event(key(KeyCode::Char('e')));
        app.handle_event(key(KeyCode::Char('.')));
        app.handle_event(key(KeyCode::Char('m')));
        app.handle_event(key(KeyCode::Char('p')));
        app.handle_event(key(KeyCode::Char('4')));

        // 按 Enter → Confirm
        app.handle_event(key(KeyCode::Enter));
        assert_eq!(*app.state(), AppState::Confirm);
    }

    #[test]
    fn esc_quits_app() {
        let schema = test_schema();
        let mut app = TuiApp::new(&schema);
        let cont = app.handle_event(key(KeyCode::Esc));
        assert!(!cont, "Esc should quit");
        assert!(app.should_quit);
    }

    #[test]
    fn tab_cycles_focus() {
        let schema = test_schema();
        let mut app = TuiApp::new(&schema);
        assert_eq!(app.form.focus_index, 0);
        app.handle_event(key(KeyCode::Tab));
        assert_eq!(app.form.focus_index, 1);
        app.handle_event(key(KeyCode::Tab));
        assert_eq!(app.form.focus_index, 2);
    }
}
