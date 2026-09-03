#![forbid(unsafe_code)]
#![warn(clippy::pedantic, clippy::nursery)]
#![allow(
    clippy::missing_docs_in_private_items,
    clippy::similar_names,
    reason = "the CLI keeps private command plumbing terse and mirrors protocol field names"
)]

use std::process::ExitCode;

use cli::common::{
    camera_state::{CAMERA_STATE_UNKNOWN_EXIT_CODE, CameraStateUnknown},
    file::ArtifactOutputIncomplete,
};
use fujicli::interrupt::{INTERRUPTED_EXIT_CODE, Interrupted};

mod cli;
mod log;

fn run() -> anyhow::Result<()> {
    let cli = cli::Cli::parse();
    log::init(cli.options.verbose)?;
    cli::common::interrupt::install()?;
    cli::handle(cli)?;
    Ok(())
}

/// A consumer closing stdout early is a successful termination for textual
/// output only. An artifact that was requested in full and could not be
/// delivered in full carries [`ArtifactOutputIncomplete`] and stays a failure.
fn handle_result(result: anyhow::Result<()>) -> anyhow::Result<()> {
    match result {
        Err(error)
            if !error.is::<ArtifactOutputIncomplete>()
                && error
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
/// already sent and its outcome is unconfirmed; an [`Interrupted`] marker
/// means a Ctrl-C was honoured once the in-flight PTP transaction completed.
/// `anyhow::Error::is` searches the whole context chain, so the mapping is
/// independent of added context.
fn error_exit_code(error: &anyhow::Error) -> u8 {
    if error.is::<CameraStateUnknown>() {
        return CAMERA_STATE_UNKNOWN_EXIT_CODE;
    }
    match error.downcast_ref::<Interrupted>() {
        Some(Interrupted {
            after_camera_write: true,
        }) => CAMERA_STATE_UNKNOWN_EXIT_CODE,
        Some(Interrupted {
            after_camera_write: false,
        }) => INTERRUPTED_EXIT_CODE,
        None => 1,
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

    use super::{CameraStateUnknown, Interrupted, error_exit_code, handle_result};

    #[test]
    fn broken_stdout_pipe_is_a_successful_cli_exit() {
        let result = handle_result(Err(
            io::Error::new(io::ErrorKind::BrokenPipe, "closed").into()
        ));

        assert!(result.is_ok());
    }

    #[test]
    fn truncated_artifact_on_a_broken_pipe_stays_a_failure() {
        let error = anyhow::Error::new(io::Error::new(io::ErrorKind::BrokenPipe, "closed"))
            .context(super::ArtifactOutputIncomplete);

        let error = handle_result(Err(error)).expect_err("a truncated artifact is not a success");

        assert_eq!(error_exit_code(&error), 1);
    }

    #[test]
    fn error_carrying_the_marker_maps_to_state_unknown_exit_code() {
        let error = anyhow::Error::new(CameraStateUnknown).context("operation outcome unconfirmed");

        assert_eq!(error_exit_code(&error), 3);
    }

    #[test]
    fn honoured_interrupt_maps_to_the_interrupted_exit_code() {
        let error = anyhow::Error::new(Interrupted {
            after_camera_write: false,
        })
        .context("after GetObject");

        assert_eq!(error_exit_code(&error), 130);
    }

    #[test]
    fn interrupt_after_a_camera_write_maps_to_state_unknown_exit_code() {
        let error = anyhow::Error::new(Interrupted {
            after_camera_write: true,
        })
        .context("waiting for the camera to reconnect after restore");

        assert_eq!(error_exit_code(&error), 3);
    }

    #[test]
    fn state_unknown_wins_over_an_interrupt_marker() {
        let error = anyhow::Error::new(Interrupted {
            after_camera_write: false,
        })
        .context(CameraStateUnknown)
        .context("restore verification interrupted");

        assert_eq!(error_exit_code(&error), 3);
    }

    #[test]
    fn ordinary_error_maps_to_operational_failure_exit_code() {
        let error = anyhow::anyhow!("plain failure").context("more context");

        assert_eq!(error_exit_code(&error), 1);
    }
}
