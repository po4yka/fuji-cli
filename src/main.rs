#![forbid(unsafe_code)]
#![warn(clippy::pedantic, clippy::nursery)]
#![allow(
    clippy::missing_docs_in_private_items,
    clippy::similar_names,
    reason = "the CLI keeps private command plumbing terse and mirrors protocol field names"
)]

use clap::Parser;

mod cli;
mod log;

fn run() -> anyhow::Result<()> {
    let cli = cli::Cli::parse();
    log::init(cli.options.verbose)?;
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

fn main() -> anyhow::Result<()> {
    handle_result(run())
}

#[cfg(test)]
mod tests {
    use std::io;

    use super::handle_result;

    #[test]
    fn broken_stdout_pipe_is_a_successful_cli_exit() {
        let result = handle_result(Err(
            io::Error::new(io::ErrorKind::BrokenPipe, "closed").into()
        ));

        assert!(result.is_ok());
    }
}
