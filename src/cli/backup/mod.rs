use std::time::Duration;

use anyhow::{Context as _, ensure};
use clap::{Args, Subcommand};
use fujicli::{
    features::backup::{
        BACKUP_ARTIFACT_FORMAT_VERSION, BackupArtifact, BackupIdentity, BackupPurpose,
        MAX_BACKUP_ARTIFACT_BYTES,
    },
    policy::SerialFingerprint,
};
use log::warn;

use crate::cli::{
    GlobalOptions,
    common::{interrupt, usb},
};

use super::common::file::{Input, Output, write_stdout_line};

const BACKUP_RECONNECT_TIMEOUT: Duration = Duration::from_mins(2);

fn ensure_import_confirmation(yes: bool, dry_run: bool, emulated: bool) -> anyhow::Result<()> {
    ensure!(
        dry_run || yes,
        "backup import requires explicit --yes confirmation"
    );
    ensure!(
        !emulated,
        "safe backup import does not support --emulate; use fujicli-dev only for explicit protocol research"
    );
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ImportMode {
    DryRun,
    Restore,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StdinPermission {
    Denied,
    Allowed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ImportSource {
    File,
    Stdin(StdinPermission),
}

fn ensure_import_input_policy(
    mode: ImportMode,
    source: ImportSource,
    expected_sha256: Option<&str>,
) -> anyhow::Result<()> {
    if mode == ImportMode::Restore
        && let ImportSource::Stdin(permission) = source
    {
        ensure!(
            permission == StdinPermission::Allowed,
            "destructive backup import from stdin requires --allow-stdin"
        );
        ensure!(
            expected_sha256.is_some(),
            "destructive backup import from stdin requires --expect-sha256"
        );
    }
    Ok(())
}

fn validated_backup_import_target_warning(camera_name: &str, usb_id: &str) -> String {
    format!(
        "Restoring an integrity-checked backup artifact payload to {camera_name} at USB {usb_id}; a wire failure may leave camera state unknown and must not be retried automatically"
    )
}

fn restore_after_recovery_saved<T>(
    recovery: &BackupArtifact,
    backup: &BackupArtifact,
    save_recovery: impl FnOnce(&[u8]) -> anyhow::Result<()>,
    restore: impl FnOnce(&BackupArtifact) -> anyhow::Result<T>,
) -> anyhow::Result<T> {
    save_recovery(recovery.as_bytes())?;
    restore(backup)
}

fn report_dry_run(
    backup: &BackupArtifact,
    target: &BackupIdentity,
    explicitly_bound: bool,
    json: bool,
) -> anyhow::Result<()> {
    backup.validate_target_compatibility(target)?;
    let serial_bound =
        target.serial_sha256 == backup.manifest().source.serial_sha256 || explicitly_bound;
    if json {
        let result = serde_json::json!({
            "compatible": true,
            "serialBound": serial_bound,
            "artifactSha256": backup.fingerprint(),
            "target": target,
        });
        write_stdout_line(format_args!("{}", serde_json::to_string_pretty(&result)?))?;
    } else {
        write_stdout_line(format_args!(
            "Compatible target: {} ({}, firmware {})\nTarget serial SHA-256: {}\nArtifact SHA-256: {}",
            target.camera_name,
            target.model,
            target.firmware,
            target.serial_sha256,
            backup.fingerprint(),
        ))?;
        if !serial_bound {
            write_stdout_line(format_args!(
                "Destructive import requires --target-serial-sha256 {}",
                target.serial_sha256
            ))?;
        }
    }
    Ok(())
}

#[derive(Args, Debug, Clone)]
pub struct BackupImportArgs {
    /// Input file (use '-' to read from stdin)
    input: Input,

    /// Confirm sending the validated backup artifact to the selected camera
    #[arg(long, required_unless_present = "dry_run")]
    yes: bool,

    /// Validate compatibility without exporting recovery state or restoring
    #[arg(long)]
    dry_run: bool,

    /// New file that receives the target's current settings before restore
    #[arg(long, required_unless_present = "dry_run")]
    recovery_backup: Option<Output>,

    /// Expected SHA-256 of the complete input artifact
    #[arg(long, required_unless_present = "dry_run")]
    expect_sha256: Option<String>,

    /// Expected SHA-256 fingerprint of a different target camera serial
    #[arg(long)]
    target_serial_sha256: Option<String>,

    /// Permit destructive import from stdin (also requires --expect-sha256)
    #[arg(long, requires = "yes")]
    allow_stdin: bool,
}

#[derive(Subcommand, Debug, Clone)]
pub enum BackupCmd {
    /// Export backup
    #[command(alias = "e")]
    Export {
        /// Output file (use '-' to write to stdout)
        output: Output,
    },

    /// Inspect and validate a backup artifact without connecting a camera
    Inspect {
        /// Input file (use '-' to read from stdin)
        input: Input,
    },

    /// Import backup
    #[command(alias = "i")]
    Import(BackupImportArgs),
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "command handlers consume parsed CLI values"
)]
fn handle_export(options: GlobalOptions, output: Output) -> anyhow::Result<()> {
    let GlobalOptions {
        device,
        emulate,
        json,
        ..
    } = options;
    ensure!(
        emulate.is_none(),
        "safe backup export does not support --emulate; use fujicli-dev only for explicit protocol research"
    );

    let mut camera = usb::get_native_camera(device, emulate)?;

    let backup = camera.export_backup(BackupPurpose::Portable)?;
    let artifact_sha256 = backup.fingerprint();
    output.write_all(backup.as_bytes())?;
    if !output.is_stdout() {
        if json {
            let result = serde_json::json!({ "artifactSha256": artifact_sha256 });
            write_stdout_line(format_args!("{}", serde_json::to_string(&result)?))?;
        } else {
            write_stdout_line(format_args!("Artifact SHA-256: {artifact_sha256}"))?;
        }
    }

    Ok(())
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "command handlers consume parsed CLI values"
)]
fn handle_inspect(options: GlobalOptions, input: Input) -> anyhow::Result<()> {
    let backup = input.read_limited(MAX_BACKUP_ARTIFACT_BYTES, "backup input")?;
    let backup = BackupArtifact::parse(backup)?;
    let manifest = backup.manifest();
    let artifact_sha256 = backup.fingerprint();

    if options.json {
        let inspection = serde_json::json!({
            "formatVersion": BACKUP_ARTIFACT_FORMAT_VERSION,
            "purpose": manifest.purpose,
            "source": &manifest.source,
            "payloadLen": manifest.payload_len,
            "payloadSha256": manifest.payload_sha256,
            "artifactSha256": artifact_sha256,
        });
        write_stdout_line(format_args!(
            "{}",
            serde_json::to_string_pretty(&inspection)?
        ))?;
    } else {
        write_stdout_line(format_args!(
            "Format: fujicli backup v{}\nPurpose: {}\nCamera: {}\nPTP model: {}\nFirmware: {}\nPayload bytes: {}\nPayload SHA-256: {}\nArtifact SHA-256: {}\nSource serial SHA-256: {}",
            BACKUP_ARTIFACT_FORMAT_VERSION,
            manifest.purpose,
            manifest.source.camera_name,
            manifest.source.model,
            manifest.source.firmware,
            manifest.payload_len,
            manifest.payload_sha256,
            artifact_sha256,
            manifest.source.serial_sha256,
        ))?;
    }
    Ok(())
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "command handlers consume parsed CLI values"
)]
fn handle_import(options: GlobalOptions, args: BackupImportArgs) -> anyhow::Result<()> {
    let BackupImportArgs {
        input,
        yes,
        dry_run,
        recovery_backup,
        expect_sha256,
        target_serial_sha256,
        allow_stdin,
    } = args;
    let GlobalOptions {
        device,
        emulate,
        json,
        ..
    } = options;
    let emulated = emulate.is_some();
    ensure_import_confirmation(yes, dry_run, emulated)?;
    if let Some(recovery_backup) = &recovery_backup {
        ensure!(
            !recovery_backup.is_stdout(),
            "--recovery-backup requires a file path, not stdout"
        );
    }
    let mode = if dry_run {
        ImportMode::DryRun
    } else {
        ImportMode::Restore
    };
    let source = if input.is_stdin() {
        let permission = if allow_stdin {
            StdinPermission::Allowed
        } else {
            StdinPermission::Denied
        };
        ImportSource::Stdin(permission)
    } else {
        ImportSource::File
    };
    ensure_import_input_policy(mode, source, expect_sha256.as_deref())?;

    let backup = input.read_limited(MAX_BACKUP_ARTIFACT_BYTES, "backup input")?;
    let backup = BackupArtifact::parse(backup)?;
    if let Some(expected) = &expect_sha256 {
        backup.verify_fingerprint(expected)?;
    }
    let mut camera = usb::get_native_camera(device, emulate)?;
    let usb_id = camera.connected_usb_id();
    let binding = target_serial_sha256
        .as_deref()
        .unwrap_or(&backup.manifest().source.serial_sha256)
        .parse::<SerialFingerprint>()?;
    let mut session = camera.preflight_backup_restore(&binding)?;
    if dry_run {
        let target = session.target_identity();
        if target_serial_sha256.is_some() {
            backup.validate_target(&target, target_serial_sha256.as_deref())?;
        }
        return report_dry_run(&backup, &target, target_serial_sha256.is_some(), json);
    }
    let target = session.target_identity();
    backup.validate_target(&target, target_serial_sha256.as_deref())?;
    let recovery_backup = recovery_backup
        .ok_or_else(|| anyhow::anyhow!("backup import requires --recovery-backup"))?;
    let recovery = session.export_recovery()?;
    let camera_name = session.evidence().camera_name;
    let accepted = restore_after_recovery_saved(
        &recovery,
        &backup,
        |bytes| recovery_backup.write_all_new(bytes),
        |backup| {
            warn!(
                "{}",
                validated_backup_import_target_warning(camera_name, &usb_id)
            );
            interrupt::critical_camera_write("backup restore", || {
                session.restore(backup, target_serial_sha256.as_deref())
            })
        },
    )?;
    drop(session);
    drop(camera);

    warn!(
        "PTP restore was accepted by {camera_name}; waiting for a fresh camera session to verify restored state"
    );
    let mut camera = usb::reconnect_camera_by_serial(&binding, BACKUP_RECONNECT_TIMEOUT)
        .context("backup restore was accepted, but the target camera did not reconnect for persistence verification; camera state is unknown and the restore must not be retried automatically")?;
    let observed = camera
        .export_backup(BackupPurpose::Recovery)
        .context("backup restore was accepted, but exporting a fresh-session verification backup failed; camera state is unknown and the restore must not be retried automatically")?;
    backup.validate_target(&observed.manifest().source, target_serial_sha256.as_deref())?;
    anyhow::ensure!(
        observed.manifest().source == target,
        "fresh-session backup identity changed after restore; camera state is unknown"
    );
    let verified = accepted.verify_post_restore_export(&backup, &observed)?;
    warn!(
        "Backup restore was semantically verified and persisted across a fresh camera session (payload SHA-256 {})",
        verified.payload_sha256()
    );

    Ok(())
}

pub fn handle(cmd: BackupCmd, options: GlobalOptions) -> anyhow::Result<()> {
    match cmd {
        BackupCmd::Export { output } => handle_export(options, output),
        BackupCmd::Inspect { input } => handle_inspect(options, input),
        BackupCmd::Import(args) => handle_import(options, args),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ImportMode, ImportSource, StdinPermission, ensure_import_confirmation,
        ensure_import_input_policy, restore_after_recovery_saved,
    };
    use fujicli::features::backup::{BackupArtifact, BackupIdentity, BackupPurpose, sha256_hex};

    #[test]
    fn safe_backup_import_rejects_emulated_target() {
        let error = ensure_import_confirmation(true, false, true)
            .expect_err("safe import must not authorize an emulated restore target");

        assert!(error.to_string().contains("--emulate"));
    }

    #[test]
    fn destructive_stdin_requires_separate_opt_in_and_external_fingerprint() {
        let missing_opt_in = ensure_import_input_policy(
            ImportMode::Restore,
            ImportSource::Stdin(StdinPermission::Denied),
            Some("fingerprint"),
        )
        .expect_err("fingerprint alone must not authorize stdin restore");
        let missing_fingerprint = ensure_import_input_policy(
            ImportMode::Restore,
            ImportSource::Stdin(StdinPermission::Allowed),
            None,
        )
        .expect_err("stdin opt-in alone must not authorize an unpinned artifact");

        assert!(missing_opt_in.to_string().contains("--allow-stdin"));
        assert!(missing_fingerprint.to_string().contains("--expect-sha256"));
    }

    #[test]
    fn recovery_write_failure_prevents_restore_call() -> anyhow::Result<()> {
        let identity = BackupIdentity {
            camera_name: "FUJIFILM X-T5".to_owned(),
            vendor_id: 0x04cb,
            product_id: 0x02fc,
            manufacturer: "FUJIFILM".to_owned(),
            model: "X-T5".to_owned(),
            firmware: "4.31".to_owned(),
            serial_sha256: sha256_hex(b"serial"),
        };
        let recovery =
            BackupArtifact::create(BackupPurpose::Recovery, identity.clone(), b"current state")?;
        let backup = BackupArtifact::create(BackupPurpose::Portable, identity, b"new state")?;
        let mut restore_calls = 0;

        let result = restore_after_recovery_saved(
            &recovery,
            &backup,
            |_| anyhow::bail!("disk full"),
            |_| {
                restore_calls += 1;
                Ok(())
            },
        );

        assert!(result.is_err());
        assert_eq!(restore_calls, 0);
        Ok(())
    }
}
