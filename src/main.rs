#![forbid(unsafe_code)]
#![warn(clippy::pedantic, clippy::nursery)]
#![allow(
    clippy::missing_docs_in_private_items,
    clippy::similar_names,
    reason = "the CLI keeps private command plumbing terse and mirrors protocol field names"
)]

use std::process::ExitCode;

use cli::common::camera_state::{CAMERA_STATE_UNKNOWN_EXIT_CODE, CameraStateUnknown};

mod cli;
mod log;

fn run() -> anyhow::Result<()> {
    let cli = cli::Cli::parse();
    log::init(cli.options.verbose)?;
    cli::common::interrupt::install()?;
    cli::handle(cli)?;
    Ok(())
}

fn handle_result(result: anyhow::Result<()>) -> anyhow::Result<()> {
    match result {
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|error| error.kind() == std::io::ErrorKind::BrokenPipe) =>
        {
            Ok(())
        }
        result => result,
    }
}

/// Map an application error to the documented process status. A
/// [`CameraStateUnknown`] marker means a state-changing camera operation was
/// already sent and its outcome is unconfirmed. `anyhow::Error::is` searches
/// the whole context chain, so the mapping is independent of added context.
fn error_exit_code(error: &anyhow::Error) -> u8 {
    if error.is::<CameraStateUnknown>() {
        CAMERA_STATE_UNKNOWN_EXIT_CODE
    } else {
        1
    }
}

fn main() -> ExitCode {
    match handle_result(run()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Error: {error:?}");
            ExitCode::from(error_exit_code(&error))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io;

    use super::{CameraStateUnknown, error_exit_code, handle_result};

    #[test]
    fn broken_stdout_pipe_is_a_successful_cli_exit() {
        let result = handle_result(Err(
            io::Error::new(io::ErrorKind::BrokenPipe, "closed").into()
        ));

        assert!(result.is_ok());
    }

    #[test]
    fn error_carrying_the_marker_maps_to_state_unknown_exit_code() {
        let error = anyhow::Error::new(CameraStateUnknown).context("operation outcome unconfirmed");

        assert_eq!(error_exit_code(&error), 3);
    }

    #[test]
    fn ordinary_error_maps_to_operational_failure_exit_code() {
        let error = anyhow::anyhow!("plain failure").context("more context");

        assert_eq!(error_exit_code(&error), 1);
    }
}
