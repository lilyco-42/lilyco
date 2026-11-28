//! Execute typed input and give observers one terminal progress event.

use crate::{core::progress::Progress, TinyApp};

pub fn dispatch_command<A: TinyApp>(
    app: &A,
    args: A::Args,
    mut emit: impl FnMut(Progress),
) -> Result<A::Output, A::Error> {
    if let Err(error) = A::validate(&args) {
        emit(Progress::Error);
        return Err(error);
    }

    let mut terminal = None;
    let result = app.execute(args, |event| match event {
        Progress::Done | Progress::Error => {
            if terminal != Some(Progress::Error) {
                terminal = Some(event);
            }
        }
        other if terminal.is_none() => emit(other),
        _ => {}
    });
    emit(if result.is_ok() && terminal != Some(Progress::Error) {
        Progress::Done
    } else {
        Progress::Error
    });
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::schema::CommandSchema;

    struct App;

    impl TinyApp for App {
        type Args = &'static str;
        type Output = ();
        type Error = &'static str;

        fn schema() -> CommandSchema {
            CommandSchema::new("echo", 4)
        }

        fn validate(args: &Self::Args) -> Result<(), Self::Error> {
            Self::schema().validate_input(args)
        }

        fn execute<F>(&self, args: Self::Args, mut progress: F) -> Result<(), &'static str>
        where
            F: FnMut(Progress),
        {
            progress(Progress::Init);
            progress(Progress::Done);
            progress(Progress::Done);
            if args == "fail" {
                return Err("execution failed");
            }
            Ok(())
        }
    }

    #[test]
    fn publishes_one_terminal_event_and_gives_errors_precedence() {
        for (args, expected, middle) in [
            ("echo", Progress::Done, Some(Progress::Init)),
            ("fail", Progress::Error, Some(Progress::Init)),
            ("", Progress::Error, None),
        ] {
            let mut events = heapless::Vec::<Progress, 2>::new();
            let result = dispatch_command(&App, args, |event| events.push(event).unwrap());
            assert_eq!(events.first().copied(), middle.or(Some(expected)));
            assert_eq!(events.last(), Some(&expected));
            assert_eq!(events.len(), 1 + usize::from(middle.is_some()));
            assert_eq!(result.is_ok(), expected == Progress::Done);
        }
    }
}
