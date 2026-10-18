//! Trace view-model: turns agent observations into rows the UI can paint.
//!
//! Pure data plus token lookups, so a GPUI element, a TUI cell and an HTML
//! fragment can all render the *same* row without re-deciding what "failed"
//! means.

use crate::adapters::image_inspect::{fit, Format};
use crate::agent::ooda::Phase;
use crate::agent::tool::LogStream;
use crate::ui::color::Color;
use crate::ui::theme::Palette;

/// How a row should read at a glance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Ok,
    Warn,
    Fail,
}

impl Status {
    /// Stable label for logs and tests.
    pub const fn label(self) -> &'static str {
        match self {
            Status::Ok => "ok",
            Status::Warn => "warn",
            Status::Fail => "fail",
        }
    }
}

/// The payload a row carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowBody<'a> {
    /// Free text, e.g. a read excerpt.
    Text(&'a str),
    /// A patch summary.
    Diff { added: u32, removed: u32 },
    /// Console or process output.
    Console { stream: LogStream, lines: u32 },
    /// A decoded image.
    Image {
        format: Format,
        width: u32,
        height: u32,
    },
    /// A finished process.
    Exit { code: i32 },
}

/// One line of the agent trace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TraceRow<'a> {
    /// OODA step number.
    pub step: u32,
    /// Phase the call happened in.
    pub phase: Phase,
    /// Tool name.
    pub tool: &'a str,
    /// What came back.
    pub body: RowBody<'a>,
}

impl<'a> TraceRow<'a> {
    /// Build a row.
    pub const fn new(step: u32, phase: Phase, tool: &'a str, body: RowBody<'a>) -> Self {
        TraceRow {
            step,
            phase,
            tool,
            body,
        }
    }

    /// Status is derived, never passed in: one source of truth.
    pub const fn status(&self) -> Status {
        match self.body {
            RowBody::Exit { code: 0 } => Status::Ok,
            RowBody::Exit { .. } => Status::Fail,
            RowBody::Console {
                stream: LogStream::Stderr,
                ..
            } => Status::Warn,
            RowBody::Console { .. } => Status::Ok,
            RowBody::Text(_) => Status::Ok,
            RowBody::Diff { .. } => Status::Ok,
            RowBody::Image { .. } => Status::Ok,
        }
    }

    /// `(background, foreground)` for the status dot, straight from the palette.
    pub const fn status_colors(&self, palette: &Palette) -> (Color, Color) {
        match self.status() {
            Status::Ok => (palette.success, palette.background),
            Status::Warn => (palette.warning, palette.background),
            Status::Fail => (palette.destructive, palette.destructive_foreground),
        }
    }

    /// `step 3 · act · edit_file +12 −4`
    pub fn summary<const N: usize>(
        &self,
        out: &mut heapless::String<N>,
    ) -> Result<(), &'static str> {
        use ::core::fmt::Write as _;
        ::core::write!(out, "step {} · {} · {}", self.step, self.phase.label(), self.tool)
            .map_err(|_| "buffer full")?;
        self.append_body(out)
    }

    /// Owned summary, for front-ends that need a value outliving a borrow.
    pub fn summary_buf(&self) -> heapless::String<128> {
        let mut out = heapless::String::new();
        let _ = self.summary(&mut out);
        out
    }

    fn append_body<const N: usize>(
        &self,
        out: &mut heapless::String<N>,
    ) -> Result<(), &'static str> {
        use ::core::fmt::Write as _;
        match self.body {
            RowBody::Text(text) => ::core::write!(out, " · {}", text).map_err(|_| "buffer full"),
            RowBody::Diff { added, removed } => {
                ::core::write!(out, " +{} −{}", added, removed).map_err(|_| "buffer full")
            }
            RowBody::Console { lines, .. } => {
                ::core::write!(out, " {} lines", lines).map_err(|_| "buffer full")
            }
            RowBody::Image {
                format,
                width,
                height,
            } => ::core::write!(out, " {} {}x{}", format.label(), width, height)
                .map_err(|_| "buffer full"),
            RowBody::Exit { code } => ::core::write!(out, " exit {}", code).map_err(|_| "buffer full"),
        }
    }
}

/// Count `+`/`-` lines in a unified diff, ignoring the file headers.
pub fn diff_stat(patch: &str) -> (u32, u32) {
    let mut added = 0u32;
    let mut removed = 0u32;
    for line in patch.split('\n') {
        if line.starts_with("+++") || line.starts_with("---") {
            continue;
        }
        if line.starts_with('+') {
            added = added.saturating_add(1);
        } else if line.starts_with('-') {
            removed = removed.saturating_add(1);
        }
    }
    (added, removed)
}

/// `png 1024x768 → 1568x1176`, clamped so the vision call stays predictable.
pub fn image_label<const N: usize>(
    format: Format,
    width: u32,
    height: u32,
    max_edge: u32,
    out: &mut heapless::String<N>,
) -> Result<(), &'static str> {
    use ::core::fmt::Write as _;
    let (w, h) = fit(max_edge, width, height);
    ::core::write!(out, "{} {}x{} → {}x{}", format.label(), width, height, w, h)
        .map_err(|_| "buffer full")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::theme::Theme;

    #[test]
    fn exit_zero_is_ok_and_nonzero_fails() {
        let ok = TraceRow::new(1, Phase::Verify, "test", RowBody::Exit { code: 0 });
        let bad = TraceRow::new(1, Phase::Verify, "test", RowBody::Exit { code: 1 });
        assert_eq!(ok.status(), Status::Ok);
        assert_eq!(bad.status(), Status::Fail);
    }

    #[test]
    fn stderr_is_a_warning() {
        let row = TraceRow::new(
            2,
            Phase::Act,
            "bash",
            RowBody::Console {
                stream: LogStream::Stderr,
                lines: 3,
            },
        );
        assert_eq!(row.status(), Status::Warn);
    }

    #[test]
    fn status_colors_come_from_the_palette() {
        let theme = Theme::tokyo_night();
        let p = &theme.palette;
        let bad = TraceRow::new(2, Phase::Verify, "test", RowBody::Exit { code: 2 });
        assert_eq!(bad.status_colors(p).0, p.destructive);
        let good = TraceRow::new(2, Phase::Verify, "test", RowBody::Exit { code: 0 });
        assert_eq!(good.status_colors(p).0, p.success);
    }

    #[test]
    fn summaries_are_stable() {
        let diff = TraceRow::new(3, Phase::Act, "edit_file", RowBody::Diff { added: 12, removed: 4 });
        assert_eq!(diff.summary_buf().as_str(), "step 3 · act · edit_file +12 −4");
        let exit = TraceRow::new(4, Phase::Verify, "cargo test", RowBody::Exit { code: 0 });
        assert_eq!(exit.summary_buf().as_str(), "step 4 · verify · cargo test exit 0");
    }

    #[test]
    fn diff_stat_ignores_file_headers() {
        let patch = "--- a/x\n+++ b/x\n+one\n+two\n-gone\n unchanged";
        assert_eq!(diff_stat(patch), (2, 1));
        assert_eq!(diff_stat(""), (0, 0));
    }

    #[test]
    fn image_label_shows_the_downscale() {
        let mut buf: heapless::String<64> = heapless::String::new();
        image_label(Format::Png, 3000, 2000, 1568, &mut buf).unwrap();
        assert_eq!(buf.as_str(), "png 3000x2000 → 1568x1045");
    }
}
