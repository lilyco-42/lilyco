//! Browser control: builds Chrome DevTools Protocol frames, runs no I/O.
//!
//! The agent's "eyes" on the web. Every request is a single JSON frame on a
//! CDP websocket, so the core stays `no_std` and the host only owns the socket.

/// Operations the agent needs from a live page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op<'a> {
    /// Load a URL and wait for the main frame.
    Navigate(&'a str),
    /// Evaluate JS and return the value by value.
    Evaluate(&'a str),
    /// Capture the viewport as PNG.
    Screenshot,
    /// Enable runtime events so console output starts streaming.
    EnableRuntime,
    /// Fetch the DOM tree as a serialisable document.
    DomSnapshot,
}

impl Op<'_> {
    /// CDP method name.
    pub const fn method(&self) -> &'static str {
        match self {
            Op::Navigate(_) => "Page.navigate",
            Op::Evaluate(_) => "Runtime.evaluate",
            Op::Screenshot => "Page.captureScreenshot",
            Op::EnableRuntime => "Runtime.enable",
            Op::DomSnapshot => "DOM.getDocument",
        }
    }
}

/// CDP event the host must subscribe to for console capture.
pub const CONSOLE_EVENT: &str = "Runtime.consoleAPICalled";

/// CDP event the host must subscribe to for navigation failures.
pub const LOAD_FAILED_EVENT: &str = "Page.loadEventFired";

/// Only these schemes may be driven by an agent.
pub fn validate_url(url: &str) -> Result<(), &'static str> {
    if url.is_empty() {
        return Err("url is empty");
    }
    let allowed = ["https://", "http://", "file://", "about:blank"];
    if allowed.iter().any(|p| url.starts_with(*p)) {
        return Ok(());
    }
    Err("url scheme is not allowed")
}

/// Render one complete CDP request frame.
pub fn render<const N: usize>(
    op: Op<'_>,
    id: u32,
    out: &mut heapless::String<N>,
) -> Result<(), &'static str> {
    use ::core::fmt::Write as _;
    match op {
        Op::Navigate(url) => {
            validate_url(url)?;
            ::core::write!(out, "{{\"id\":{},\"method\":\"Page.navigate\",\"params\":{{\"url\":\"", id)
                .map_err(|_| "buffer full")?;
            json_escape(url, out)?;
            out.push_str("\"}}").map_err(|_| "buffer full")
        }
        Op::Evaluate(js) => {
            if js.is_empty() {
                return Err("expression is empty");
            }
            ::core::write!(out, "{{\"id\":{},\"method\":\"Runtime.evaluate\",\"params\":{{\"expression\":\"", id)
                .map_err(|_| "buffer full")?;
            json_escape(js, out)?;
            out.push_str("\",\"returnByValue\":true}}").map_err(|_| "buffer full")
        }
        Op::Screenshot => {
            let head = ",\"method\":\"Page.captureScreenshot\",\"params\":{\"format\":\"png\"}}";
            ::core::write!(out, "{{\"id\":{}", id).map_err(|_| "buffer full")?;
            out.push_str(head).map_err(|_| "buffer full")
        }
        Op::EnableRuntime => {
            ::core::write!(out, "{{\"id\":{},\"method\":\"Runtime.enable\"}}", id)
                .map_err(|_| "buffer full")
        }
        Op::DomSnapshot => {
            ::core::write!(out, "{{\"id\":{},\"method\":\"DOM.getDocument\",\"params\":{{\"depth\":3}}}}", id)
                .map_err(|_| "buffer full")
        }
    }
}

fn json_escape<const N: usize>(
    src: &str,
    out: &mut heapless::String<N>,
) -> Result<(), &'static str> {
    use ::core::fmt::Write as _;
    for ch in src.chars() {
        match ch {
            '"' => out.push_str("\\\"").map_err(|_| "buffer full")?,
            '\\' => out.push_str("\\\\").map_err(|_| "buffer full")?,
            '\n' => out.push_str("\\n").map_err(|_| "buffer full")?,
            '\r' => out.push_str("\\r").map_err(|_| "buffer full")?,
            '\t' => out.push_str("\\t").map_err(|_| "buffer full")?,
            c if (c as u32) < 0x20 => {
                ::core::write!(out, "\\u{:04x}", c as u32).map_err(|_| "buffer full")?;
            }
            c => out.push(c).map_err(|_| "buffer full")?,
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(op: Op<'_>) -> heapless::String<256> {
        let mut buf = heapless::String::new();
        render(op, 7, &mut buf).unwrap();
        buf
    }

    #[test]
    fn navigate_escapes_and_wraps() {
        let got = frame(Op::Navigate("https://a.test/x?q=\"1\""));
        assert_eq!(
            got.as_str(),
            "{\"id\":7,\"method\":\"Page.navigate\",\"params\":{\"url\":\"https://a.test/x?q=\\\"1\\\"\"}}"
        );
    }

    #[test]
    fn dangerous_schemes_are_rejected() {
        assert!(validate_url("javascript:alert(1)").is_err());
        assert!(validate_url("data:text/html,hi").is_err());
        assert!(validate_url("").is_err());
        assert!(validate_url("http://localhost:3000").is_ok());
        assert!(validate_url("about:blank").is_ok());
    }

    #[test]
    fn evaluate_returns_by_value() {
        let got = frame(Op::Evaluate("document.title"));
        assert!(got.as_str().contains("\"returnByValue\":true"));
        assert!(got.as_str().starts_with("{\"id\":7,\"method\":\"Runtime.evaluate\""));
    }

    #[test]
    fn control_characters_become_escapes() {
        let got = frame(Op::Evaluate("a\nb"));
        assert!(got.as_str().contains("a\\nb"));
    }

    #[test]
    fn methods_match_the_spec() {
        assert_eq!(Op::Screenshot.method(), "Page.captureScreenshot");
        assert_eq!(Op::DomSnapshot.method(), "DOM.getDocument");
        assert_eq!(CONSOLE_EVENT, "Runtime.consoleAPICalled");
    }
}
