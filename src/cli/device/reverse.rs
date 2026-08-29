use anyhow::anyhow;
use clap::Subcommand;
use fujicli::{
    Camera,
    features::backup,
    generated::{
        cli::SIMULATION_PROP_CODES,
        options::{CustomSetting, UsbMode},
    },
    ptp::{CommandCode, DevicePropCode, option::SimulationSetting},
};
use log::{debug, warn};
use strum::IntoEnumIterator;

use crate::cli::{
    GlobalOptions,
    backup::{MAX_BACKUP_INPUT_BYTES, backup_import_target_warning},
    common::{
        file::{Input, Output},
        usb,
    },
};

#[derive(Subcommand, Debug, Clone)]
pub enum ReverseCmd {
    /// Attempt to manage backups
    #[command(alias = "b", subcommand)]
    Backup(ReverseBackupCmd),

    /// Attempt to get camera info
    #[command(alias = "i")]
    Info,

    /// Get information about supported simulation management commands
    #[command(alias = "s")]
    Simulation,
}

#[derive(Subcommand, Debug, Clone)]
pub enum ReverseBackupCmd {
    /// Attempt to export a backup from an unknown camera
    #[command(alias = "e")]
    Export {
        /// Output file (use '-' to write to stdout)
        output: Output,
    },

    /// Attempt to import a backup into an unknown camera
    #[command(alias = "i")]
    Import {
        /// Input file (use '-' to read from stdin)
        input: Input,

        /// Confirm sending this opaque backup to the selected camera
        #[arg(long, required = true)]
        yes: bool,

        /// Allow restore without a known camera model or format identity
        #[arg(long, required = true, requires = "yes")]
        allow_unknown_camera: bool,
    },
}

macro_rules! try_call {
    ($call:expr $(,)?) => {{
        let result = $call;
        match &result {
            Ok(_) => debug!("{}: succeeded", stringify!($call)),
            Err(_) => debug!("{}: unavailable", stringify!($call)),
        }
        result
    }};
}

#[derive(Default)]
struct ProbeSummary {
    succeeded: usize,
    failed: usize,
}

impl ProbeSummary {
    const fn observe<T, E>(&mut self, result: &Result<T, E>) {
        if result.is_ok() {
            self.succeeded += 1;
        } else {
            self.failed += 1;
        }
    }

    fn finish(self, context: &'static str) -> anyhow::Result<()> {
        debug!(
            "Reverse {context} probes complete: {} succeeded, {} failed",
            self.succeeded, self.failed
        );
        if self.succeeded == 0 {
            return Err(anyhow!("No {context} probes succeeded"));
        }
        Ok(())
    }
}

fn log_simulation_probe<T, E>(code: u16, result: &Result<T, E>) {
    match result {
        Ok(_) => debug!("Simulation property 0x{code:04X}: succeeded"),
        Err(_) => debug!("Simulation property 0x{code:04X}: unavailable"),
    }
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "command handlers consume parsed CLI values"
)]
fn handle_backup_export(options: GlobalOptions, output: Output) -> anyhow::Result<()> {
    let GlobalOptions { device, .. } = options;

    let location = device.ok_or_else(|| anyhow!("Device must be specified for backup export"))?;
    let usb = usb::get_usb_device_by_location(location)?;
    let mut camera = Camera::open_unknown(&usb)?;

    try_call!(camera.ptp.send(
        CommandCode::GetObjectInfo,
        &backup::EXPORT_OBJECT_INFO_HANDLE,
        None
    ))?;
    let backup = try_call!(
        camera
            .ptp
            .send(CommandCode::GetObject, &backup::OBJECT_HANDLE, None)
    )?;
    output.write_all(&backup)?;

    Ok(())
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "command handlers consume parsed CLI values"
)]
fn handle_backup_import(
    options: GlobalOptions,
    input: Input,
    yes: bool,
    allow_unknown_camera: bool,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        yes,
        "reverse backup import requires explicit --yes confirmation"
    );
    anyhow::ensure!(
        allow_unknown_camera,
        "reverse backup import requires --allow-unknown-camera"
    );
    let GlobalOptions { device, .. } = options;

    let location = device.ok_or_else(|| anyhow!("Device must be specified for backup import"))?;
    let backup = input.read_limited(MAX_BACKUP_INPUT_BYTES, "backup input")?;
    let usb = usb::get_usb_device_by_location(location)?;
    let mut camera = Camera::open_unknown(&usb)?;

    warn!(
        "{}",
        backup_import_target_warning(camera.name(), &camera.connected_usb_id(), false)
    );
    try_call!(backup::import_backup_over_ptp(&mut camera.ptp, &backup))?;

    Ok(())
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "command handlers consume parsed CLI values"
)]
fn handle_info(options: GlobalOptions) -> anyhow::Result<()> {
    let GlobalOptions { device, .. } = options;

    let location = device.ok_or_else(|| anyhow!("Device must be specified for info dump"))?;
    let usb = usb::get_usb_device_by_location(location)?;
    let mut camera = Camera::open_unknown(&usb)?;

    let mut probes = ProbeSummary::default();
    let result = try_call!(camera.ptp.get_info());
    probes.observe(&result);
    let result = try_call!(camera.ptp.get_prop_raw(UsbMode::prop_code()));
    probes.observe(&result);
    let result = try_call!(camera.ptp.get_prop_raw(DevicePropCode::FujiBatteryInfo2));
    probes.observe(&result);

    probes.finish("camera info")
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "command handlers consume parsed CLI values"
)]
fn handle_simulation(options: GlobalOptions) -> anyhow::Result<()> {
    let GlobalOptions { device, .. } = options;

    let location =
        device.ok_or_else(|| anyhow!("Device must be specified for simulation prop dump"))?;
    let usb = usb::get_usb_device_by_location(location)?;
    let mut camera = Camera::open_unknown(&usb)?;
    let mut probes = ProbeSummary::default();

    for slot in CustomSetting::iter() {
        if try_call!(slot.try_push(&mut camera.ptp)).is_err() {
            continue;
        }

        for &code in SIMULATION_PROP_CODES {
            let result = camera.ptp.get_prop_raw(code);
            log_simulation_probe(code, &result);
            probes.observe(&result);
        }
    }

    probes.finish("simulation properties")
}

pub fn handle(cmd: ReverseCmd, options: GlobalOptions) -> anyhow::Result<()> {
    match cmd {
        ReverseCmd::Backup(ReverseBackupCmd::Export { output }) => {
            handle_backup_export(options, output)
        }
        ReverseCmd::Backup(ReverseBackupCmd::Import {
            input,
            yes,
            allow_unknown_camera,
        }) => handle_backup_import(options, input, yes, allow_unknown_camera),
        ReverseCmd::Info => handle_info(options),
        ReverseCmd::Simulation => handle_simulation(options),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Mutex, MutexGuard, Once};

    use log::{Level, LevelFilter, Log, Metadata, Record};

    use super::*;

    #[derive(Clone, Debug)]
    struct CapturedRecord {
        level: Level,
        message: String,
    }

    struct CapturingLogger {
        records: Mutex<Vec<CapturedRecord>>,
    }

    impl Log for CapturingLogger {
        fn enabled(&self, _metadata: &Metadata<'_>) -> bool {
            true
        }

        fn log(&self, record: &Record<'_>) {
            self.records
                .lock()
                .expect("captured log records must remain accessible")
                .push(CapturedRecord {
                    level: record.level(),
                    message: record.args().to_string(),
                });
        }

        fn flush(&self) {}
    }

    static LOGGER: CapturingLogger = CapturingLogger {
        records: Mutex::new(Vec::new()),
    };
    static LOGGER_INIT: Once = Once::new();
    static LOGGER_TEST_LOCK: Mutex<()> = Mutex::new(());

    fn start_log_capture() -> MutexGuard<'static, ()> {
        let guard = LOGGER_TEST_LOCK
            .lock()
            .expect("logger tests must run serially");
        LOGGER_INIT.call_once(|| {
            log::set_logger(&LOGGER).expect("test logger must be installed once");
            log::set_max_level(LevelFilter::Debug);
        });
        LOGGER
            .records
            .lock()
            .expect("captured log records must remain accessible")
            .clear();
        guard
    }

    #[derive(Debug)]
    struct SensitiveProbeResult {
        backup_bytes: Vec<u8>,
        custom_setting_name: &'static str,
        serial_number: &'static str,
    }

    #[test]
    fn successful_reverse_call_does_not_log_returned_value() {
        let _capture = start_log_capture();

        let private_value = SensitiveProbeResult {
            backup_bytes: vec![0, 0xff, 0x52, 0x41, 0x46],
            custom_setting_name: "reverse-private-custom-setting",
            serial_number: "reverse-private-serial",
        };
        let returned = try_call!(Ok::<_, anyhow::Error>(private_value))
            .expect("the successful call must remain successful");

        assert_eq!(returned.backup_bytes, [0, 0xff, 0x52, 0x41, 0x46]);
        assert_eq!(returned.serial_number, "reverse-private-serial");
        assert_eq!(
            returned.custom_setting_name,
            "reverse-private-custom-setting"
        );
        let records = LOGGER
            .records
            .lock()
            .expect("captured log records must remain accessible")
            .clone();
        assert!(
            records
                .iter()
                .any(|record| record.message.ends_with(": succeeded")),
            "successful reverse call did not emit its redacted status: {records:?}"
        );
        let backup_debug = format!("{:?}", returned.backup_bytes);
        assert!(
            records.iter().all(|record| {
                !record.message.contains(&backup_debug)
                    && !record.message.contains(returned.serial_number)
                    && !record.message.contains(returned.custom_setting_name)
            }),
            "successful reverse call logged its returned value: {records:?}"
        );
    }

    #[test]
    fn expected_probe_miss_is_not_logged_as_error() {
        let _capture = start_log_capture();

        let error = try_call!(Err::<(), _>(anyhow!("unsupported property")))
            .expect_err("an expected probe miss must remain an error result");
        assert_eq!(error.to_string(), "unsupported property");

        let records = LOGGER
            .records
            .lock()
            .expect("captured log records must remain accessible")
            .clone();
        assert!(
            records.iter().all(|record| record.level != Level::Error),
            "expected probe miss was logged as an error: {records:?}"
        );
    }

    #[test]
    fn probe_summary_fails_only_when_every_probe_is_unavailable() {
        let _capture = start_log_capture();
        let take_records = || {
            let mut records = LOGGER
                .records
                .lock()
                .expect("captured log records must remain accessible");
            std::mem::take(&mut *records)
        };
        let assert_bounded_summary =
            |records: &[CapturedRecord], expected_counts: [&str; 2], forbidden: [&str; 2]| {
                let [record] = records else {
                    panic!("expected exactly one probe summary, got: {records:?}");
                };
                assert_eq!(record.level, Level::Debug);
                assert!(
                    record.message.len() <= 128,
                    "probe summary is not bounded: {record:?}"
                );
                for expected in expected_counts {
                    assert!(
                        record.message.contains(expected),
                        "probe summary omitted `{expected}`: {record:?}"
                    );
                }
                for sensitive in forbidden {
                    assert!(
                        !record.message.contains(sensitive),
                        "probe summary leaked `{sensitive}`: {record:?}"
                    );
                }
            };

        let mut all_failed = ProbeSummary::default();
        for result in [
            Err::<(), _>(anyhow!("probe-alpha: unsupported property")),
            Err::<(), _>(anyhow!("probe-beta: transport timeout")),
        ] {
            all_failed.observe(&result);
        }
        assert!(
            all_failed.finish("camera info").is_err(),
            "a probe set with no successful calls must fail"
        );
        assert_bounded_summary(
            &take_records(),
            ["0 succeeded", "2 failed"],
            ["probe-alpha", "probe-beta"],
        );

        let mut partially_successful = ProbeSummary::default();
        for result in [
            Err::<(), _>(anyhow!("probe-gamma: unsupported property")),
            Ok(()),
        ] {
            partially_successful.observe(&result);
        }
        assert!(
            partially_successful.finish("camera info").is_ok(),
            "a probe set with any successful call must remain successful"
        );
        assert_bounded_summary(
            &take_records(),
            ["1 succeeded", "1 failed"],
            ["probe-gamma", "unsupported property"],
        );
    }

    #[test]
    fn simulation_probe_logs_distinguish_schema_property_codes() {
        let _capture = start_log_capture();
        let first = Ok::<_, anyhow::Error>("private-payload-alpha");
        let second = Ok::<_, anyhow::Error>("private-payload-beta");

        log_simulation_probe(0xd18d, &first);
        log_simulation_probe(0xd18e, &second);

        let records = LOGGER
            .records
            .lock()
            .expect("captured log records must remain accessible")
            .clone();
        let [first_record, second_record] = records.as_slice() else {
            panic!("expected one record per simulation probe, got: {records:?}");
        };
        assert_ne!(
            first_record.message, second_record.message,
            "distinct simulation properties produced indistinguishable logs"
        );

        for (record, expected_code) in [(first_record, "0xD18D"), (second_record, "0xD18E")] {
            assert_eq!(record.level, Level::Debug);
            assert!(
                record.message.len() <= 128,
                "simulation probe log is not bounded: {record:?}"
            );
            assert!(
                record.message.contains(expected_code),
                "simulation probe log omitted schema code `{expected_code}`: {record:?}"
            );
            assert!(
                !record.message.contains("private-payload-alpha")
                    && !record.message.contains("private-payload-beta"),
                "simulation probe log leaked a result payload: {record:?}"
            );
        }
    }
}
