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
mod firmware;
#[cfg(feature = "dangerous-reverse-engineering")]
mod interrupt;
mod log;
mod output;
#[cfg(feature = "dangerous-reverse-engineering")]
mod probe;
mod reverse;
mod strings;
mod surface;
mod usb;

use clap::{ArgAction, Parser, Subcommand};

#[cfg(feature = "dangerous-reverse-engineering")]
use crate::probe::ProbeCommand;
use crate::{firmware::FirmwareCommand, reverse::DiscoverCommand, usb::Location};

#[derive(Debug, Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    /// Exact target device as USB `BUS.ADDRESS`. Required by every command
    /// that talks to a camera; `firmware` reads a file and does not use it.
    #[arg(long, short = 'd')]
    device: Option<Location>,

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
    /// Inspect or unpack a vendor firmware container file; no camera involved
    #[command(subcommand)]
    Firmware(FirmwareCommand),
    /// Run a gated, state-changing reverse-engineering probe
    #[cfg(feature = "dangerous-reverse-engineering")]
    #[command(subcommand)]
    Probe(ProbeCommand),
}

/// Every camera command names its target explicitly; there is deliberately no
/// automatic selection, so a missing `--device` is refused before any USB I/O.
fn device(device: Option<Location>) -> anyhow::Result<Location> {
    device.ok_or_else(|| {
        anyhow::anyhow!("this command talks to a camera and requires an exact --device BUS.ADDRESS")
    })
}

fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();
    log::init(cli.verbose)?;
    #[cfg(feature = "dangerous-reverse-engineering")]
    interrupt::install()?;
    match cli.command {
        Command::Discover(command) => reverse::handle(command, device(cli.device)?),
        Command::Firmware(command) => firmware::handle(&command),
        #[cfg(feature = "dangerous-reverse-engineering")]
        Command::Probe(command) => probe::handle(&command, device(cli.device)?),
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
        let parsed = Cli::try_parse_from(["fujicli-dev", "discover", "info"])
            .expect("the device is validated per command, not by the parser");

        let error = super::device(parsed.device)
            .expect_err("development discovery must never auto-select a camera");

        assert!(error.to_string().contains("--device"), "{error}");
    }

    #[test]
    fn firmware_commands_read_a_file_and_need_no_device() {
        let parsed = Cli::try_parse_from(["fujicli-dev", "firmware", "inspect", "FWUP0030.DAT"])
            .expect("unpacking a vendor file must not demand a camera");

        assert!(parsed.device.is_none());

        let unpack = Cli::try_parse_from(["fujicli-dev", "firmware", "unpack", "in.dat", "out"]);
        assert!(unpack.is_ok(), "{unpack:?}");

        let missing_output = Cli::try_parse_from(["fujicli-dev", "firmware", "unpack", "in.dat"])
            .expect_err("unpacking requires an explicit output directory");
        assert_eq!(
            missing_output.kind(),
            clap::error::ErrorKind::MissingRequiredArgument
        );
    }

    #[test]
    fn value_printing_is_opt_in_per_probe() {
        let parsed = Cli::try_parse_from([
            "fujicli-dev",
            "--device",
            "1.2",
            "discover",
            "simulation",
            "--print-values",
        ]);
        assert!(parsed.is_ok(), "{parsed:?}");

        let error = Cli::try_parse_from([
            "fujicli-dev",
            "--device",
            "1.2",
            "discover",
            "render-profile",
            "--print-values",
            "out.json",
        ])
        .expect_err("render-profile writes an artifact and has no value printing");
        assert_eq!(error.kind(), clap::error::ErrorKind::UnknownArgument);
    }

    #[test]
    fn the_surface_survey_writes_an_artifact_and_has_no_value_printing() {
        let parsed = Cli::try_parse_from([
            "fujicli-dev",
            "--device",
            "1.2",
            "discover",
            "surface",
            "surface.json",
        ]);
        assert!(parsed.is_ok(), "{parsed:?}");

        let error = Cli::try_parse_from([
            "fujicli-dev",
            "--device",
            "1.2",
            "discover",
            "surface",
            "--print-values",
            "surface.json",
        ])
        .expect_err("the survey never prints payload bytes");
        assert_eq!(error.kind(), clap::error::ErrorKind::UnknownArgument);
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
        let parsed = Cli::try_parse_from([
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
        .expect("the device is validated per command, not by the parser");

        let error = super::device(parsed.device)
            .expect_err("the dangerous probe must never auto-select a camera either");

        assert!(error.to_string().contains("--device"), "{error}");
    }
}
