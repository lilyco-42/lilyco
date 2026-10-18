use crate::core::schema::CommandSchema;
use heapless::String;

pub enum DesktopAction {
    GetTheme,
    SendNotify { title: &'static str, body: &'static str },
}

pub struct DesktopAgentTool;

impl DesktopAgentTool {
    pub const fn schema() -> CommandSchema {
        CommandSchema::new("unidesktop_control", 128)
    }

    pub fn execute(action: DesktopAction) -> Result<String<64>, &'static str> {
        match action {
            DesktopAction::GetTheme => Self::handle_theme(),
            DesktopAction::SendNotify { title, body } => Self::handle_notify(title, body),
        }
    }

    fn handle_theme() -> Result<String<64>, &'static str> {
        let mut out = String::new();
        let _ = out.push_str("Theme: dark");
        Ok(out)
    }

    fn handle_notify(title: &str, _body: &str) -> Result<String<64>, &'static str> {
        if title.is_empty() {
            return Err("Empty title");
        }
        let mut out = String::new();
        let _ = out.push_str("Notification sent");
        Ok(out)
    }
}
