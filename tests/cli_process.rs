use std::{
    path::Path,
    process::{Command, Output},
};

use fujicli::features::backup::{BackupArtifact, BackupIdentity, BackupPurpose, sha256_hex};
use tempfile::tempdir;

fn run(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fujicli"))
        .args(arguments)
        .output()
        .expect("fujicli process must start")
}

#[test]
fn help_is_stdout_only_and_successful() {
    let output = run(&["--help"]);
    let executable_name = Path::new(env!("CARGO_BIN_EXE_fujicli"))
        .file_name()
        .expect("fujicli executable path must have a file name")
        .to_string_lossy();
    let expected_usage = format!("Usage: {executable_name} [OPTIONS] <COMMAND>");

    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains(&expected_usage));
    assert!(output.stderr.is_empty());
}

#[test]
fn version_has_the_stable_machine_readable_shape() {
    let output = run(&["--version"]);

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("version output must be UTF-8"),
        format!("fujicli {}\n", env!("CARGO_PKG_VERSION"))
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn invalid_subcommand_is_a_usage_error_on_stderr() {
    let output = run(&["not-a-command"]);

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unrecognized subcommand"));
}

#[test]
fn backup_import_without_confirmation_fails_before_device_io() {
    let output = run(&["backup", "import", "missing.fbk"]);

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--yes"));
}

#[test]
fn backup_inspect_validates_artifact_without_usb_device() -> anyhow::Result<()> {
    let directory = tempdir()?;
    let path = directory.path().join("camera.fbk");
    let artifact = BackupArtifact::create(
        BackupPurpose::Portable,
        BackupIdentity {
            camera_name: "FUJIFILM X-T5".to_owned(),
            vendor_id: 0x04cb,
            product_id: 0x02fc,
            manufacturer: "FUJIFILM".to_owned(),
            model: "X-T5".to_owned(),
            firmware: "4.31".to_owned(),
            serial_sha256: sha256_hex(b"process-test-serial"),
        },
        b"native backup payload",
    )?;
    std::fs::write(&path, artifact.as_bytes())?;
    let path = path.to_string_lossy();

    let output = run(&["backup", "inspect", &path]);

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout)?;
    assert!(stdout.contains("FUJIFILM X-T5"));
    assert!(stdout.contains("Payload SHA-256"));
    assert!(output.stderr.is_empty());
    Ok(())
}

#[test]
fn invalid_backup_import_fails_before_usb_device_access() -> anyhow::Result<()> {
    let directory = tempdir()?;
    let input = directory.path().join("invalid.fbk");
    let recovery = directory.path().join("recovery.fbk");
    std::fs::write(&input, vec![b'x'; 64])?;
    let input = input.to_string_lossy();
    let recovery = recovery.to_string_lossy();
    let expected_sha256 = "00".repeat(32);

    let output = run(&[
        "backup",
        "import",
        &input,
        "--yes",
        "--recovery-backup",
        &recovery,
        "--expect-sha256",
        &expected_sha256,
    ]);

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr)?;
    assert!(stderr.contains("not a fujicli backup artifact"));
    assert!(!stderr.contains("supported camera"));
    Ok(())
}

#[test]
fn backup_fingerprint_mismatch_fails_before_usb_or_recovery_export() -> anyhow::Result<()> {
    let directory = tempdir()?;
    let input = directory.path().join("camera.fbk");
    let recovery = directory.path().join("recovery.fbk");
    let artifact = BackupArtifact::create(
        BackupPurpose::Portable,
        BackupIdentity {
            camera_name: "FUJIFILM X-T5".to_owned(),
            vendor_id: 0x04cb,
            product_id: 0x02fc,
            manufacturer: "FUJIFILM".to_owned(),
            model: "X-T5".to_owned(),
            firmware: "4.31".to_owned(),
            serial_sha256: sha256_hex(b"process-test-serial"),
        },
        b"native backup payload",
    )?;
    std::fs::write(&input, artifact.as_bytes())?;
    let input_argument = input.to_string_lossy();
    let recovery_argument = recovery.to_string_lossy();
    let wrong_fingerprint = "00".repeat(32);

    let output = run(&[
        "backup",
        "import",
        &input_argument,
        "--yes",
        "--recovery-backup",
        &recovery_argument,
        "--expect-sha256",
        &wrong_fingerprint,
    ]);

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr)?;
    assert!(stderr.contains("does not match --expect-sha256"));
    assert!(!stderr.contains("supported camera"));
    assert!(!recovery.exists());
    Ok(())
}
