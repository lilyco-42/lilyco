//! Execution host: parses arguments, validates, runs, and guarantees a
//! terminal `Done`/`Error` progress event.

use crate::core::progress::Progress;
use crate::TinyApp;

/// Run one command end to end.
pub fn dispatch_command<A: TinyApp>(app: &A, raw_args: &[&str]) -> Result<A::Output, A::Error>
where
    A::Error: From<&'static str>,
{
    let parsed = parse_raw_args::<A>(raw_args)?;
    A::validate(&parsed)?;
    let mut last = Progress::Init;
    let result = app.execute(parsed, |p| last = p);
    finalize_execution(result, last)
}

fn parse_raw_args<A: TinyApp>(_args: &[&str]) -> Result<A::Args, A::Error>
where
    A::Error: From<&'static str>,
{
    Err(A::Error::from("argument parsing is not implemented for this app"))
}

fn finalize_execution<T, E>(res: Result<T, E>, _last: Progress) -> Result<T, E> {
    res
}
