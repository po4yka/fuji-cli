#![forbid(unsafe_code)]
#![warn(clippy::pedantic, clippy::nursery)]
#![allow(
    clippy::missing_docs_in_private_items,
    clippy::similar_names,
    reason = "the CLI keeps private command plumbing terse and mirrors protocol field names"
)]

use std::process::ExitCode;

use clap::Parser;
use cli::common::camera_state::CameraStateUnknown;

mod cli;
mod log;

/// Exit code for a failure where a state-changing camera operation was
/// already sent and its outcome could not be confirmed. Distinct from the
/// generic failure code (`1`) so wrapper scripts can tell "safe to retry"
/// apart from "camera state is unknown -- do not retry automatically"
/// without scraping stderr text. Part of the CLI's public exit-code
/// contract; see `docs/users/usage.md`.
const CAMERA_STATE_UNKNOWN_EXIT_CODE: u8 = 3;

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

/// Whether `error`'s chain carries the [`CameraStateUnknown`] marker,
/// meaning a state-changing camera operation was already sent and its
/// outcome is unconfirmed. `anyhow::Error::is` searches the whole context
/// chain (not only the outermost layer), so this finds the marker
/// regardless of how many `.context(...)` layers were added on top of it.
fn is_camera_state_unknown(error: &anyhow::Error) -> bool {
    error.is::<CameraStateUnknown>()
}

fn main() -> ExitCode {
    match handle_result(run()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Error: {error:?}");
            if is_camera_state_unknown(&error) {
                ExitCode::from(CAMERA_STATE_UNKNOWN_EXIT_CODE)
            } else {
                ExitCode::FAILURE
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io;

    use super::{CameraStateUnknown, handle_result, is_camera_state_unknown};

    #[test]
    fn broken_stdout_pipe_is_a_successful_cli_exit() {
        let result = handle_result(Err(
            io::Error::new(io::ErrorKind::BrokenPipe, "closed").into()
        ));

        assert!(result.is_ok());
    }

    #[test]
    fn error_carrying_the_marker_is_classified_as_state_unknown() {
        let error = anyhow::Error::new(CameraStateUnknown).context("operation outcome unconfirmed");

        assert!(is_camera_state_unknown(&error));
    }

    #[test]
    fn error_without_the_marker_is_not_classified_as_state_unknown() {
        let error = anyhow::anyhow!("plain failure").context("more context");

        assert!(!is_camera_state_unknown(&error));
    }
}
