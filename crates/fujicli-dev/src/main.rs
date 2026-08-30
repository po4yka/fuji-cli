#![forbid(unsafe_code)]
#![warn(clippy::pedantic, clippy::nursery)]
#![allow(
    clippy::missing_docs_in_private_items,
    reason = "the unpublished adapter keeps private command plumbing terse"
)]

mod log;
mod output;
mod reverse;
mod usb;

use clap::{ArgAction, Parser, Subcommand};

use crate::{reverse::DiscoverCommand, usb::Location};

#[derive(Debug, Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    /// Exact target device as USB `BUS.ADDRESS`
    #[arg(long, short = 'd')]
    device: Location,

    /// Log extra protocol metadata; repeat for more detail
    #[arg(long, short = 'v', action = ArgAction::Count, global = true)]
    verbose: u8,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Run read-only discovery against an explicitly selected camera
    #[command(subcommand)]
    Discover(DiscoverCommand),
}

fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();
    log::init(cli.verbose)?;
    reverse::handle(cli.command, cli.device)
}

fn main() -> anyhow::Result<()> {
    run()
}

#[cfg(test)]
mod tests {
    use clap::Parser as _;

    use super::Cli;

    #[test]
    fn every_discovery_command_requires_an_explicit_device() {
        let error = Cli::try_parse_from(["fujicli-dev", "discover", "info"])
            .expect_err("development discovery must never auto-select a camera");

        assert_eq!(
            error.kind(),
            clap::error::ErrorKind::MissingRequiredArgument
        );
        assert!(error.to_string().contains("--device"));
    }

    #[test]
    fn dangerous_commands_are_not_present() {
        let error = Cli::try_parse_from(["fujicli-dev", "--device", "1.2", "dangerous", "restore"])
            .expect_err("no state-changing reverse command is implemented");

        assert_eq!(error.kind(), clap::error::ErrorKind::InvalidSubcommand);
    }

    #[test]
    fn raw_profile_discovery_requires_a_private_output_file() {
        let parsed = Cli::try_parse_from([
            "fujicli-dev",
            "--device",
            "1.2",
            "discover",
            "render-profile",
            "profile.json",
        ]);
        assert!(parsed.is_ok(), "RAW discovery must parse: {parsed:?}");

        let error = Cli::try_parse_from([
            "fujicli-dev",
            "--device",
            "1.2",
            "discover",
            "render-profile",
            "-",
        ])
        .expect_err("lossless wire artifacts must not be written to stdout");
        assert_eq!(error.kind(), clap::error::ErrorKind::ValueValidation);
    }
}
