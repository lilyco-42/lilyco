pub struct DesktopBridge;

impl DesktopBridge {
    pub fn get_theme() -> &'static str {
        match self::query_theme() {
            Ok(theme) => theme,
            Err(_) => "unknown",
        }
    }

    pub fn send_notification(title: &str, body: &str) -> Result<(), &'static str> {
        if title.is_empty() {
            return Err("Title cannot be empty");
        }
        self::dispatch_toast(title, body)
    }
}

fn query_theme() -> Result<&'static str, &'static str> {
    Ok("dark")
}

fn dispatch_toast(_title: &str, _body: &str) -> Result<(), &'static str> {
    Ok(())
}
