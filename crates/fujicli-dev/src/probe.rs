//! The gated, state-changing `dangerous-reverse-engineering` probe surface.
//! Every command here is compiled only when that feature is enabled; the
//! read-only `discover` surface in [`crate::reverse`] is unaffected.
//!
//! `simulation-namespace` implements the six-step guard sequence from
//! `docs/contributors/reversing.md`'s "Requirements for Any Future Dangerous
//! Probe", using the sanctioned raw single-property write primitive
//! (`Camera::reverse_probe_write_single_property`, see
//! `docs/contributors/reversing.md`'s "Maintainer decisions (2026-08-30)").
//! This deliberately reopens the raw-mutation surface commit `124aa4f`
//! ("fix: seal raw PTP mutation access") sealed, narrowly and only for this
//! one property round trip.
//!
//! No wire observable is known to distinguish the still and movie
//! custom-setting namespaces (`reversing.md`, open question 1), so the
//! verdict this command reaches is always [`decision::Verdict::Ambiguous`]
//! today. Fabricating a Still/Movie verdict from an unknown signal would
//! violate `AGENTS.md`'s prohibition on inventing camera capabilities, so
//! this module does not do that; corroborate manually via the camera's
//! C1-C7 LCD menu per the maintainer decision.

use std::{
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context as _, ensure};
use clap::{Subcommand, ValueEnum};
use fujicli::{Camera, generated::options::CustomSetting};

use crate::{
    audit::{self, AuditRecord},
    decision::{self, Verdict},
    output::NewOutput,
    usb::{self, Location},
};

/// The PTP property selector this probe experiments on. Sourced from
/// `fml/option.cue`'s `custom_setting` option (`prop_code: 0xD18C`); this is
/// the exact selector `docs/contributors/reversing.md` and `docs/users/`
/// already document as under investigation, not an invented code.
const SIMULATION_NAMESPACE_PROPERTY: u16 = 0xD18C;

/// Fixed acknowledgement string required by
/// `docs/contributors/reversing.md`'s guard sequence, step 5. Naming the
/// exact selector this probe writes.
const REQUIRED_ACKNOWLEDGEMENT: &str = "I-UNDERSTAND-THIS-WRITES-SELECTOR-D18C";

/// Bound applied to camera-reported model/firmware strings before they enter
/// the audit log.
const AUDIT_STRING_BOUND_BYTES: usize = 128;

/// One explicit C1-C7 custom-setting slot. The guard sequence in
/// `docs/contributors/reversing.md` requires selector experiments to touch
/// only one explicit slot per invocation.
#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum CustomSettingSlot {
    C1,
    C2,
    C3,
    C4,
    C5,
    C6,
    C7,
}

impl CustomSettingSlot {
    /// The wire value for this slot, sourced from `fml/option.cue`'s
    /// `custom_setting` option encoding via the generated
    /// `CustomSetting` enum -- not re-derived here.
    const fn wire_value(self) -> u16 {
        let setting = match self {
            Self::C1 => CustomSetting::C1,
            Self::C2 => CustomSetting::C2,
            Self::C3 => CustomSetting::C3,
            Self::C4 => CustomSetting::C4,
            Self::C5 => CustomSetting::C5,
            Self::C6 => CustomSetting::C6,
            Self::C7 => CustomSetting::C7,
        };
        setting as u16
    }
}

#[derive(Debug, Subcommand)]
pub enum ProbeCommand {
    /// Determine whether selector 0xD18C addresses the still or movie
    /// custom-setting namespace.
    SimulationNamespace {
        /// Explicit C1-C7 slot value the probe writes to 0xD18C exactly
        /// once.
        slot: CustomSettingSlot,

        /// New (not-yet-existing) path for the mandatory pre-probe backup.
        backup: NewOutput,

        /// Path to the append-only JSONL audit log (created if absent).
        audit_log: PathBuf,

        /// SHA-256 fingerprint of the connected camera's serial number, as
        /// printed by this command on a prior dry run. Must match the live
        /// camera exactly; a mismatch aborts before any write.
        #[arg(long)]
        confirm_fingerprint: String,

        /// Fixed acknowledgement string proving the operator has read and
        /// accepted the risk. Must equal
        /// `I-UNDERSTAND-THIS-WRITES-SELECTOR-D18C` exactly.
        #[arg(long)]
        acknowledge: String,
    },
}

/// Device round trips the guard sequence needs, factored out so the
/// sequence's ordering can be driven and asserted against a fake in tests
/// without any device I/O.
trait ProbeIo {
    fn read_prop(&mut self, prop: u16) -> anyhow::Result<Vec<u8>>;
    fn write_prop(&mut self, prop: u16, value: &[u8]) -> anyhow::Result<Vec<u8>>;
    fn export_backup(&mut self) -> anyhow::Result<Vec<u8>>;
}

impl ProbeIo for Camera {
    fn read_prop(&mut self, prop: u16) -> anyhow::Result<Vec<u8>> {
        self.reverse_device_property(prop)
    }

    fn write_prop(&mut self, prop: u16, value: &[u8]) -> anyhow::Result<Vec<u8>> {
        self.reverse_probe_write_single_property(prop, value)
    }

    fn export_backup(&mut self) -> anyhow::Result<Vec<u8>> {
        self.reverse_export_backup()
    }
}

/// Context injected by the call site (never generated inside a pure
/// function) for the one audit record this sequence writes.
struct AuditContext {
    timestamp: String,
    tool_version: String,
    invocation_id: String,
    usb_location: String,
    vid_pid: String,
    model: String,
    firmware: String,
}

fn check_acknowledgement(provided: &str) -> anyhow::Result<()> {
    ensure!(
        provided.trim() == REQUIRED_ACKNOWLEDGEMENT,
        "acknowledgement string does not match; aborting before any write"
    );
    Ok(())
}

fn check_fingerprint(live: &str, provided: &str) -> anyhow::Result<()> {
    ensure!(
        provided.trim() == live,
        "confirmed fingerprint does not match the live camera's fingerprint; \
         aborting before any write. Live fingerprint: {live}"
    );
    Ok(())
}

/// The five PTP round trips required by `reversing.md`'s guard sequence,
/// step 5: snapshot, write, read-back, restore, verify. Exactly one probe
/// write and one restore write; never retried.
fn run_write_sequence<IO: ProbeIo>(
    io: &mut IO,
    slot: CustomSettingSlot,
) -> anyhow::Result<Vec<u8>> {
    let snapshot = io
        .read_prop(SIMULATION_NAMESPACE_PROPERTY)
        .context("reading 0xD18C snapshot before the probe write")?;

    let write_value = slot.wire_value().to_le_bytes();
    io.write_prop(SIMULATION_NAMESPACE_PROPERTY, &write_value)
        .inspect_err(|_| {
            eprintln!("DO NOT RETRY AUTOMATICALLY: probe write failed, camera state is unknown");
        })?;

    let observed = io
        .read_prop(SIMULATION_NAMESPACE_PROPERTY)
        .inspect_err(|_| {
            eprintln!(
                "DO NOT RETRY AUTOMATICALLY: post-write read-back failed, camera state is unknown"
            );
        })?;

    io.write_prop(SIMULATION_NAMESPACE_PROPERTY, &snapshot)
        .inspect_err(|_| {
            eprintln!("DO NOT RETRY AUTOMATICALLY: restore write failed, camera state is unknown");
        })?;

    let verified = io
        .read_prop(SIMULATION_NAMESPACE_PROPERTY)
        .inspect_err(|_| {
            eprintln!(
                "DO NOT RETRY AUTOMATICALLY: restore verification read failed, camera state is unknown"
            );
        })?;

    if verified != snapshot {
        eprintln!(
            "DO NOT RETRY AUTOMATICALLY: restore verification mismatch, camera state may differ \
             from the pre-probe snapshot"
        );
        anyhow::bail!(
            "restore verification failed: read-back does not match the pre-probe snapshot"
        );
    }

    Ok(observed)
}

/// Runs the full guard sequence (gates, pre-backup, audit record, write
/// sequence, verdict) against any [`ProbeIo`]. Kept generic so tests drive
/// it with a fake and assert exact call ordering without device I/O.
#[allow(
    clippy::too_many_arguments,
    reason = "each argument is independently required by a distinct guard step; bundling them would obscure which step needs what"
)]
fn run_guarded_sequence<IO: ProbeIo>(
    io: &mut IO,
    slot: CustomSettingSlot,
    confirm_fingerprint: &str,
    acknowledge: &str,
    live_fingerprint: &str,
    backup: &NewOutput,
    audit_log: &Path,
    context: AuditContext,
) -> anyhow::Result<Verdict> {
    check_acknowledgement(acknowledge)?;
    check_fingerprint(live_fingerprint, confirm_fingerprint)?;

    let backup_bytes = io
        .export_backup()
        .context("exporting mandatory pre-probe backup")?;
    backup.write_all(&backup_bytes)?;
    let backup_digest = fujicli::features::backup::sha256_hex(&backup_bytes);

    let record = AuditRecord {
        timestamp: context.timestamp,
        tool_version: context.tool_version,
        invocation_id: context.invocation_id,
        operation: "probe_simulation_namespace".to_owned(),
        risk_class: "state_changing".to_owned(),
        ptp_operation_codes: vec!["0x1015".to_owned(), "0x1016".to_owned()],
        usb_location: context.usb_location,
        vid_pid: context.vid_pid,
        model: audit::bound(&context.model, AUDIT_STRING_BOUND_BYTES),
        firmware: audit::bound(&context.firmware, AUDIT_STRING_BOUND_BYTES),
        serial_fingerprint: live_fingerprint.to_owned(),
        pre_backup_sha256: backup_digest,
        outcome: "attempted".to_owned(),
    };
    audit::append(audit_log, &record).context("durably recording the probe attempt")?;

    let _observed = run_write_sequence(io, slot)?;

    // No wire observable is known to distinguish still vs movie; see the
    // module docs. Do not fabricate one.
    let verdict = decision::decide(None);
    if verdict == Verdict::Ambiguous {
        eprintln!("DO NOT RETRY AUTOMATICALLY");
        eprintln!(
            "Verdict: ambiguous -- no known wire observable distinguishes the still and movie \
             custom-setting namespaces. Corroborate manually via the camera's C1-C7 LCD menu \
             per the maintainer decision recorded in docs/contributors/reversing.md."
        );
    }
    Ok(verdict)
}

fn invocation_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    format!("{}-{nanos}", std::process::id())
}

fn timestamp() -> String {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or_else(
        |_| "0".to_owned(),
        |duration| duration.as_secs().to_string(),
    )
}

fn simulation_namespace(
    slot: CustomSettingSlot,
    backup: &NewOutput,
    audit_log: &Path,
    confirm_fingerprint: &str,
    acknowledge: &str,
    location: Location,
) -> anyhow::Result<()> {
    let device = usb::exact_device(location)?;
    let descriptor = device.device_descriptor()?;
    let vid_pid = format!(
        "{:04x}:{:04x}",
        descriptor.vendor_id(),
        descriptor.product_id()
    );

    eprintln!("WARNING: fujicli-dev is an unpublished reverse-engineering tool");
    eprintln!(
        "WARNING: this probe issues ONE raw SetDevicePropValue(0x{SIMULATION_NAMESPACE_PROPERTY:04x}) \
         without permit authorization; see docs/contributors/reversing.md"
    );
    eprintln!("Target: USB {location}, VID:PID {vid_pid}");

    let mut camera = Camera::open_unknown(&device)?;
    let identity = camera.reverse_probe_device_identity()?;
    let live_fingerprint = fujicli::features::backup::sha256_hex(identity.serial_number.as_bytes());

    eprintln!("Manufacturer: {}", identity.manufacturer);
    eprintln!("Model: {}", identity.model);
    eprintln!("Firmware: {}", identity.firmware);
    eprintln!("Serial fingerprint (SHA-256): {live_fingerprint}");

    let context = AuditContext {
        timestamp: timestamp(),
        tool_version: env!("CARGO_PKG_VERSION").to_owned(),
        invocation_id: invocation_id(),
        usb_location: location.to_string(),
        vid_pid,
        model: identity.model,
        firmware: identity.firmware,
    };

    let verdict = run_guarded_sequence(
        &mut camera,
        slot,
        confirm_fingerprint,
        acknowledge,
        &live_fingerprint,
        backup,
        audit_log,
        context,
    )?;

    eprintln!("Verdict: {verdict:?}");
    Ok(())
}

pub fn handle(command: &ProbeCommand, location: Location) -> anyhow::Result<()> {
    match command {
        ProbeCommand::SimulationNamespace {
            slot,
            backup,
            audit_log,
            confirm_fingerprint,
            acknowledge,
        } => simulation_namespace(
            *slot,
            backup,
            audit_log,
            confirm_fingerprint,
            acknowledge,
            location,
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, str::FromStr as _};

    use clap::Parser as _;

    use super::{
        AuditContext, CustomSettingSlot, ProbeIo, REQUIRED_ACKNOWLEDGEMENT,
        SIMULATION_NAMESPACE_PROPERTY, check_acknowledgement, check_fingerprint,
        run_guarded_sequence, run_write_sequence,
    };
    use crate::{decision::Verdict, output::NewOutput};

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Call {
        Read(u16),
        Write(u16, Vec<u8>),
        ExportBackup,
    }

    struct FakeProbeIo {
        calls: RefCell<Vec<Call>>,
        prop_value: RefCell<Vec<u8>>,
        backup_bytes: Vec<u8>,
        fail_export_backup: bool,
    }

    impl FakeProbeIo {
        fn new(initial_prop_value: Vec<u8>) -> Self {
            Self {
                calls: RefCell::new(Vec::new()),
                prop_value: RefCell::new(initial_prop_value),
                backup_bytes: b"backup-bytes".to_vec(),
                fail_export_backup: false,
            }
        }

        fn calls(&self) -> Vec<Call> {
            self.calls.borrow().clone()
        }

        fn write_call_count(&self) -> usize {
            self.calls
                .borrow()
                .iter()
                .filter(|call| matches!(call, Call::Write(..)))
                .count()
        }
    }

    impl ProbeIo for FakeProbeIo {
        fn read_prop(&mut self, prop: u16) -> anyhow::Result<Vec<u8>> {
            self.calls.borrow_mut().push(Call::Read(prop));
            Ok(self.prop_value.borrow().clone())
        }

        fn write_prop(&mut self, prop: u16, value: &[u8]) -> anyhow::Result<Vec<u8>> {
            self.calls
                .borrow_mut()
                .push(Call::Write(prop, value.to_vec()));
            *self.prop_value.borrow_mut() = value.to_vec();
            Ok(value.to_vec())
        }

        fn export_backup(&mut self) -> anyhow::Result<Vec<u8>> {
            self.calls.borrow_mut().push(Call::ExportBackup);
            if self.fail_export_backup {
                anyhow::bail!("simulated backup export failure");
            }
            Ok(self.backup_bytes.clone())
        }
    }

    fn sample_context() -> AuditContext {
        AuditContext {
            timestamp: "1735603200".to_owned(),
            tool_version: "0.2.0".to_owned(),
            invocation_id: "test-invocation".to_owned(),
            usb_location: "1.2".to_owned(),
            vid_pid: "04cb:02cb".to_owned(),
            model: "X-T5".to_owned(),
            firmware: "4.31".to_owned(),
        }
    }

    #[derive(Debug, clap::Parser)]
    struct TestCli {
        #[command(subcommand)]
        command: super::ProbeCommand,
    }

    #[test]
    fn simulation_namespace_requires_an_explicit_slot() {
        let error = TestCli::try_parse_from(["probe", "simulation-namespace"])
            .expect_err("the slot argument must be required, not defaulted");

        assert_eq!(
            error.kind(),
            clap::error::ErrorKind::MissingRequiredArgument
        );
    }

    #[test]
    fn wrong_acknowledgement_is_rejected() {
        let error = check_acknowledgement("not the right string")
            .expect_err("a wrong acknowledgement string must be rejected");
        assert!(error.to_string().contains("acknowledgement"));
    }

    #[test]
    fn exact_acknowledgement_is_accepted() {
        check_acknowledgement(REQUIRED_ACKNOWLEDGEMENT)
            .expect("the exact required acknowledgement string must be accepted");
    }

    #[test]
    fn wrong_fingerprint_is_rejected() {
        let error = check_fingerprint(&"a".repeat(64), &"b".repeat(64))
            .expect_err("a mismatched fingerprint must be rejected");
        assert!(error.to_string().contains("fingerprint"));
    }

    #[test]
    fn matching_fingerprint_is_accepted() {
        let fingerprint = "a".repeat(64);
        check_fingerprint(&fingerprint, &fingerprint)
            .expect("a matching fingerprint must be accepted");
    }

    #[test]
    fn gate_refusal_wrong_fingerprint_records_zero_writes() {
        let mut io = FakeProbeIo::new(vec![0]);
        let tempdir = tempfile::tempdir().expect("tempdir must be created");
        let backup = NewOutput::from_str(tempdir.path().join("backup.fbk").to_str().unwrap())
            .expect("backup path must parse");
        let audit_log = tempdir.path().join("audit.jsonl");

        let error = run_guarded_sequence(
            &mut io,
            CustomSettingSlot::C1,
            "wrong-fingerprint",
            REQUIRED_ACKNOWLEDGEMENT,
            &"a".repeat(64),
            &backup,
            &audit_log,
            sample_context(),
        )
        .expect_err("a fingerprint mismatch must abort before any write");

        assert!(error.to_string().contains("fingerprint"));
        assert_eq!(io.write_call_count(), 0);
        assert!(io.calls().is_empty());
    }

    #[test]
    fn gate_refusal_wrong_acknowledgement_records_zero_writes() {
        let mut io = FakeProbeIo::new(vec![0]);
        let tempdir = tempfile::tempdir().expect("tempdir must be created");
        let backup = NewOutput::from_str(tempdir.path().join("backup.fbk").to_str().unwrap())
            .expect("backup path must parse");
        let audit_log = tempdir.path().join("audit.jsonl");

        let error = run_guarded_sequence(
            &mut io,
            CustomSettingSlot::C1,
            &"a".repeat(64),
            "not-the-acknowledgement",
            &"a".repeat(64),
            &backup,
            &audit_log,
            sample_context(),
        )
        .expect_err("a wrong acknowledgement must abort before any write");

        assert!(error.to_string().contains("acknowledgement"));
        assert_eq!(io.write_call_count(), 0);
        assert!(io.calls().is_empty());
    }

    #[test]
    fn write_sequence_orders_reads_and_writes_and_restores_the_snapshot() {
        let mut io = FakeProbeIo::new(vec![0xAA]);

        let observed =
            run_write_sequence(&mut io, CustomSettingSlot::C1).expect("sequence must succeed");

        let expected_write_value = CustomSettingSlot::C1.wire_value().to_le_bytes().to_vec();
        assert_eq!(observed, expected_write_value);

        let calls = io.calls();
        assert_eq!(
            calls,
            vec![
                Call::Read(SIMULATION_NAMESPACE_PROPERTY),
                Call::Write(SIMULATION_NAMESPACE_PROPERTY, expected_write_value),
                Call::Read(SIMULATION_NAMESPACE_PROPERTY),
                Call::Write(SIMULATION_NAMESPACE_PROPERTY, vec![0xAA]),
                Call::Read(SIMULATION_NAMESPACE_PROPERTY),
            ]
        );
        assert_eq!(io.write_call_count(), 2);
        // The restore write must carry exactly the pre-probe snapshot value.
        assert_eq!(
            calls[3],
            Call::Write(SIMULATION_NAMESPACE_PROPERTY, vec![0xAA])
        );
        // Final state must equal the snapshot after restore.
        assert_eq!(*io.prop_value.borrow(), vec![0xAA]);
    }

    #[test]
    fn full_sequence_exports_backup_writes_audit_record_and_never_retries() {
        let mut io = FakeProbeIo::new(vec![0x01]);
        let tempdir = tempfile::tempdir().expect("tempdir must be created");
        let backup_path = tempdir.path().join("backup.fbk");
        let backup =
            NewOutput::from_str(backup_path.to_str().unwrap()).expect("backup path must parse");
        let audit_log = tempdir.path().join("audit.jsonl");
        let fingerprint = "a".repeat(64);

        let verdict = run_guarded_sequence(
            &mut io,
            CustomSettingSlot::C2,
            &fingerprint,
            REQUIRED_ACKNOWLEDGEMENT,
            &fingerprint,
            &backup,
            &audit_log,
            sample_context(),
        )
        .expect("a fully gated sequence must succeed against the fake");

        assert_eq!(verdict, Verdict::Ambiguous);
        assert_eq!(io.write_call_count(), 2);
        assert!(backup_path.exists(), "the pre-backup must be written");
        assert!(audit_log.exists(), "the audit log must be written");

        let audit_contents =
            std::fs::read_to_string(&audit_log).expect("audit log must be readable");
        assert_eq!(
            audit_contents.lines().count(),
            1,
            "exactly one audit line must be appended per invocation"
        );
        assert!(!audit_contents.contains("backup-bytes"));
    }

    #[test]
    fn backup_export_failure_aborts_before_any_write() {
        let mut io = FakeProbeIo::new(vec![0x01]);
        io.fail_export_backup = true;
        let tempdir = tempfile::tempdir().expect("tempdir must be created");
        let backup = NewOutput::from_str(tempdir.path().join("backup.fbk").to_str().unwrap())
            .expect("backup path must parse");
        let audit_log = tempdir.path().join("audit.jsonl");
        let fingerprint = "a".repeat(64);

        let error = run_guarded_sequence(
            &mut io,
            CustomSettingSlot::C1,
            &fingerprint,
            REQUIRED_ACKNOWLEDGEMENT,
            &fingerprint,
            &backup,
            &audit_log,
            sample_context(),
        )
        .expect_err("a failed pre-backup export must abort the sequence");

        assert!(error.to_string().contains("backup"));
        assert_eq!(io.write_call_count(), 0);
    }
}
