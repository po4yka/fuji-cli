use super::common::file::{Input, Output};
use crate::cli::{GlobalOptions, common::usb};
use anyhow::ensure;
use clap::Subcommand;
use log::warn;

pub const MAX_BACKUP_INPUT_BYTES: usize = 256 * 1024 * 1024;

fn ensure_import_confirmation(
    yes: bool,
    emulated: bool,
    allow_emulated_target: bool,
) -> anyhow::Result<()> {
    ensure!(yes, "backup import requires explicit --yes confirmation");
    ensure!(
        !emulated || allow_emulated_target,
        "backup import with --emulate also requires --allow-emulated-target"
    );
    Ok(())
}

pub fn backup_import_target_warning(camera_name: &str, usb_id: &str, emulated: bool) -> String {
    let mode = if emulated {
        " using an emulated camera model"
    } else {
        ""
    };
    format!(
        "Restoring opaque backup to {camera_name} at USB {usb_id}{mode}; a failed restore may leave camera state unknown"
    )
}

#[derive(Subcommand, Debug, Clone)]
pub enum BackupCmd {
    /// Export backup
    #[command(alias = "e")]
    Export {
        /// Output file (use '-' to write to stdout)
        output: Output,
    },

    /// Import backup
    #[command(alias = "i")]
    Import {
        /// Input file (use '-' to read from stdin)
        input: Input,

        /// Confirm sending this opaque backup to the selected camera
        #[arg(long, required = true)]
        yes: bool,

        /// Allow restore while treating the physical camera as another model
        #[arg(long, requires = "yes")]
        allow_emulated_target: bool,
    },
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "command handlers consume parsed CLI values"
)]
fn handle_export(options: GlobalOptions, output: Output) -> anyhow::Result<()> {
    let GlobalOptions {
        device, emulate, ..
    } = options;

    let mut camera = usb::get_camera(device, emulate)?;

    let backup = camera.export_backup()?;
    output.write_all(&backup)?;

    Ok(())
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "command handlers consume parsed CLI values"
)]
fn handle_import(
    options: GlobalOptions,
    input: Input,
    yes: bool,
    allow_emulated_target: bool,
) -> anyhow::Result<()> {
    let GlobalOptions {
        device, emulate, ..
    } = options;
    let emulated = emulate.is_some();
    ensure_import_confirmation(yes, emulated, allow_emulated_target)?;

    let backup = input.read_limited(MAX_BACKUP_INPUT_BYTES, "backup input")?;
    let mut camera = usb::get_camera(device, emulate)?;
    warn!(
        "{}",
        backup_import_target_warning(camera.name(), &camera.connected_usb_id(), emulated)
    );
    camera.import_backup(&backup)?;

    Ok(())
}

pub fn handle(cmd: BackupCmd, options: GlobalOptions) -> anyhow::Result<()> {
    match cmd {
        BackupCmd::Export { output } => handle_export(options, output),
        BackupCmd::Import {
            input,
            yes,
            allow_emulated_target,
        } => handle_import(options, input, yes, allow_emulated_target),
    }
}

#[cfg(test)]
mod tests {
    use super::{backup_import_target_warning, ensure_import_confirmation};

    #[test]
    fn emulated_backup_import_requires_separate_opt_in() {
        let error = ensure_import_confirmation(true, true, false)
            .expect_err("--yes alone must not authorize an emulated restore target");

        assert!(error.to_string().contains("emulated"));
    }

    #[test]
    fn restore_warning_identifies_physical_usb_target_and_emulation() {
        let warning = backup_import_target_warning("FUJIFILM X-T5", "3.7", true);

        assert!(warning.contains("FUJIFILM X-T5"));
        assert!(warning.contains("USB 3.7"));
        assert!(warning.contains("emulated"));
    }
}
