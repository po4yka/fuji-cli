pub mod backup;
pub mod common;
pub mod device;
pub mod image;
pub mod simulation;

use clap::{ArgAction, Args, Parser, Subcommand};
use fujicli::policy::{CommandRisk, EmulationAcknowledgement, ModelBindingKind, authorize};

use backup::BackupCmd;
use device::DeviceCmd;
use image::ImageCmd;
use simulation::SimulationCmd;

use crate::cli::common::usb::{Identity, Location};

#[derive(Parser, Debug)]
#[command(version, about, long_about = None, author)]
pub struct Cli {
    /// Subcommands
    #[command(subcommand)]
    pub command: Commands,

    #[command(flatten)]
    pub options: GlobalOptions,
}

#[derive(Args, Debug)]
pub struct GlobalOptions {
    /// Format output using json
    #[arg(long, short = 'j', global = true)]
    pub json: bool,

    /// Log extra debugging information (multiple instances increase verbosity)
    #[arg(long, short = 'v', action = ArgAction::Count, global = true)]
    pub verbose: u8,

    /// Manually specify target device using USB <BUS>.<ADDRESS>
    #[arg(long, short = 'd', global = true)]
    pub device: Option<Location>,

    #[expect(
        clippy::doc_markdown,
        reason = "the angle-bracket notation is user-facing CLI syntax, not Rust documentation"
    )]
    /// Treat device as a different model using <VENDOR_ID>:<PRODUCT_ID>
    #[arg(long, global = true)]
    pub emulate: Option<Identity>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Manage devices
    #[command(alias = "d", subcommand)]
    Device(DeviceCmd),

    /// Manage film simulations
    #[command(alias = "s", subcommand)]
    Simulation(SimulationCmd),

    /// Manage backups
    #[command(alias = "b", subcommand)]
    Backup(BackupCmd),

    /// Manage and render images
    #[command(alias = "i", subcommand)]
    Image(ImageCmd),
}

pub fn handle(cli: Cli) -> Result<(), anyhow::Error> {
    authorize_command(&cli.command, &cli.options)?;

    let () = match cli.command {
        Commands::Device(device_cmd) => device::handle(device_cmd, cli.options)?,
        Commands::Backup(backup_cmd) => backup::handle(backup_cmd, cli.options)?,
        Commands::Simulation(simulation_cmd) => {
            simulation::handle(simulation_cmd, cli.options)?;
        }
        Commands::Image(render_cmd) => image::handle(render_cmd, cli.options)?,
    };

    Ok(())
}

fn authorize_command(command: &Commands, options: &GlobalOptions) -> anyhow::Result<()> {
    let binding = if options.emulate.is_some() {
        ModelBindingKind::Emulated
    } else {
        return Ok(());
    };
    let risk = match command {
        Commands::Simulation(SimulationCmd::Set { .. } | SimulationCmd::Import { .. }) => {
            CommandRisk::PersistentSettingsWrite
        }
        Commands::Simulation(
            SimulationCmd::List | SimulationCmd::Get { .. } | SimulationCmd::Export { .. },
        )
        | Commands::Device(DeviceCmd::List) => CommandRisk::EmulationForbidden,
        Commands::Device(DeviceCmd::Info) => CommandRisk::ReadOnly,
        Commands::Backup(BackupCmd::Import(_)) => CommandRisk::OpaqueRestore,
        Commands::Backup(BackupCmd::Export { .. } | BackupCmd::Inspect { .. }) => {
            CommandRisk::EmulationForbidden
        }
        Commands::Image(_) => CommandRisk::DestructiveRecoverySensitive,
    };

    authorize(binding, risk, EmulationAcknowledgement::NotProvided)
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::{Cli, authorize_command};

    #[test]
    fn image_extract_is_not_advertised_until_it_is_implemented() {
        let result = Cli::try_parse_from(["fujicli", "image", "extract", "input.jpg", "-"]);

        assert!(result.is_err());
    }

    #[test]
    fn image_render_does_not_accept_unimplemented_like_option() {
        let result = Cli::try_parse_from([
            "fujicli",
            "image",
            "render",
            "--like",
            "reference.jpg",
            "input.raf",
            "output.jpg",
        ]);

        assert!(result.is_err());
    }

    #[test]
    fn image_recover_parses_handle_output_and_explicit_cleanup() {
        let parsed = Cli::try_parse_from([
            "fujicli",
            "image",
            "recover",
            "--target-serial-sha256",
            "0000000000000000000000000000000000000000000000000000000000000000",
            "--delete-after-save",
            "42",
            "recovered.jpg",
        ]);

        assert!(parsed.is_ok(), "recovery command must parse: {parsed:?}");
    }

    #[test]
    fn simulation_reads_reject_emulation_without_an_impossible_acknowledgement_hint() {
        let cli = Cli::try_parse_from(["fujicli", "--emulate", "04cb:02fc", "simulation", "list"])
            .expect("emulated simulation command must parse before policy authorization");

        let error = authorize_command(&cli.command, &cli.options)
            .expect_err("temporary selector writes must reject emulation");

        assert!(error.to_string().contains("--emulate is not supported"));
        assert!(!error.to_string().contains("acknowledgement"));
    }

    #[test]
    fn backup_import_requires_explicit_acknowledgement() {
        let error = Cli::try_parse_from(["fujicli", "backup", "import", "backup.dat"])
            .expect_err("backup import must require --yes before any I/O");

        assert_eq!(
            error.kind(),
            clap::error::ErrorKind::MissingRequiredArgument
        );
    }

    #[test]
    fn backup_import_dry_run_does_not_require_destructive_acknowledgement() {
        let parsed =
            Cli::try_parse_from(["fujicli", "backup", "import", "backup.dat", "--dry-run"]);

        assert!(
            parsed.is_ok(),
            "dry-run must be non-destructive: {parsed:?}"
        );
    }

    #[test]
    fn backup_import_requires_recovery_output_before_destructive_restore() {
        let error = Cli::try_parse_from([
            "fujicli",
            "backup",
            "import",
            "backup.dat",
            "--yes",
            "--expect-sha256",
            "0000000000000000000000000000000000000000000000000000000000000000",
        ])
        .expect_err("destructive restore must require a recovery backup path");

        assert_eq!(
            error.kind(),
            clap::error::ErrorKind::MissingRequiredArgument
        );
    }

    #[test]
    fn backup_import_requires_external_artifact_fingerprint() {
        let error = Cli::try_parse_from([
            "fujicli",
            "backup",
            "import",
            "backup.dat",
            "--yes",
            "--recovery-backup",
            "recovery.fbk",
        ])
        .expect_err("destructive restore must pin the complete artifact");

        assert_eq!(
            error.kind(),
            clap::error::ErrorKind::MissingRequiredArgument
        );
        assert!(error.to_string().contains("--expect-sha256"));
    }

    #[test]
    fn backup_inspect_is_available_without_device_options() {
        let parsed = Cli::try_parse_from(["fujicli", "backup", "inspect", "backup.dat"]);

        assert!(
            parsed.is_ok(),
            "offline backup inspect must parse: {parsed:?}"
        );
    }

    #[test]
    fn reverse_command_is_absent_from_production_parser() {
        let error = Cli::try_parse_from(["fujicli", "device", "reverse", "info"])
            .expect_err("production parser must not expose reverse commands");

        assert_eq!(error.kind(), clap::error::ErrorKind::InvalidSubcommand);
    }
}
