pub mod backup;
pub mod common;
pub mod device;
pub mod image;
pub mod simulation;

use std::{ffi::OsString, io::Write as _};

use clap::{ArgAction, Args, CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::{Shell, generate};

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

impl Cli {
    pub fn parse() -> Self {
        Self::try_parse_from(std::env::args_os()).unwrap_or_else(|error| error.exit())
    }

    pub fn try_parse_from<I, T>(arguments: I) -> Result<Self, clap::Error>
    where
        I: IntoIterator<Item = T>,
        T: Into<OsString> + Clone,
    {
        let parsed = <Self as Parser>::try_parse_from(arguments)?;
        if let Commands::Backup(BackupCmd::Import(args)) = &parsed.command
            && let Some((kind, message)) = args.validation_error()
        {
            return Err(Self::command().error(kind, message));
        }
        Ok(parsed)
    }
}

#[derive(Args, Debug)]
pub struct GlobalOptions {
    /// Log extra debugging information (multiple instances increase verbosity)
    #[arg(long, short = 'v', action = ArgAction::Count, global = true)]
    pub verbose: u8,
}

#[derive(Args, Debug, Default, Clone)]
pub struct JsonOptions {
    /// Format output using JSON
    #[arg(long, short = 'j')]
    pub json: bool,
}

#[derive(Args, Debug, Default, Clone)]
pub struct DeviceOptions {
    /// Manually specify target device using USB <BUS>.<ADDRESS>
    #[arg(long, short = 'd')]
    pub device: Option<Location>,
}

#[derive(Args, Debug, Default, Clone)]
pub struct EmulationOptions {
    #[expect(
        clippy::doc_markdown,
        reason = "the angle-bracket notation is user-facing CLI syntax, not Rust documentation"
    )]
    /// Treat device as a different model using <VENDOR_ID>:<PRODUCT_ID>
    #[arg(long)]
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

    /// Generate a shell completion script
    Completion {
        /// Shell whose completion script should be generated
        #[arg(value_enum)]
        shell: CompletionShell,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum CompletionShell {
    Bash,
    Zsh,
    Fish,
    #[value(name = "powershell")]
    PowerShell,
}

impl From<CompletionShell> for Shell {
    fn from(shell: CompletionShell) -> Self {
        match shell {
            CompletionShell::Bash => Self::Bash,
            CompletionShell::Zsh => Self::Zsh,
            CompletionShell::Fish => Self::Fish,
            CompletionShell::PowerShell => Self::PowerShell,
        }
    }
}

fn write_completion(shell: CompletionShell) -> anyhow::Result<()> {
    let mut script = Vec::new();
    generate(
        Shell::from(shell),
        &mut Cli::command(),
        "fujicli",
        &mut script,
    );

    let stdout = std::io::stdout();
    let mut stdout = stdout.lock();
    stdout.write_all(&script)?;
    stdout.flush()?;
    Ok(())
}

pub fn handle(cli: Cli) -> Result<(), anyhow::Error> {
    let () = match cli.command {
        Commands::Device(device_cmd) => device::handle(device_cmd)?,
        Commands::Backup(backup_cmd) => backup::handle(backup_cmd)?,
        Commands::Simulation(simulation_cmd) => simulation::handle(simulation_cmd)?,
        Commands::Image(render_cmd) => image::handle(render_cmd)?,
        Commands::Completion { shell } => write_completion(shell)?,
    };

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, fs, path::Path};

    use clap::CommandFactory;
    use clap_complete::{Shell, generate};

    use super::Cli;

    fn directory_files(root: &Path) -> std::io::Result<BTreeMap<String, Vec<u8>>> {
        fn visit(
            root: &Path,
            directory: &Path,
            files: &mut BTreeMap<String, Vec<u8>>,
        ) -> std::io::Result<()> {
            for entry in fs::read_dir(directory)? {
                let entry = entry?;
                let path = entry.path();
                if path.is_dir() {
                    visit(root, &path, files)?;
                } else {
                    let relative = path
                        .strip_prefix(root)
                        .expect("visited files must stay below their root")
                        .to_string_lossy()
                        .replace(std::path::MAIN_SEPARATOR, "/");
                    files.insert(relative, fs::read(path)?);
                }
            }
            Ok(())
        }

        let mut files = BTreeMap::new();
        if !root.exists() {
            return Ok(files);
        }
        visit(root, root, &mut files)?;
        Ok(files)
    }

    fn generate_cli_assets(root: &Path) -> anyhow::Result<()> {
        let completions = [
            (Shell::Bash, "bash-completion/completions/fujicli"),
            (Shell::Zsh, "zsh/site-functions/_fujicli"),
            (Shell::Fish, "fish/vendor_completions.d/fujicli.fish"),
            (Shell::PowerShell, "powershell/Completions/fujicli.ps1"),
        ];

        for (shell, relative_path) in completions {
            let path = root.join(relative_path);
            fs::create_dir_all(path.parent().expect("completion path has a parent"))?;
            let mut output = Vec::new();
            generate(shell, &mut Cli::command(), "fujicli", &mut output);
            fs::write(path, output)?;
        }

        let man_directory = root.join("man/man1");
        fs::create_dir_all(&man_directory)?;
        clap_mangen::generate_to(Cli::command(), &man_directory)?;
        for entry in fs::read_dir(man_directory)? {
            let path = entry?.path();
            let generated = fs::read_to_string(&path)?;
            let mut normalized = String::with_capacity(generated.len());
            for line in generated.lines() {
                normalized.push_str(line.trim_end());
                normalized.push('\n');
            }
            fs::write(path, normalized)?;
        }
        Ok(())
    }

    #[test]
    fn packaged_cli_assets_match_the_command_model() -> anyhow::Result<()> {
        let temporary = tempfile::tempdir()?;
        generate_cli_assets(temporary.path())?;
        let actual = directory_files(temporary.path())?;
        let asset_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/share");

        if std::env::var_os("BLESS_CLI_ASSETS").is_some() {
            if asset_root.exists() {
                fs::remove_dir_all(&asset_root)?;
            }
            generate_cli_assets(&asset_root)?;
        }

        let expected = directory_files(&asset_root)?;
        let actual_paths = actual.keys().collect::<Vec<_>>();
        let expected_paths = expected.keys().collect::<Vec<_>>();
        assert_eq!(
            actual_paths, expected_paths,
            "packaged CLI asset set is stale"
        );
        for (path, actual_bytes) in actual {
            assert!(
                expected
                    .get(&path)
                    .is_some_and(|bytes| bytes == &actual_bytes),
                "packaged CLI asset {path} is stale; regenerate it with BLESS_CLI_ASSETS=1"
            );
        }
        Ok(())
    }

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
    fn generated_string_option_does_not_consume_following_flag() {
        let error = Cli::try_parse_from([
            "fujicli",
            "simulation",
            "set",
            "c1",
            "--target-serial-sha256",
            "0000000000000000000000000000000000000000000000000000000000000000",
            "--custom-setting-name",
            "--verbose",
        ])
        .expect_err("--verbose must not become a custom-setting-name value");

        assert_eq!(error.kind(), clap::error::ErrorKind::InvalidValue);
        assert!(error.to_string().contains("--custom-setting-name"));
    }

    #[test]
    fn generated_enum_option_does_not_consume_following_flag() {
        let error = Cli::try_parse_from([
            "fujicli",
            "image",
            "render",
            "--target-serial-sha256",
            "0000000000000000000000000000000000000000000000000000000000000000",
            "--film-simulation",
            "--draft",
            "input.raf",
            "output.jpg",
        ])
        .expect_err("--draft must not become a film-simulation value");
        let diagnostic = error.to_string();

        assert!(diagnostic.contains("a value is required"), "{diagnostic}");
        assert!(diagnostic.contains("--film-simulation"), "{diagnostic}");
        assert!(
            !diagnostic.contains("invalid value '--draft'"),
            "{diagnostic}"
        );
    }

    #[test]
    fn generated_numeric_option_accepts_negative_value_before_following_flag() {
        let parsed = Cli::try_parse_from([
            "fujicli",
            "simulation",
            "set",
            "c1",
            "--target-serial-sha256",
            "0000000000000000000000000000000000000000000000000000000000000000",
            "--shadow-tone",
            "-1.0",
            "--verbose",
        ])
        .expect("negative numeric values and the following flag must both parse");

        assert_eq!(parsed.options.verbose, 1);
        let super::Commands::Simulation(super::SimulationCmd::Set { simulation, .. }) =
            parsed.command
        else {
            panic!("expected simulation set command");
        };
        assert!(simulation.shadow_tone.is_some());
    }

    #[test]
    fn generated_string_option_accepts_attached_hyphen_value() {
        let parsed = Cli::try_parse_from([
            "fujicli",
            "simulation",
            "set",
            "c1",
            "--target-serial-sha256",
            "0000000000000000000000000000000000000000000000000000000000000000",
            "--custom-setting-name=--draft",
        ])
        .expect("an attached string value may start with a hyphen");

        let super::Commands::Simulation(super::SimulationCmd::Set { simulation, .. }) =
            parsed.command
        else {
            panic!("expected simulation set command");
        };
        assert!(simulation.custom_setting_name.is_some());
    }

    #[test]
    fn device_list_rejects_device_selector() {
        let error = Cli::try_parse_from(["fujicli", "device", "list", "--device", "1.2"])
            .expect_err("device list must not accept a selector that it ignores");

        assert_eq!(error.kind(), clap::error::ErrorKind::UnknownArgument);
    }

    #[test]
    fn irrelevant_leaf_options_are_rejected() {
        const SERIAL: &str = "0000000000000000000000000000000000000000000000000000000000000000";
        let cases: &[&[&str]] = &[
            &["fujicli", "device", "list", "--emulate", "04cb:02fc"],
            &[
                "fujicli",
                "backup",
                "export",
                "backup.fbk",
                "--emulate",
                "04cb:02fc",
            ],
            &[
                "fujicli",
                "backup",
                "inspect",
                "backup.fbk",
                "--device",
                "1.2",
            ],
            &[
                "fujicli",
                "backup",
                "inspect",
                "backup.fbk",
                "--emulate",
                "04cb:02fc",
            ],
            &["fujicli", "simulation", "list", "--emulate", "04cb:02fc"],
            &["fujicli", "simulation", "export", "c1", "-", "--json"],
            &[
                "fujicli",
                "simulation",
                "export",
                "c1",
                "-",
                "--emulate",
                "04cb:02fc",
            ],
            &[
                "fujicli",
                "simulation",
                "set",
                "c1",
                "--target-serial-sha256",
                SERIAL,
                "--custom-setting-name",
                "Test",
                "--json",
            ],
            &[
                "fujicli",
                "simulation",
                "import",
                "c1",
                "simulation.json",
                "--target-serial-sha256",
                SERIAL,
                "--json",
            ],
            &[
                "fujicli",
                "image",
                "render",
                "--target-serial-sha256",
                SERIAL,
                "input.raf",
                "output.jpg",
                "--json",
            ],
            &[
                "fujicli",
                "image",
                "recover",
                "--target-serial-sha256",
                SERIAL,
                "42",
                "output.jpg",
                "--json",
            ],
        ];

        for arguments in cases {
            let error = Cli::try_parse_from(*arguments)
                .expect_err("an option without a consumer must be rejected by clap");

            assert_eq!(
                error.kind(),
                clap::error::ErrorKind::UnknownArgument,
                "unexpected parser result for {arguments:?}: {error}"
            );
        }
    }

    #[test]
    fn applicable_leaf_options_remain_available() {
        const SERIAL: &str = "0000000000000000000000000000000000000000000000000000000000000000";
        let cases: &[&[&str]] = &[
            &["fujicli", "device", "list", "--json"],
            &[
                "fujicli",
                "device",
                "info",
                "--json",
                "--device",
                "1.2",
                "--emulate",
                "04cb:02fc",
            ],
            &[
                "fujicli",
                "backup",
                "export",
                "backup.fbk",
                "--json",
                "--device",
                "1.2",
            ],
            &["fujicli", "backup", "inspect", "backup.fbk", "--json"],
            &[
                "fujicli",
                "backup",
                "import",
                "backup.fbk",
                "--dry-run",
                "--json",
                "--expect-sha256",
                SERIAL,
                "--target-serial-sha256",
                SERIAL,
                "--device",
                "1.2",
            ],
            &[
                "fujicli",
                "backup",
                "import",
                "-",
                "--yes",
                "--recovery-backup",
                "recovery.fbk",
                "--expect-sha256",
                SERIAL,
                "--allow-stdin",
            ],
            &["fujicli", "simulation", "list", "--json", "--device", "1.2"],
            &[
                "fujicli",
                "simulation",
                "set",
                "c1",
                "--target-serial-sha256",
                SERIAL,
                "--custom-setting-name",
                "Test",
                "--device",
                "1.2",
            ],
            &[
                "fujicli",
                "image",
                "render",
                "--target-serial-sha256",
                SERIAL,
                "input.raf",
                "output.jpg",
                "--device",
                "1.2",
            ],
        ];

        for arguments in cases {
            let parsed = Cli::try_parse_from(*arguments);

            assert!(
                parsed.is_ok(),
                "an option with a leaf consumer must parse for {arguments:?}: {parsed:?}"
            );
        }
    }

    #[test]
    fn backup_import_json_requires_dry_run() {
        let error = Cli::try_parse_from([
            "fujicli",
            "backup",
            "import",
            "backup.fbk",
            "--yes",
            "--recovery-backup",
            "recovery.fbk",
            "--expect-sha256",
            "0000000000000000000000000000000000000000000000000000000000000000",
            "--json",
        ])
        .expect_err("restore has no JSON result to format");

        assert_eq!(
            error.kind(),
            clap::error::ErrorKind::MissingRequiredArgument
        );
        assert!(error.to_string().contains("--dry-run"));
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
