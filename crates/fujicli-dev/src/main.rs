#![forbid(unsafe_code)]
#![warn(clippy::pedantic, clippy::nursery)]
#![allow(
    clippy::missing_docs_in_private_items,
    reason = "the unpublished adapter keeps private command plumbing terse"
)]

#[cfg(feature = "dangerous-reverse-engineering")]
mod audit;
#[cfg(feature = "dangerous-reverse-engineering")]
mod decision;
mod log;
mod output;
#[cfg(feature = "dangerous-reverse-engineering")]
mod probe;
mod reverse;
mod usb;

use clap::{ArgAction, Parser, Subcommand};

#[cfg(feature = "dangerous-reverse-engineering")]
use crate::probe::ProbeCommand;
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
    /// Run a gated, state-changing reverse-engineering probe
    #[cfg(feature = "dangerous-reverse-engineering")]
    #[command(subcommand)]
    Probe(ProbeCommand),
}

fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();
    log::init(cli.verbose)?;
    match cli.command {
        Command::Discover(command) => reverse::handle(command, cli.device),
        #[cfg(feature = "dangerous-reverse-engineering")]
        Command::Probe(command) => probe::handle(&command, cli.device),
    }
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

    #[test]
    #[cfg(not(feature = "dangerous-reverse-engineering"))]
    fn probe_is_absent_without_the_dangerous_feature() {
        let error = Cli::try_parse_from([
            "fujicli-dev",
            "--device",
            "1.2",
            "probe",
            "simulation-namespace",
            "c1",
        ])
        .expect_err("probe must not exist without dangerous-reverse-engineering");

        assert_eq!(error.kind(), clap::error::ErrorKind::InvalidSubcommand);
    }

    #[test]
    #[cfg(feature = "dangerous-reverse-engineering")]
    fn probe_simulation_namespace_still_requires_an_explicit_device() {
        let error = Cli::try_parse_from([
            "fujicli-dev",
            "probe",
            "simulation-namespace",
            "c1",
            "/tmp/nonexistent-backup.fbk",
            "/tmp/nonexistent-audit.jsonl",
            "--confirm-fingerprint",
            "deadbeef",
            "--acknowledge",
            "I-UNDERSTAND-THIS-WRITES-SELECTOR-D18C",
        ])
        .expect_err("the dangerous probe must never auto-select a camera either");

        assert_eq!(
            error.kind(),
            clap::error::ErrorKind::MissingRequiredArgument
        );
        assert!(error.to_string().contains("--device"));
    }
}
