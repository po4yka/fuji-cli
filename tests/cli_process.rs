use std::process::{Command, Output};

fn run(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fujicli"))
        .args(arguments)
        .output()
        .expect("fujicli process must start")
}

#[test]
fn help_is_stdout_only_and_successful() {
    let output = run(&["--help"]);

    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("Usage: fujicli [OPTIONS] <COMMAND>"));
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
