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
    use lilyco_core::schema::{ArgKind, ArgSchema, CommandSchema};
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;

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
        assert!(
            !preview_before.contains("--verbose"),
            "verbose should be off by default"
        );

        // 焦点移到 verbose (index 0)，按空格切换
        app.form.focus_index = 0;
        app.handle_event(key(KeyCode::Char(' ')));

        let preview_after = app.form.cli_preview();
        assert!(
            preview_after.contains("--verbose"),
            "verbose should be on after toggle: {preview_after}"
        );
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
        assert_eq!(
            app.form.cli_preview().contains("51"),
            true,
            "should show 51"
        );

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
        assert!(
            !preview.contains("23"),
            "default quality should be omitted: {preview}"
        );
        // codec 默认 h264，当前值即 h264，不应出现
        assert!(
            !preview.contains("h264"),
            "default codec should be omitted: {preview}"
        );
        // auto 默认 true，不应出现
        assert!(
            !preview.contains("auto"),
            "default auto=true should be omitted: {preview}"
        );
    }

    // ─── 测试 6：false 的 flag 不出现在预览里 ────────────

    #[test]
    fn cli_preview_omits_false_flags() {
        let schema = test_schema();
        let app = TuiApp::new(&schema);
        let preview = app.form.cli_preview();

        // verbose 默认 false，不应出现
        assert!(
            !preview.contains("verbose"),
            "false flag should be omitted: {preview}"
        );
    }

    // ─── 附加：测试代码片段 ─────────────────────────────

    #[test]
    fn cli_preview_is_non_empty() {
        let schema = test_schema();
        let app = TuiApp::new(&schema);
        let preview = app.form.cli_preview();
        assert!(
            preview.starts_with("demo"),
            "preview should start with command name: {preview}"
        );
        // 空必填字段不出现在预览中（用户需要先填写）
        assert_eq!(
            preview.trim(),
            "demo",
            "only command name when all defaults/empty"
        );
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

    // ─── Running 状态取消 ────────────────────────────────

    fn goto_running(app: &mut TuiApp) {
        // 直接进入 Running（模拟已确认执行）
        app.form.app_state = AppState::Running;
    }

    #[test]
    fn running_c_cancels_to_error() {
        let schema = test_schema();
        let mut app = TuiApp::new(&schema);
        goto_running(&mut app);
        app.handle_event(key(KeyCode::Char('c')));
        assert_eq!(*app.state(), AppState::Error);
    }

    #[test]
    fn running_q_cancels_to_error() {
        let schema = test_schema();
        let mut app = TuiApp::new(&schema);
        goto_running(&mut app);
        app.handle_event(key(KeyCode::Char('q')));
        assert_eq!(*app.state(), AppState::Error);
    }

    #[test]
    fn running_esc_cancels_to_error() {
        let schema = test_schema();
        let mut app = TuiApp::new(&schema);
        goto_running(&mut app);
        app.handle_event(key(KeyCode::Esc));
        assert_eq!(*app.state(), AppState::Error);
    }

    #[test]
    fn running_ctrl_c_cancels_to_error() {
        let schema = test_schema();
        let mut app = TuiApp::new(&schema);
        goto_running(&mut app);
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        app.handle_event(ctrl_c);
        assert_eq!(*app.state(), AppState::Error);
    }

    // ─── 表单校验（required / 范围 / must_exist） ────────

    #[test]
    fn enter_blocked_when_required_empty_and_message_shown() {
        let schema = test_schema();
        let mut app = TuiApp::new(&schema);
        // output（Path, required）默认空 → Enter 应被拦截并给出消息
        app.handle_event(key(KeyCode::Enter));
        assert_eq!(
            *app.state(),
            AppState::Form,
            "empty required must stay in Form"
        );
        let msg = app
            .form
            .validation_message
            .clone()
            .expect("validation message expected");
        assert!(
            msg.contains("output"),
            "message should name the field: {msg}"
        );

        // 消息渲染进 buffer
        let out = render_to_string(&app, 80, 20);
        assert!(out.contains("⚠"), "warning glyph expected: {out}");

        // 填上后 Enter 正常进入 Confirm，消息清除
        if let FieldValue::Path(v) = &mut app.fields_mut()[4].value {
            *v = "/tmp/out.png".into();
        }
        app.handle_event(key(KeyCode::Enter));
        assert_eq!(*app.state(), AppState::Confirm);
        assert!(app.form.validation_message.is_none());
    }

    #[test]
    fn enter_blocked_when_number_out_of_range() {
        let schema = test_schema();
        let mut app = TuiApp::new(&schema);
        if let FieldValue::Path(v) = &mut app.fields_mut()[4].value {
            *v = "/tmp/out.png".into();
        }
        if let FieldValue::Number(n) = &mut app.fields_mut()[3].value {
            *n = 99.0; // 上限 51
        }
        app.handle_event(key(KeyCode::Enter));
        assert_eq!(
            *app.state(),
            AppState::Form,
            "out-of-range must stay in Form"
        );
        let msg = app.form.validation_message.clone().unwrap();
        assert!(msg.contains("51"), "range error should mention max: {msg}");
    }

    #[test]
    fn number_up_down_clamped_to_schema_range() {
        let schema = test_schema();
        let mut app = TuiApp::new(&schema);
        app.form.focus_index = 3; // quality, min 0 / max 51

        if let FieldValue::Number(n) = &mut app.fields_mut()[3].value {
            *n = 51.0;
        }
        app.handle_event(key(KeyCode::Up)); // 52 → 夹回 51
        if let FieldValue::Number(n) = &app.fields()[3].value {
            assert_eq!(*n, 51.0, "up must clamp at max");
        }

        if let FieldValue::Number(n) = &mut app.fields_mut()[3].value {
            *n = 0.0;
        }
        app.handle_event(key(KeyCode::Down)); // -1 → 夹回 0
        if let FieldValue::Number(n) = &app.fields()[3].value {
            assert_eq!(*n, 0.0, "down must clamp at min");
        }
    }

    #[test]
    fn editing_clears_validation_message() {
        let schema = test_schema();
        let mut app = TuiApp::new(&schema);
        app.form.validation_message = Some("stale".into());
        app.form.focus_index = 0; // verbose flag
        app.handle_event(key(KeyCode::Char(' '))); // 值变更
        assert!(
            app.form.validation_message.is_none(),
            "edit should clear message"
        );
    }

    #[test]
    fn path_must_exist_validated() {
        let schema = test_schema();
        let mut app = TuiApp::new(&schema);
        if let FieldValue::Path(v) = &mut app.fields_mut()[4].value {
            *v = "definitely/not/exist.bin".into();
        }
        // 把 output 的 schema 换成 must_exist=true 再校验
        app.fields_mut()[4].kind = lilyco_core::schema::ArgKind::Path { must_exist: true };
        let errors = app.form.validation_errors();
        assert!(
            errors.iter().any(|e| e.contains("路径不存在")),
            "must_exist violation expected: {errors:?}"
        );
    }

    // ─── 多命令选择页（CommandSelect） ───────────────────

    fn two_schemas() -> Vec<CommandSchema> {
        vec![
            CommandSchema {
                name: "ping".into(),
                about: "问好".into(),
                args: vec![],
                subcommands: vec![],
            },
            CommandSchema {
                name: "add".into(),
                about: "加法".into(),
                args: vec![],
                subcommands: vec![],
            },
        ]
    }

    #[test]
    fn multi_command_select_renders_list() {
        let app = TuiApp::new_multi("tool", two_schemas());
        assert_eq!(*app.state(), AppState::CommandSelect);
        let out = render_to_string(&app, 80, 20);
        assert!(out.contains("tool"), "title shows app name: {out}");
        assert!(out.contains("ping"), "lists ping: {out}");
        assert!(out.contains("add"), "lists add: {out}");
    }

    #[test]
    fn multi_select_enter_moves_to_form() {
        let mut app = TuiApp::new_multi("tool", two_schemas());
        app.handle_event(key(KeyCode::Enter));
        assert_eq!(*app.state(), AppState::Form);
        assert_eq!(app.active_command.as_deref(), Some("ping"));
        assert_eq!(app.form.command_name, "ping");
    }

    #[test]
    fn multi_select_arrow_bounds() {
        let mut app = TuiApp::new_multi("tool", two_schemas());
        app.handle_event(key(KeyCode::Down));
        app.handle_event(key(KeyCode::Down)); // 2 条命令，最多到 1
        assert_eq!(app.selected_command, 1);
        app.handle_event(key(KeyCode::Up));
        assert_eq!(app.selected_command, 0);
        app.handle_event(key(KeyCode::Up)); // 顶部不再上移
        assert_eq!(app.selected_command, 0);
        app.handle_event(key(KeyCode::Down));
        app.handle_event(key(KeyCode::Enter));
        assert_eq!(app.active_command.as_deref(), Some("add"));
    }

    #[test]
    fn multi_select_esc_quits() {
        let mut app = TuiApp::new_multi("tool", two_schemas());
        let cont = app.handle_event(key(KeyCode::Esc));
        assert!(!cont);
        assert!(app.should_quit);
    }

    #[test]
    fn multi_done_returns_to_select_but_single_quits() {
        // 多命令：Done 后任意键回选择页
        let mut app = TuiApp::new_multi("tool", two_schemas());
        app.handle_event(key(KeyCode::Enter)); // → Form
        app.form.app_state = AppState::Done;
        app.handle_event(key(KeyCode::Enter));
        assert_eq!(*app.state(), AppState::CommandSelect);
        assert!(app.active_command.is_none());

        // 单命令：Done 后任意键退出
        let schema = two_schemas().remove(0);
        let mut single = TuiApp::new(&schema);
        single.form.app_state = AppState::Done;
        let cont = single.handle_event(key(KeyCode::Enter));
        assert!(!cont);
        assert!(single.should_quit);
    }

    // ─── 路径 Tab 补全 ──────────────────────────────────

    fn path_field_schema() -> CommandSchema {
        CommandSchema {
            name: "demo".into(),
            about: "demo".into(),
            args: vec![
                ArgSchema {
                    name: "input".into(),
                    about: "输入路径".into(),
                    kind: ArgKind::Path { must_exist: false },
                    required: true,
                    default: None,
                },
                ArgSchema {
                    name: "other".into(),
                    about: "其他".into(),
                    kind: ArgKind::Text,
                    required: false,
                    default: None,
                },
            ],
            subcommands: vec![],
        }
    }

    #[test]
    fn split_dir_prefix_cases() {
        use super::app::split_dir_prefix;
        assert_eq!(split_dir_prefix("src/"), ("src/".into(), "".into()));
        assert_eq!(split_dir_prefix("src/ma"), ("src/".into(), "ma".into()));
        assert_eq!(split_dir_prefix("ma"), ("".into(), "ma".into()));
        // Windows 反斜杠（Rust raw string，源码零转义）
        assert_eq!(
            split_dir_prefix(r"C:\tmp\x"),
            (r"C:\tmp\".into(), "x".into())
        );
    }

    #[test]
    fn tab_on_empty_path_navigates_but_with_input_completes() {
        let schema = path_field_schema();
        let mut app = TuiApp::new(&schema);
        // 空 Path：Tab 前进到下一字段
        app.handle_event(key(KeyCode::Tab));
        assert_eq!(app.form.focus_index, 1);

        // 回到 input，输入真实目录 → Tab 触发补全而非跳转
        app.form.focus_index = 0;
        let base = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(base.path().join("alpha")).unwrap();
        std::fs::write(base.path().join("afile.txt"), "x").unwrap();
        if let FieldValue::Path(v) = &mut app.fields_mut()[0].value {
            *v = format!("{}{}", base.path().display(), "/");
        }
        app.handle_event(key(KeyCode::Tab));
        assert_eq!(
            app.form.focus_index, 0,
            "Tab on path input must NOT navigate"
        );
        if let FieldValue::Path(v) = &app.fields()[0].value {
            let name = v.rsplit('/').next().unwrap();
            assert!(
                name == "alpha/" || name == "afile.txt",
                "first sorted candidate expected, got {v}"
            );
        }
    }

    #[test]
    fn tab_cycles_candidates() {
        let schema = path_field_schema();
        let mut app = TuiApp::new(&schema);
        let base = tempfile::tempdir().unwrap();
        std::fs::write(base.path().join("aa1.txt"), "x").unwrap();
        std::fs::write(base.path().join("aa2.txt"), "x").unwrap();
        if let FieldValue::Path(v) = &mut app.fields_mut()[0].value {
            *v = base.path().join("aa").display().to_string();
        }

        // 首次 Tab → 第一个候选；再次 Tab → 循环下一个
        app.handle_event(key(KeyCode::Tab));
        if let FieldValue::Path(v) = &app.fields()[0].value {
            assert!(v.ends_with("aa1.txt"), "first candidate: {v}");
        }
        app.handle_event(key(KeyCode::Tab));
        if let FieldValue::Path(v) = &app.fields()[0].value {
            assert!(v.ends_with("aa2.txt"), "cycled candidate: {v}");
        }
    }

    #[test]
    fn tab_on_nonexistent_dir_keeps_value() {
        let schema = path_field_schema();
        let mut app = TuiApp::new(&schema);
        if let FieldValue::Path(v) = &mut app.fields_mut()[0].value {
            *v = "definitely/not/exist/xx".into();
        }
        app.handle_event(key(KeyCode::Tab));
        if let FieldValue::Path(v) = &app.fields()[0].value {
            assert_eq!(
                v, "definitely/not/exist/xx",
                "no candidates → value unchanged"
            );
        }
    }
}
