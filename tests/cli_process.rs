use std::{
    io::Write as _,
    path::Path,
    process::{Command, Output, Stdio},
};

#[cfg(unix)]
use std::{
    io::{BufRead as _, BufReader, Read as _},
    sync::mpsc,
    thread,
    time::Duration,
};

use fujicli::features::backup::{BackupArtifact, BackupIdentity, BackupPurpose, sha256_hex};
use tempfile::tempdir;

fn run(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fujicli"))
        .args(arguments)
        .output()
        .expect("fujicli process must start")
}

fn sample_backup_artifact() -> anyhow::Result<BackupArtifact> {
    BackupArtifact::create(
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
    )
}

#[cfg(unix)]
#[test]
fn idle_sigint_exits_130_without_writing_stdout() -> anyhow::Result<()> {
    let directory = tempdir()?;
    let recovery = directory.path().join("recovery.fbk");
    let mut child = Command::new(env!("CARGO_BIN_EXE_fujicli"))
        .args([
            "-vvv",
            "backup",
            "import",
            "-",
            "--yes",
            "--recovery-backup",
        ])
        .arg(&recovery)
        .args([
            "--expect-sha256",
            "0000000000000000000000000000000000000000000000000000000000000000",
            "--allow-stdin",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let stdin = child.stdin.take().expect("piped stdin must be available");
    let stdout = child.stdout.take().expect("piped stdout must be available");
    let stderr = child.stderr.take().expect("piped stderr must be available");

    let stdout_reader = thread::spawn(move || {
        let mut output = Vec::new();
        BufReader::new(stdout).read_to_end(&mut output)?;
        Ok::<_, std::io::Error>(output)
    });
    let (ready_tx, ready_rx) = mpsc::channel();
    let stderr_reader = thread::spawn(move || {
        let mut output = Vec::new();
        for line in BufReader::new(stderr).lines() {
            let line = line?;
            if line.contains("waiting for backup input on stdin") {
                let _ = ready_tx.send(());
            }
            output.extend_from_slice(line.as_bytes());
            output.push(b'\n');
        }
        Ok::<_, std::io::Error>(output)
    });

    if ready_rx.recv_timeout(Duration::from_secs(5)).is_err() {
        child.kill()?;
        child.wait()?;
        drop(stdin);
        stdout_reader
            .join()
            .expect("stdout reader thread must not panic")?;
        let stderr = stderr_reader
            .join()
            .expect("stderr reader thread must not panic")?;
        anyhow::bail!(
            "process did not report stdin readiness; stderr: {}",
            String::from_utf8_lossy(&stderr)
        );
    }

    let signal = Command::new("kill")
        .args(["-INT", &child.id().to_string()])
        .status()?;
    assert!(signal.success(), "kill -INT must succeed");

    let (finished_tx, finished_rx) = mpsc::channel();
    let child_id = child.id();
    let watchdog = thread::spawn(move || {
        if finished_rx.recv_timeout(Duration::from_secs(5)).is_err() {
            drop(
                Command::new("kill")
                    .args(["-KILL", &child_id.to_string()])
                    .status(),
            );
        }
    });
    let status = child.wait()?;
    let _ = finished_tx.send(());
    watchdog.join().expect("watchdog thread must not panic");
    drop(stdin);
    let stdout = stdout_reader
        .join()
        .expect("stdout reader thread must not panic")?;
    let stderr = stderr_reader
        .join()
        .expect("stderr reader thread must not panic")?;

    assert_eq!(status.code(), Some(130));
    assert!(stdout.is_empty());
    assert!(String::from_utf8_lossy(&stderr).contains("interrupted"));
    assert!(!recovery.exists());
    Ok(())
}

#[test]
fn closed_stdout_is_a_successful_process_exit() -> anyhow::Result<()> {
    let artifact = sample_backup_artifact()?;
    let mut child = Command::new(env!("CARGO_BIN_EXE_fujicli"))
        .args(["backup", "inspect", "-", "--json"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    drop(child.stdout.take().expect("piped stdout must be available"));
    let mut stdin = child.stdin.take().expect("piped stdin must be available");
    stdin.write_all(artifact.as_bytes())?;
    drop(stdin);
    let output = child.wait_with_output()?;

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    Ok(())
}

#[test]
fn backup_inspect_json_has_a_stable_process_contract() -> anyhow::Result<()> {
    let directory = tempdir()?;
    let path = directory.path().join("camera.fbk");
    let artifact = sample_backup_artifact()?;
    std::fs::write(&path, artifact.as_bytes())?;
    let path = path.to_string_lossy();

    let output = run(&["backup", "inspect", &path, "--json"]);

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        String::from_utf8(output.stdout)?,
        concat!(
            "{\n",
            "  \"artifactSha256\": \"85800ca4598398f7443de9be0cd19ea3af0aa102b43db231a358a56330ee3b42\",\n",
            "  \"formatVersion\": 1,\n",
            "  \"payloadLen\": 21,\n",
            "  \"payloadSha256\": \"09e31808e0b2da1c8a8110bbbb6f20cb88fec0c1f03d91664b7876ac3cc6791b\",\n",
            "  \"purpose\": \"portable\",\n",
            "  \"source\": {\n",
            "    \"cameraName\": \"FUJIFILM X-T5\",\n",
            "    \"firmware\": \"4.31\",\n",
            "    \"manufacturer\": \"FUJIFILM\",\n",
            "    \"model\": \"X-T5\",\n",
            "    \"productId\": 764,\n",
            "    \"serialSha256\": \"ef1c012340f88741a524f3934ecd8e41c4fac4f7c0dbdcb0d2b30db712d58fbf\",\n",
            "    \"vendorId\": 1227\n",
            "  }\n",
            "}\n",
        )
    );
    assert!(output.stderr.is_empty());
    Ok(())
}

#[test]
fn docs_contract_help_needs_no_hardware_and_is_stdout_only() {
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
fn long_help_adds_safe_examples_and_web_routes_without_expanding_short_help() {
    let long_help = run(&["--help"]);
    let short_help = run(&["-h"]);

    for output in [&long_help, &short_help] {
        assert_eq!(output.status.code(), Some(0));
        assert!(output.stderr.is_empty());
    }

    let long_help = String::from_utf8(long_help.stdout).expect("long help must be UTF-8");
    for expected in [
        "Examples:",
        "fujicli device list",
        "fujicli device info",
        "https://github.com/po4yka/fuji-cli/blob/main/docs/README.md",
        "https://github.com/po4yka/fuji-cli/issues",
        "https://github.com/po4yka/fuji-cli/blob/main/SUPPORT.md",
        "https://github.com/po4yka/fuji-cli/security/policy",
    ] {
        assert!(
            long_help.contains(expected),
            "missing {expected:?}\n{long_help}"
        );
    }

    let short_help = String::from_utf8(short_help.stdout).expect("short help must be UTF-8");
    assert!(!short_help.contains("Examples:"));
    assert!(!short_help.contains("https://github.com/po4yka/fuji-cli"));
}

#[test]
fn completion_writes_supported_shell_scripts_to_stdout() {
    let cases = [
        ("bash", "_fujicli()"),
        ("zsh", "#compdef fujicli"),
        ("fish", "complete -c fujicli"),
        (
            "powershell",
            "Register-ArgumentCompleter -Native -CommandName 'fujicli'",
        ),
    ];

    for (shell, marker) in cases {
        let output = run(&["completion", shell]);

        assert_eq!(output.status.code(), Some(0), "shell: {shell}");
        assert!(output.stderr.is_empty(), "shell: {shell}");
        let script = String::from_utf8(output.stdout).expect("completion output must be UTF-8");
        assert!(script.contains(marker), "shell: {shell}\n{script}");
    }
}

#[test]
fn schema_driven_option_help_names_the_setting() {
    let output = run(&["simulation", "set", "--help"]);

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let help = String::from_utf8(output.stdout).expect("help output must be UTF-8");
    assert!(help.contains("High ISO Noise Reduction"), "{help}");
}

#[test]
fn schema_driven_numeric_help_shows_range_and_step() {
    let output = run(&["simulation", "set", "--help"]);

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let help = String::from_utf8(output.stdout).expect("help output must be UTF-8");
    let help = help.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        help.contains("Clarity Schema range: -5..=5. Step: 1."),
        "{help}"
    );
}

#[test]
fn schema_driven_string_help_shows_length_limit() {
    let output = run(&["simulation", "set", "--help"]);

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let help = String::from_utf8(output.stdout).expect("help output must be UTF-8");
    let help = help.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        help.contains("Custom Setting Name Maximum length: 25 characters."),
        "{help}"
    );
}

#[test]
fn schema_driven_enum_help_lists_canonical_values() {
    let output = run(&["simulation", "set", "--help"]);

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let help = String::from_utf8(output.stdout).expect("help output must be UTF-8");
    let help = help.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        help.contains("Color Space Schema values: srgb, adobe_rgb."),
        "{help}"
    );
}

#[test]
fn schema_driven_numeric_lookup_help_lists_values_in_numeric_order() {
    let output = run(&["image", "render", "--help"]);

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let help = String::from_utf8(output.stdout).expect("help output must be UTF-8");
    let help = help.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        help.contains("Exposure Offset Schema values: -3.0, -2.7, -2.3"),
        "{help}"
    );
    assert!(help.contains("2.3, 2.7, 3.0."), "{help}");
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
fn docs_contract_invalid_grammar_is_a_usage_error_on_stderr() {
    let output = run(&["not-a-command"]);

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unrecognized subcommand"));
}

#[test]
fn irrelevant_leaf_option_is_a_usage_error() {
    let output = run(&["device", "list", "--device", "1.2"]);

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unexpected argument '--device'"));
}

#[test]
fn native_simulation_write_requires_exact_serial_binding_before_usb_lookup() {
    let output = run(&["simulation", "set", "c1", "--device", "255.255"]);

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--target-serial-sha256"));
    assert!(!stderr.contains("No USB device found"));
}

#[test]
fn simulation_set_json_is_a_recognized_flag_not_a_usage_error() {
    let output = run(&[
        "simulation",
        "set",
        "c1",
        "--json",
        "--target-serial-sha256",
        "0000000000000000000000000000000000000000000000000000000000000000",
        "--device",
        "255.255",
    ]);

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("No USB device found"), "{stderr}");
}

#[test]
fn simulation_import_json_is_a_recognized_flag_not_a_usage_error() -> anyhow::Result<()> {
    let directory = tempdir()?;
    let input = directory.path().join("c1.json");
    std::fs::write(&input, b"{}")?;
    let input = input.to_string_lossy();

    let output = run(&[
        "simulation",
        "import",
        "c1",
        &input,
        "--json",
        "--target-serial-sha256",
        "0000000000000000000000000000000000000000000000000000000000000000",
        "--device",
        "255.255",
    ]);

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("No USB device found"), "{stderr}");
    Ok(())
}

#[test]
fn raw_conversion_requires_exact_serial_binding_before_file_or_usb_access() {
    let output = run(&[
        "image",
        "render",
        "/definitely/missing.raf",
        "/tmp/unused.jpg",
        "--device",
        "255.255",
    ]);

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--target-serial-sha256"));
    assert!(!stderr.contains("No such file or directory"));
    assert!(!stderr.contains("No USB device found"));
}

#[test]
fn raw_conversion_rejects_simulation_slot_before_file_or_usb_access() {
    let output = run(&[
        "image",
        "render",
        "--slot",
        "c1",
        "--target-serial-sha256",
        "0000000000000000000000000000000000000000000000000000000000000000",
        "/definitely/missing.raf",
        "/tmp/unused.jpg",
        "--device",
        "255.255",
    ]);

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unexpected argument '--slot'"));
    assert!(!stderr.contains("No such file or directory"));
    assert!(!stderr.contains("No USB device found"));
}

#[test]
fn raw_conversion_force_allows_existing_output_before_input_access() -> anyhow::Result<()> {
    let directory = tempdir()?;
    let destination = directory.path().join("rendered.jpg");
    std::fs::write(&destination, b"existing JPEG")?;
    let destination = destination.to_string_lossy();

    let output = run(&[
        "image",
        "render",
        "--force",
        "--target-serial-sha256",
        "0000000000000000000000000000000000000000000000000000000000000000",
        "/definitely/missing.raf",
        &destination,
        "--device",
        "255.255",
    ]);

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("reading RAF image metadata"), "{stderr}");
    assert!(!stderr.contains("already exists"), "{stderr}");
    assert!(!stderr.contains("No USB device found"), "{stderr}");
    assert_eq!(std::fs::read(&*destination)?, b"existing JPEG");
    Ok(())
}

#[test]
fn raw_recovery_requires_exact_serial_binding_before_output_or_usb_access() {
    let output = run(&[
        "image",
        "recover",
        "42",
        "/definitely/missing/recovered.jpg",
        "--device",
        "255.255",
    ]);

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--target-serial-sha256"));
    assert!(!stderr.contains("No such file or directory"));
    assert!(!stderr.contains("No USB device found"));
}

#[test]
fn raw_recovery_never_deletes_after_stdout() {
    let output = run(&[
        "image",
        "recover",
        "--target-serial-sha256",
        "0000000000000000000000000000000000000000000000000000000000000000",
        "--delete-after-save",
        "42",
        "-",
        "--device",
        "255.255",
    ]);

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("requires a file output"));
    assert!(!stderr.contains("No USB device found"));
}

#[test]
fn raw_recovery_force_allows_existing_output_before_usb_access() -> anyhow::Result<()> {
    let directory = tempdir()?;
    let destination = directory.path().join("recovered.jpg");
    std::fs::write(&destination, b"existing JPEG")?;
    let destination = destination.to_string_lossy();

    let output = run(&[
        "image",
        "recover",
        "--force",
        "--target-serial-sha256",
        "0000000000000000000000000000000000000000000000000000000000000000",
        "42",
        &destination,
        "--device",
        "255.255",
    ]);

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("No USB device found"), "{stderr}");
    assert!(!stderr.contains("already exists"), "{stderr}");
    assert_eq!(std::fs::read(&*destination)?, b"existing JPEG");
    Ok(())
}

#[test]
fn simulation_set_rejects_emulation_as_a_usage_error() {
    let output = run(&[
        "simulation",
        "set",
        "c1",
        "--emulate",
        "04cb:02f7",
        "--device",
        "255.255",
    ]);

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unexpected argument '--emulate'"));
    assert!(!stderr.contains("No USB device found"));
}

#[test]
fn simulation_import_rejects_emulation_before_file_or_usb_access() {
    let output = run(&[
        "simulation",
        "import",
        "c1",
        "/definitely/missing.json",
        "--emulate",
        "04cb:02f7",
        "--device",
        "255.255",
    ]);

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unexpected argument '--emulate'"));
    assert!(!stderr.contains("reading simulation JSON metadata"));
    assert!(!stderr.contains("No such file or directory"));
    assert!(!stderr.contains("No USB device found"));
}

#[test]
fn simulation_get_rejects_emulation_before_usb_lookup() {
    let output = run(&[
        "simulation",
        "get",
        "c1",
        "--emulate",
        "04cb:02f7",
        "--device",
        "255.255",
    ]);

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unexpected argument '--emulate'"));
    assert!(!stderr.contains("No USB device found"));
}

#[test]
fn emulation_is_not_applicable_to_device_list() {
    let output = run(&["device", "list", "--emulate", "04cb:02f7"]);

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unexpected argument '--emulate'"));
}

#[test]
fn backup_inspect_rejects_emulation_before_file_access() {
    let output = run(&[
        "backup",
        "inspect",
        "/definitely/missing.fbk",
        "--emulate",
        "04cb:02f7",
    ]);

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unexpected argument '--emulate'"));
    assert!(!stderr.contains("No such file or directory"));
}

#[test]
fn backup_import_rejects_emulation_before_file_access() {
    let output = run(&[
        "backup",
        "import",
        "/definitely/missing.fbk",
        "--dry-run",
        "--emulate",
        "04cb:02f7",
    ]);

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unexpected argument '--emulate'"));
    assert!(!stderr.contains("No such file or directory"));
}

fn assert_reverse_absent(arguments: &[&str]) {
    let output = run(arguments);

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unrecognized subcommand"));
    assert!(!stderr.contains("No USB device found"));
}

#[test]
fn production_binary_rejects_reverse_command() {
    assert_reverse_absent(&["device", "reverse", "info"]);
}

#[test]
fn production_binary_rejects_reverse_alias() {
    assert_reverse_absent(&["device", "r", "info"]);
}

#[test]
fn production_binary_rejects_device_alias_with_reverse_command() {
    assert_reverse_absent(&["d", "reverse", "info"]);
}

#[test]
fn production_binary_rejects_device_and_reverse_aliases() {
    assert_reverse_absent(&["d", "r", "info"]);
}

#[test]
fn image_render_rejects_emulation_before_file_or_usb_access() {
    let output = run(&[
        "image",
        "render",
        "/definitely/missing.raf",
        "/tmp/unused.jpg",
        "--emulate",
        "04cb:02f7",
        "--device",
        "255.255",
    ]);

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unexpected argument '--emulate'"));
    assert!(!stderr.contains("reading RAF image metadata"));
    assert!(!stderr.contains("No such file or directory"));
    assert!(!stderr.contains("No USB device found"));
}

#[test]
fn image_recovery_rejects_emulation_before_output_or_usb_access() {
    let output = run(&[
        "image",
        "recover",
        "42",
        "/definitely/missing/recovered.jpg",
        "--emulate",
        "04cb:02f7",
        "--device",
        "255.255",
    ]);

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unexpected argument '--emulate'"));
    assert!(!stderr.contains("No such file or directory"));
    assert!(!stderr.contains("No USB device found"));
}

#[test]
fn backup_import_without_confirmation_fails_before_device_io() {
    let output = run(&["backup", "import", "missing.fbk"]);

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--yes"));
}

#[test]
fn malformed_backup_fingerprint_is_a_usage_error_before_file_access() {
    let output = run(&[
        "backup",
        "import",
        "/definitely/missing.fbk",
        "--yes",
        "--recovery-backup",
        "recovery.fbk",
        "--expect-sha256",
        "not-a-sha256",
    ]);

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("exactly 64 lowercase hexadecimal characters"));
    assert!(!stderr.contains("No such file or directory"));
}

#[test]
fn malformed_backup_target_fingerprint_is_a_usage_error_before_file_access() {
    let output = run(&[
        "backup",
        "import",
        "/definitely/missing.fbk",
        "--dry-run",
        "--target-serial-sha256",
        "not-a-sha256",
    ]);

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("exactly 64 lowercase hexadecimal characters"));
    assert!(!stderr.contains("No such file or directory"));
}

#[test]
fn backup_dry_run_rejects_restore_confirmation_before_file_access() {
    let output = run(&[
        "backup",
        "import",
        "/definitely/missing.fbk",
        "--dry-run",
        "--yes",
    ]);

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("cannot be used with"));
    assert!(stderr.contains("--dry-run"));
    assert!(stderr.contains("--yes"));
    assert!(!stderr.contains("No such file or directory"));
}

#[test]
fn backup_dry_run_rejects_recovery_output_before_file_access() {
    let output = run(&[
        "backup",
        "import",
        "/definitely/missing.fbk",
        "--dry-run",
        "--recovery-backup",
        "recovery.fbk",
    ]);

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("cannot be used with"));
    assert!(stderr.contains("--dry-run"));
    assert!(stderr.contains("--recovery-backup"));
    assert!(!stderr.contains("No such file or directory"));
}

#[test]
fn backup_recovery_output_rejects_stdout_as_a_usage_error() {
    let output = run(&[
        "backup",
        "import",
        "/definitely/missing.fbk",
        "--yes",
        "--recovery-backup",
        "-",
        "--expect-sha256",
        "0000000000000000000000000000000000000000000000000000000000000000",
    ]);

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--recovery-backup"));
    assert!(stderr.contains("file path"));
    assert!(!stderr.contains("No such file or directory"));
}

#[test]
fn backup_export_json_rejects_stdout_as_a_usage_error() {
    let output = run(&["backup", "export", "-", "--json"]);

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--json"));
    assert!(!stderr.contains("No device"));
}

#[test]
fn backup_allow_stdin_rejects_file_input_as_a_usage_error() {
    let output = run(&[
        "backup",
        "import",
        "/definitely/missing.fbk",
        "--yes",
        "--recovery-backup",
        "recovery.fbk",
        "--expect-sha256",
        "0000000000000000000000000000000000000000000000000000000000000000",
        "--allow-stdin",
    ]);

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--allow-stdin"));
    assert!(stderr.contains("input '-'"));
    assert!(!stderr.contains("No such file or directory"));
}

#[test]
fn destructive_backup_stdin_requires_opt_in_as_a_usage_error() {
    let output = run(&[
        "backup",
        "import",
        "-",
        "--yes",
        "--recovery-backup",
        "recovery.fbk",
        "--expect-sha256",
        "0000000000000000000000000000000000000000000000000000000000000000",
    ]);

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("input '-'"));
    assert!(stderr.contains("--allow-stdin"));
    assert!(!stderr.contains("backup artifact is truncated"));
}

#[test]
fn backup_export_rejects_existing_output_before_usb_access() -> anyhow::Result<()> {
    let directory = tempdir()?;
    let destination = directory.path().join("camera.fbk");
    std::fs::write(&destination, b"existing backup")?;
    let destination = destination.to_string_lossy();

    let output = run(&["backup", "export", &destination, "--device", "255.255"]);

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("already exists"), "{stderr}");
    assert!(stderr.contains("--force"), "{stderr}");
    assert!(!stderr.contains("No USB device found"), "{stderr}");
    assert_eq!(std::fs::read(&*destination)?, b"existing backup");
    Ok(())
}

#[test]
fn backup_export_force_allows_existing_output_before_usb_access() -> anyhow::Result<()> {
    let directory = tempdir()?;
    let destination = directory.path().join("camera.fbk");
    std::fs::write(&destination, b"existing backup")?;
    let destination = destination.to_string_lossy();

    let output = run(&[
        "backup",
        "export",
        "--force",
        &destination,
        "--device",
        "255.255",
    ]);

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("No USB device found"), "{stderr}");
    assert!(!stderr.contains("already exists"), "{stderr}");
    assert_eq!(std::fs::read(&*destination)?, b"existing backup");
    Ok(())
}

#[test]
fn simulation_export_rejects_existing_output_before_usb_access() -> anyhow::Result<()> {
    let directory = tempdir()?;
    let destination = directory.path().join("c1.json");
    std::fs::write(&destination, b"existing simulation")?;
    let destination = destination.to_string_lossy();

    let output = run(&[
        "simulation",
        "export",
        "c1",
        &destination,
        "--device",
        "255.255",
    ]);

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("already exists"), "{stderr}");
    assert!(stderr.contains("--force"), "{stderr}");
    assert!(!stderr.contains("No USB device found"), "{stderr}");
    assert_eq!(std::fs::read(&*destination)?, b"existing simulation");
    Ok(())
}

#[test]
fn simulation_export_force_allows_existing_output_before_usb_access() -> anyhow::Result<()> {
    let directory = tempdir()?;
    let destination = directory.path().join("c1.json");
    std::fs::write(&destination, b"existing simulation")?;
    let destination = destination.to_string_lossy();

    let output = run(&[
        "simulation",
        "export",
        "--force",
        "c1",
        &destination,
        "--device",
        "255.255",
    ]);

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("No USB device found"), "{stderr}");
    assert!(!stderr.contains("already exists"), "{stderr}");
    assert_eq!(std::fs::read(&*destination)?, b"existing simulation");
    Ok(())
}

#[test]
fn docs_contract_offline_backup_inspect_needs_no_hardware() -> anyhow::Result<()> {
    let directory = tempdir()?;
    let path = directory.path().join("camera.fbk");
    let artifact = sample_backup_artifact()?;
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
    let artifact = sample_backup_artifact()?;
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
