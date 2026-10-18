//! File engine: line windows, safe relative paths and exact-match edits.
//!
//! All functions are pure planners. They decide *what* to read or replace and
//! hand byte spans back to the host, so the same logic runs on a microcontroller
//! and on a desktop without a single `std::fs` call in the core.

/// Default number of lines a read returns.
pub const DEFAULT_READ_LINES: usize = 200;

/// Hard cap on lines per call, so one read cannot flood the context window.
pub const MAX_READ_LINES: usize = 2000;

/// Bytes of a single line rendered into an observation.
pub const MAX_LINE_BYTES: usize = 512;

/// A clamped half-open line range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window {
    pub offset: usize,
    pub limit: usize,
}

impl Window {
    /// Clamp a requested window against the real line count.
    pub fn plan(total: usize, offset: usize, limit: usize) -> Window {
        if offset >= total {
            return Window { offset: total, limit: 0 };
        }
        let want = if limit == 0 { DEFAULT_READ_LINES } else { limit };
        let cap = want.min(MAX_READ_LINES);
        let left = total - offset;
        Window {
            offset,
            limit: cap.min(left),
        }
    }

    /// True when the window selects nothing.
    pub const fn is_empty(&self) -> bool {
        self.limit == 0
    }
}

/// Count logical lines. A trailing newline does not start a new line.
pub fn count_lines(src: &str) -> usize {
    if src.is_empty() {
        return 0;
    }
    let breaks = src.bytes().filter(|b| *b == b'\n').count();
    if src.ends_with('\n') {
        breaks
    } else {
        breaks + 1
    }
}

/// Byte span of the unique match, ready to be spliced by the host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

/// Reject absolute paths, drive letters and `..` traversal.
pub fn sanitize_rel(path: &str) -> Result<&str, &'static str> {
    if path.is_empty() {
        return Err("path is empty");
    }
    let first = path.as_bytes()[0];
    if first == b'/' || first == b'\\' {
        return Err("absolute paths are not allowed");
    }
    if path.contains('\0') || path.contains(':') {
        return Err("illegal character in path");
    }
    let mut segs = path.split(['/', '\\']);
    let escape = segs.any(|s| s == ".." || s == "~");
    if escape {
        return Err("path traversal is not allowed");
    }
    Ok(path)
}

/// Locate the single exact occurrence of `needle`.
///
/// Ambiguity is an error, not a guess: a model that cannot name a unique anchor
/// must widen its context first.
pub fn plan_replace(src: &str, needle: &str) -> Result<Span, &'static str> {
    if needle.is_empty() {
        return Err("needle is empty");
    }
    let mut hits = src.match_indices(needle);
    let first = hits.next().ok_or("needle not found")?;
    if hits.next().is_some() {
        return Err("needle is ambiguous");
    }
    Ok(Span {
        start: first.0,
        end: first.0 + needle.len(),
    })
}

/// Split at a span so an allocating host can splice without copying here.
pub fn split_at(src: &str, span: Span) -> Result<(&str, &str), &'static str> {
    let head = src.get(..span.start).ok_or("span start is not a boundary")?;
    let tail = src.get(span.end..).ok_or("span end is not a boundary")?;
    Ok((head, tail))
}

/// Render `   12| code` lines for the selected window.
pub fn render_numbered<const N: usize>(
    src: &str,
    window: Window,
    out: &mut heapless::String<N>,
) -> Result<usize, &'static str> {
    use ::core::fmt::Write as _;
    if window.is_empty() {
        return Ok(0);
    }
    let last = window.offset + window.limit;
    let mut written = 0usize;
    for (idx, line) in src.split('\n').enumerate() {
        if idx < window.offset || idx >= last {
            continue;
        }
        let body = line.strip_suffix('\r').unwrap_or(line);
        ::core::writeln!(out, "{:>5}| {}", idx + 1, clip(body, MAX_LINE_BYTES))
            .map_err(|_| "buffer full")?;
        written += 1;
    }
    Ok(written)
}

fn clip(line: &str, max: usize) -> &str {
    if line.len() <= max {
        return line;
    }
    let mut end = max;
    while end > 0 && !line.is_char_boundary(end) {
        end -= 1;
    }
    line.get(..end).unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_is_clamped() {
        assert_eq!(Window::plan(10, 0, 0), Window { offset: 0, limit: 10 });
        assert_eq!(Window::plan(9000, 0, 0), Window { offset: 0, limit: DEFAULT_READ_LINES });
        assert_eq!(Window::plan(5, 3, 100), Window { offset: 3, limit: 2 });
        assert!(Window::plan(5, 9, 4).is_empty());
    }

    #[test]
    fn line_counting_handles_trailing_newline() {
        assert_eq!(count_lines(""), 0);
        assert_eq!(count_lines("a"), 1);
        assert_eq!(count_lines("a\nb\n"), 2);
        assert_eq!(count_lines("a\nb"), 2);
    }

    #[test]
    fn traversal_is_rejected() {
        assert!(sanitize_rel("../etc/passwd").is_err());
        assert!(sanitize_rel("/etc/passwd").is_err());
        assert!(sanitize_rel("C:\\win").is_err());
        assert_eq!(sanitize_rel("src/main.rs"), Ok("src/main.rs"));
    }

    #[test]
    fn replace_requires_a_unique_anchor() {
        assert_eq!(plan_replace("let a = 1;", "a = 1"), Ok(Span { start: 4, end: 9 }));
        assert!(plan_replace("aa aa", "aa").is_err());
        assert!(plan_replace("nothing", "zz").is_err());
        assert!(plan_replace("x", "").is_err());
    }

    #[test]
    fn split_preserves_both_halves() {
        let src = "hello world";
        let span = Span { start: 5, end: 6 };
        assert_eq!(split_at(src, span), Ok(("hello", "world")));
    }

    #[test]
    fn numbered_render_respects_window() {
        let src = "alpha\nbeta\ngamma\n";
        let mut buf: heapless::String<128> = heapless::String::new();
        let n = render_numbered(src, Window { offset: 1, limit: 1 }, &mut buf).unwrap();
        assert_eq!(n, 1);
        assert_eq!(buf.as_str(), "    2| beta\n");
    }

    #[test]
    fn cjk_is_not_split_mid_character() {
        let line = "中文中文中文";
        assert_eq!(clip(line, 7), "中文");
    }
}
