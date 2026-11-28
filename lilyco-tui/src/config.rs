//! 键位配置：默认键位与 `keymap.toml` 覆盖。

use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde::Deserialize;

/// 可配置的聊天动作
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyAction {
    SelectNext,
    SelectPrev,
    SelectTop,
    SelectBottom,
    HalfPageDown,
    HalfPageUp,
    Insert,
    ExitInsert,
    Send,
    Cancel,
}

/// 聊天键位表；未写的字段使用默认值。
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Keymap {
    pub select_next: Vec<String>,
    pub select_prev: Vec<String>,
    pub select_top: Vec<String>,
    pub select_bottom: Vec<String>,
    pub half_page_down: Vec<String>,
    pub half_page_up: Vec<String>,
    pub insert: Vec<String>,
    pub exit_insert: Vec<String>,
    pub send: Vec<String>,
    pub cancel: Vec<String>,
}

impl Default for Keymap {
    fn default() -> Self {
        let keys = |items: &[&str]| items.iter().map(|item| (*item).to_string()).collect();
        Self {
            select_next: keys(&["j", "down"]),
            select_prev: keys(&["k", "up"]),
            select_top: keys(&["home"]),
            select_bottom: keys(&["G"]),
            half_page_down: keys(&["ctrl+d"]),
            half_page_up: keys(&["ctrl+u"]),
            insert: keys(&["i"]),
            exit_insert: keys(&["esc"]),
            send: keys(&["enter"]),
            cancel: keys(&["q", "c", "ctrl+c", "esc"]),
        }
    }
}

impl Keymap {
    /// 按 XDG / HOME 顺序加载 keymap.toml；任何读取或解析失败都回退默认。
    pub fn load_default() -> Keymap {
        let mut paths = Vec::new();
        if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
            if !xdg.is_empty() {
                paths.push(PathBuf::from(xdg).join("lilyco-tui/keymap.toml"));
            }
        }
        if let Some(home) = std::env::var_os("HOME") {
            paths.push(PathBuf::from(home).join(".config/lilyco-tui/keymap.toml"));
        }
        for path in paths {
            if path.is_file() {
                return Self::from_path(&path);
            }
        }
        Self::default()
    }

    /// 判断按键是否匹配某个动作
    pub fn is(&self, action: KeyAction, key: KeyEvent) -> bool {
        let list = match action {
            KeyAction::SelectNext => &self.select_next,
            KeyAction::SelectPrev => &self.select_prev,
            KeyAction::SelectTop => &self.select_top,
            KeyAction::SelectBottom => &self.select_bottom,
            KeyAction::HalfPageDown => &self.half_page_down,
            KeyAction::HalfPageUp => &self.half_page_up,
            KeyAction::Insert => &self.insert,
            KeyAction::ExitInsert => &self.exit_insert,
            KeyAction::Send => &self.send,
            KeyAction::Cancel => &self.cancel,
        };
        self.matches(list, key)
    }

    /// 判断按键是否命中键名列表
    pub fn matches(&self, list: &[String], key: KeyEvent) -> bool {
        let Some(name) = key_name(key) else {
            return false;
        };
        list.iter().any(|candidate| candidate == &name)
    }

    fn from_path(path: &Path) -> Keymap {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| toml::from_str::<Keymap>(&text).ok())
            .unwrap_or_default()
    }

    #[cfg(test)]
    pub(crate) fn load_from(path: &Path) -> Keymap {
        Self::from_path(path)
    }
}

fn key_name(key: KeyEvent) -> Option<String> {
    let control = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    match key.code {
        KeyCode::Char(ch) => {
            let lower = ch.to_ascii_lowercase();
            if control && alt {
                Some(format!("ctrl+alt+{lower}"))
            } else if control {
                Some(format!("ctrl+{lower}"))
            } else if alt {
                Some(format!("alt+{lower}"))
            } else {
                Some(ch.to_string())
            }
        }
        KeyCode::Enter => Some("enter".into()),
        KeyCode::Esc => Some("esc".into()),
        KeyCode::Backspace => Some("backspace".into()),
        KeyCode::Tab => Some("tab".into()),
        KeyCode::BackTab => Some("backtab".into()),
        KeyCode::Up => Some("up".into()),
        KeyCode::Down => Some("down".into()),
        KeyCode::Left => Some("left".into()),
        KeyCode::Right => Some("right".into()),
        KeyCode::Home => Some("home".into()),
        KeyCode::End => Some("end".into()),
        KeyCode::PageUp => Some("pageup".into()),
        KeyCode::PageDown => Some("pagedown".into()),
        KeyCode::Delete => Some("delete".into()),
        KeyCode::Insert => Some("insert".into()),
        KeyCode::F(number) => Some(format!("f{number}")),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn keymap_default_matches_j_k_and_ctrl_d() {
        let map = Keymap::default();
        assert!(map.is(KeyAction::SelectNext, key(KeyCode::Char('j'))));
        assert!(map.is(KeyAction::SelectNext, key(KeyCode::Down)));
        assert!(map.is(KeyAction::SelectPrev, key(KeyCode::Char('k'))));
        assert!(map.is(KeyAction::SelectPrev, key(KeyCode::Up)));
        assert!(map.is(
            KeyAction::HalfPageDown,
            KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL)
        ));
        assert!(map.is(
            KeyAction::HalfPageUp,
            KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL)
        ));
        assert!(map.is(KeyAction::SelectBottom, key(KeyCode::Char('G'))));
        assert!(!map.is(KeyAction::SelectBottom, key(KeyCode::Char('g'))));
        assert!(!map.is(KeyAction::SelectNext, key(KeyCode::Char('k'))));
    }

    #[test]
    fn keymap_toml_overrides_default() {
        let mut file = tempfile::NamedTempFile::new().expect("临时文件");
        std::io::Write::write_all(file.as_file_mut(), b"select_next = [\"n\"]\n")
            .expect("写入 keymap");

        let map = Keymap::load_from(file.path());
        assert!(!map.is(KeyAction::SelectNext, key(KeyCode::Char('j'))));
        assert!(map.is(KeyAction::SelectNext, key(KeyCode::Char('n'))));
        assert!(map.is(KeyAction::SelectPrev, key(KeyCode::Char('k'))));
        assert!(map.is(
            KeyAction::Cancel,
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)
        ));
        drop(file);
    }
}
