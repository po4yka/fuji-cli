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
//! The candidate observable for telling the still and movie custom-setting
//! namespaces apart is `0xD18D` (`custom_setting_name`), identified in the
//! 2026-09-04 macOS session and not yet confirmed on a camera: give the probed
//! C1-C7 slot distinguishable names in each namespace on the camera body ahead
//! of time, pass them via `--still-slot-name`/`--movie-slot-name`, and this
//! command reads `0xD18D` back after the `0xD18C` write and compares it
//! against those two declared names (`reversing.md`'s "macOS findings
//! (2026-09-04)"). Omitting both flags, or a read/decode failure on that one
//! extra readback, always resolves to [`decision::Verdict::Ambiguous`] -- this
//! module never fabricates a Still/Movie verdict from an unread or undeclared
//! signal, per `AGENTS.md`'s prohibition on inventing camera capabilities.
//! This has been implemented and exercised only against fakes in this crate's
//! tests; it has not yet been run against a physical camera. A resolved
//! verdict is still a single-run observation -- corroborate it manually via
//! the camera's C1-C7 LCD menu per the maintainer decision.

use std::{
    path::{Path, PathBuf},
    str::FromStr as _,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context as _, ensure};
use clap::{Subcommand, ValueEnum};
use fujicli::{
    Camera,
    generated::options::{CustomSetting, CustomSettingName, prop_codes},
};

use crate::{
    audit::{self, AuditRecord},
    decision::{self, NamespaceSignal, Verdict},
    interrupt::{self, CriticalWriteError},
    output::NewOutput,
    reverse::decode_ptp_string,
    usb::{self, Location},
};

/// The PTP property selector this probe experiments on. Taken from
/// `fml/option.cue`'s `custom_setting` option, so it is the exact selector
/// `docs/contributors/reversing.md` and `docs/users/` already document as
/// under investigation, and cannot drift from the schema.
const SIMULATION_NAMESPACE_PROPERTY: u16 = fujicli::generated::options::prop_codes::CUSTOM_SETTING;

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

        /// Path to the append-only JSONL audit log. Symlinks are rejected;
        /// on Unix, an existing file must grant no group/other access.
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

        /// Operator-declared name of the probed slot in the still
        /// custom-setting namespace, exactly as it appears on the camera
        /// body (set it there before running this command). Given together
        /// with `--movie-slot-name`, it lets the probe read `0xD18D` back
        /// after the `0xD18C` write and resolve a Still/Movie verdict
        /// instead of Ambiguous. Omit both flags to skip that readback.
        #[arg(long, requires = "movie_slot_name")]
        still_slot_name: Option<String>,

        /// Operator-declared name of the probed slot in the movie
        /// custom-setting namespace, exactly as it appears on the camera
        /// body. Required together with `--still-slot-name`; see that
        /// flag's help.
        #[arg(long, requires = "still_slot_name")]
        movie_slot_name: Option<String>,
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

/// Operator-declared slot names for both custom-setting namespaces, parsed
/// and validated up front. Supplying this turns the `0xD18D` readback in
/// [`run_mutating_write_sequence`] into a real Still/Movie signal; its
/// absence keeps the verdict `Ambiguous` exactly as before this readback
/// existed.
struct SlotNames {
    still: String,
    movie: String,
}

impl SlotNames {
    /// Validates both raw operator-supplied names as
    /// `CustomSettingName` (reusing its `FromStr`, so the 25-character rule
    /// is not duplicated here) and rejects equal names, which cannot
    /// discriminate the two namespaces.
    fn parse(still: &str, movie: &str) -> anyhow::Result<Self> {
        let still_name = CustomSettingName::from_str(still)
            .context("--still-slot-name is not a valid custom-setting name")?;
        let movie_name = CustomSettingName::from_str(movie)
            .context("--movie-slot-name is not a valid custom-setting name")?;
        ensure!(
            still_name.as_str() != movie_name.as_str(),
            "--still-slot-name and --movie-slot-name must differ; equal names cannot \
             discriminate the still and movie custom-setting namespaces"
        );
        Ok(Self {
            still: still_name.as_str().to_owned(),
            movie: movie_name.as_str().to_owned(),
        })
    }
}

/// Validates the raw `--still-slot-name`/`--movie-slot-name` pair. Clap's
/// `requires` already guarantees both or neither reach here from the CLI;
/// this function still rejects a lone one defensively for direct callers
/// (tests, library use) rather than panicking on a caller mistake.
fn parse_slot_names(still: Option<&str>, movie: Option<&str>) -> anyhow::Result<Option<SlotNames>> {
    match (still, movie) {
        (None, None) => Ok(None),
        (Some(still), Some(movie)) => SlotNames::parse(still, movie).map(Some),
        (Some(_), None) | (None, Some(_)) => Err(anyhow::anyhow!(
            "--still-slot-name and --movie-slot-name must be given together"
        )),
    }
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

/// Which stage of [`run_write_sequence`] failed, if any. Drives the terminal
/// audit outcome classification in [`run_guarded_sequence`] from the
/// sequence's own control flow -- never from string-matching an `anyhow`
/// message. Each variant maps to exactly one outcome string in the audit-log
/// contract (`docs/contributors/reversing.md`); the vocabulary may grow but
/// existing values must never be renamed or removed once written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WriteSequenceStage {
    /// The pre-write snapshot read failed; no mutating write was attempted.
    Snapshot,
    /// The mutating probe write itself returned an error; camera state is
    /// most likely untouched, but not guaranteed.
    Write,
    /// The write succeeded but the post-write read-back failed.
    Readback,
    /// The restore write returned an error; the camera is left mutated.
    Restore,
    /// The restore write was sent but the verification read failed.
    RestoreVerifyRead,
    /// The restore was sent and the verification read succeeded, but its
    /// value did not match the pre-probe snapshot; camera state is
    /// uncertain.
    RestoreVerifyMismatch,
    /// Ctrl-C was requested after the probe write, but the original snapshot
    /// was restored and verified before the sequence stopped.
    InterruptedAfterRestore,
}

impl WriteSequenceStage {
    /// The stable, lowercase outcome string this stage contributes to the
    /// terminal audit record's `outcome` field.
    const fn outcome(self) -> &'static str {
        match self {
            Self::Snapshot => "snapshot_failed",
            Self::Write => "write_failed",
            Self::Readback => "readback_failed",
            Self::Restore => "restore_failed",
            Self::RestoreVerifyRead => "restore_verify_read_failed",
            Self::RestoreVerifyMismatch => "restore_verify_mismatch",
            Self::InterruptedAfterRestore => "interrupted_after_restore",
        }
    }
}

/// A failure from [`run_write_sequence`], carrying both the stage that
/// failed (for terminal audit classification) and the original error (for
/// the operator-facing message). The stage is never derived from `source`'s
/// text.
#[derive(Debug)]
struct WriteSequenceFailure {
    stage: WriteSequenceStage,
    source: anyhow::Error,
}

/// The five PTP round trips required by `reversing.md`'s guard sequence,
/// step 5: snapshot, write, read-back, restore, verify. Exactly one probe
/// write and one restore write; never retried. `slot_names`, when present,
/// adds one extra read (`0xD18D`) between the read-back and the restore
/// write to compute the [`NamespaceSignal`]; a failure on that extra read
/// never aborts the sequence or skips the restore.
fn run_write_sequence<IO: ProbeIo>(
    io: &mut IO,
    slot: CustomSettingSlot,
    slot_names: Option<&SlotNames>,
) -> Result<(Vec<u8>, Option<NamespaceSignal>), WriteSequenceFailure> {
    let snapshot = io
        .read_prop(SIMULATION_NAMESPACE_PROPERTY)
        .context("reading 0xD18C snapshot before the probe write")
        .map_err(|source| WriteSequenceFailure {
            stage: WriteSequenceStage::Snapshot,
            source,
        })?;

    match interrupt::critical_camera_write(|| {
        run_mutating_write_sequence(io, slot, &snapshot, slot_names)
    }) {
        Ok(result) => Ok(result),
        Err(CriticalWriteError::Operation(failure)) => Err(failure),
        Err(CriticalWriteError::Interrupted) => Err(WriteSequenceFailure {
            stage: WriteSequenceStage::InterruptedAfterRestore,
            source: anyhow::anyhow!(
                "interrupt requested during the probe write; the original selector was restored and verified"
            ),
        }),
    }
}

/// Reads `0xD18D` after the probe write and classifies it against the
/// operator-declared slot names. Never fails the caller: a read or decode
/// failure logs one stderr line and returns `None`, which resolves to
/// `Verdict::Ambiguous` -- it must never abort the sequence or skip the
/// restore write that follows it. Never prints the observed bytes, the
/// decoded text, or either declared name.
fn observe_namespace_signal<IO: ProbeIo>(
    io: &mut IO,
    names: &SlotNames,
) -> Option<NamespaceSignal> {
    let bytes = io
        .read_prop(prop_codes::CUSTOM_SETTING_NAME)
        .inspect_err(|_| {
            eprintln!("0xD18D read failed after the probe write; verdict will be ambiguous");
        })
        .ok()?;
    let Some(observed) = decode_ptp_string(&bytes) else {
        eprintln!("0xD18D payload did not decode as a PTP string; verdict will be ambiguous");
        return None;
    };
    Some(NamespaceSignal::from_slot_name(
        &observed,
        &names.still,
        &names.movie,
    ))
}

fn run_mutating_write_sequence<IO: ProbeIo>(
    io: &mut IO,
    slot: CustomSettingSlot,
    snapshot: &[u8],
    slot_names: Option<&SlotNames>,
) -> Result<(Vec<u8>, Option<NamespaceSignal>), WriteSequenceFailure> {
    let write_value = slot.wire_value().to_le_bytes();
    io.write_prop(SIMULATION_NAMESPACE_PROPERTY, &write_value)
        .inspect_err(|_| {
            eprintln!("DO NOT RETRY AUTOMATICALLY: probe write failed, camera state is unknown");
        })
        .map_err(|source| WriteSequenceFailure {
            stage: WriteSequenceStage::Write,
            source,
        })?;

    let observed = io
        .read_prop(SIMULATION_NAMESPACE_PROPERTY)
        .inspect_err(|_| {
            eprintln!(
                "DO NOT RETRY AUTOMATICALLY: post-write read-back failed, camera state is unknown"
            );
        })
        .map_err(|source| WriteSequenceFailure {
            stage: WriteSequenceStage::Readback,
            source,
        })?;

    let signal = slot_names.and_then(|names| observe_namespace_signal(io, names));

    io.write_prop(SIMULATION_NAMESPACE_PROPERTY, snapshot)
        .inspect_err(|_| {
            eprintln!("DO NOT RETRY AUTOMATICALLY: restore write failed, camera state is unknown");
        })
        .map_err(|source| WriteSequenceFailure {
            stage: WriteSequenceStage::Restore,
            source,
        })?;

    let verified = io
        .read_prop(SIMULATION_NAMESPACE_PROPERTY)
        .inspect_err(|_| {
            eprintln!(
                "DO NOT RETRY AUTOMATICALLY: restore verification read failed, camera state is unknown"
            );
        })
        .map_err(|source| WriteSequenceFailure {
            stage: WriteSequenceStage::RestoreVerifyRead,
            source,
        })?;

    if verified != snapshot {
        eprintln!(
            "DO NOT RETRY AUTOMATICALLY: restore verification mismatch, camera state may differ \
             from the pre-probe snapshot"
        );
        return Err(WriteSequenceFailure {
            stage: WriteSequenceStage::RestoreVerifyMismatch,
            source: anyhow::anyhow!(
                "restore verification failed: read-back does not match the pre-probe snapshot"
            ),
        });
    }

    Ok((observed, signal))
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
    still_slot_name: Option<&str>,
    movie_slot_name: Option<&str>,
    backup: &NewOutput,
    audit_log: &Path,
    context: AuditContext,
) -> anyhow::Result<Verdict> {
    check_acknowledgement(acknowledge)?;
    check_fingerprint(live_fingerprint, confirm_fingerprint)?;
    let slot_names = parse_slot_names(still_slot_name, movie_slot_name)?;

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

    let write_result = run_write_sequence(io, slot, slot_names.as_ref());

    // The terminal record reuses every field of the pre-write `attempted`
    // record -- same allowlist, same invocation_id -- with only `outcome`
    // replaced, so the two lines correlate as one attempt.
    let outcome = match &write_result {
        Ok(_) => "restored",
        Err(failure) => failure.stage.outcome(),
    };
    let mut terminal_record = record;
    outcome.clone_into(&mut terminal_record.outcome);
    let terminal_append = audit::append(audit_log, &terminal_record)
        .context("durably recording the probe terminal outcome");

    let (_observed, signal) = match write_result {
        Ok(result) => {
            // No probe error to protect here: a failure to durably record a
            // successful sequence's outcome is itself reported.
            terminal_append?;
            result
        }
        Err(failure) => {
            // The original probe error is what the operator sees; a
            // terminal-audit-write failure is surfaced as a warning, never
            // swallowed, but must not mask or replace `failure.source`.
            if let Err(audit_error) = terminal_append {
                eprintln!(
                    "WARNING: failed to durably record the probe terminal outcome: {audit_error:#}"
                );
            }
            return Err(failure.source);
        }
    };

    let verdict = decision::decide(signal);
    // Never print the observed 0xD18D bytes, the decoded text, or either
    // declared slot name here -- only the verdict and static guidance.
    match verdict {
        Verdict::Still => eprintln!(
            "0xD18D read back the operator-declared still slot name after the 0xD18C write. \
             This is a single-run observation; corroborate manually via the camera's C1-C7 \
             LCD menu per the maintainer decision recorded in docs/contributors/reversing.md."
        ),
        Verdict::Movie => eprintln!(
            "0xD18D read back the operator-declared movie slot name after the 0xD18C write. \
             This is a single-run observation; corroborate manually via the camera's C1-C7 \
             LCD menu per the maintainer decision recorded in docs/contributors/reversing.md."
        ),
        Verdict::Ambiguous => {
            eprintln!("DO NOT RETRY AUTOMATICALLY");
            eprintln!(
                "Verdict: ambiguous -- either no operator-declared slot names were given, or \
                 the 0xD18D readback did not resolve to exactly one of them. Corroborate \
                 manually via the camera's C1-C7 LCD menu per the maintainer decision recorded \
                 in docs/contributors/reversing.md."
            );
        }
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

#[allow(
    clippy::too_many_arguments,
    reason = "each argument is independently required by a distinct guard step or CLI flag; bundling them would obscure which step needs what"
)]
fn simulation_namespace(
    slot: CustomSettingSlot,
    backup: &NewOutput,
    audit_log: &Path,
    confirm_fingerprint: &str,
    acknowledge: &str,
    still_slot_name: Option<&str>,
    movie_slot_name: Option<&str>,
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

    eprintln!("Manufacturer: {}", identity.manufacturer.escape_debug());
    eprintln!("Model: {}", identity.model.escape_debug());
    eprintln!("Firmware: {}", identity.firmware.escape_debug());
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
        still_slot_name,
        movie_slot_name,
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
            still_slot_name,
            movie_slot_name,
        } => simulation_namespace(
            *slot,
            backup,
            audit_log,
            confirm_fingerprint,
            acknowledge,
            still_slot_name.as_deref(),
            movie_slot_name.as_deref(),
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
    use std::sync::{Mutex, MutexGuard, PoisonError};

    use crate::{
        decision::{NamespaceSignal, Verdict},
        output::NewOutput,
    };
    use fujicli::generated::options::prop_codes::CUSTOM_SETTING_NAME;

    /// The dangerous-probe interrupt latch is process-global, because a
    /// signal handler is: `critical_camera_write` clears the pending count on
    /// entry and drains it on exit. Cargo runs these tests as threads of one
    /// process, so two overlapping sequences steal each other's interrupt.
    /// Every test that runs a sequence takes this lock first.
    static SEQUENCE: Mutex<()> = Mutex::new(());

    fn sequence_lock() -> MutexGuard<'static, ()> {
        // A panicking test must fail on its own, not cascade into the rest.
        SEQUENCE.lock().unwrap_or_else(PoisonError::into_inner)
    }

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
        /// If set, the write call at this zero-based index among write
        /// calls (0 = the probe write, 1 = the restore write) fails.
        fail_write_at_call: Option<usize>,
        /// If set, simulate Ctrl-C after this successful zero-based write.
        interrupt_after_write_at_call: Option<usize>,
        /// The raw payload served for a `0xD18D` read. `None` fails that one
        /// read (every other read still succeeds); `Some` succeeds with the
        /// given bytes.
        d18d_payload: Option<Vec<u8>>,
    }

    impl FakeProbeIo {
        fn new(initial_prop_value: Vec<u8>) -> Self {
            Self {
                calls: RefCell::new(Vec::new()),
                prop_value: RefCell::new(initial_prop_value),
                backup_bytes: b"backup-bytes".to_vec(),
                fail_export_backup: false,
                fail_write_at_call: None,
                interrupt_after_write_at_call: None,
                d18d_payload: None,
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

    /// Encodes `text` the same way the real PTP string wire encoding does:
    /// one length byte (UTF-16 code units including the terminator), the
    /// UTF-16LE code units, then a NUL terminator -- the shape
    /// `decode_ptp_string` expects.
    fn ptp_string_payload(text: &str) -> Vec<u8> {
        let units: Vec<u16> = text.encode_utf16().collect();
        let mut bytes = Vec::with_capacity(1 + (units.len() + 1) * 2);
        bytes.push(u8::try_from(units.len() + 1).expect("test payload count fits in a u8"));
        for unit in &units {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes
    }

    impl ProbeIo for FakeProbeIo {
        fn read_prop(&mut self, prop: u16) -> anyhow::Result<Vec<u8>> {
            self.calls.borrow_mut().push(Call::Read(prop));
            if prop == CUSTOM_SETTING_NAME {
                return self
                    .d18d_payload
                    .clone()
                    .ok_or_else(|| anyhow::anyhow!("simulated 0xD18D read failure"));
            }
            Ok(self.prop_value.borrow().clone())
        }

        fn write_prop(&mut self, prop: u16, value: &[u8]) -> anyhow::Result<Vec<u8>> {
            let write_call_index = self.write_call_count();
            self.calls
                .borrow_mut()
                .push(Call::Write(prop, value.to_vec()));
            if self.fail_write_at_call == Some(write_call_index) {
                anyhow::bail!("simulated write failure");
            }
            *self.prop_value.borrow_mut() = value.to_vec();
            if self.interrupt_after_write_at_call == Some(write_call_index) {
                crate::interrupt::simulate_interrupt();
            }
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
        let _sequence = sequence_lock();
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
            None,
            None,
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
        let _sequence = sequence_lock();
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
            None,
            None,
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
        let _sequence = sequence_lock();
        let mut io = FakeProbeIo::new(vec![0xAA]);

        let (observed, signal) = run_write_sequence(&mut io, CustomSettingSlot::C1, None)
            .expect("sequence must succeed");

        let expected_write_value = CustomSettingSlot::C1.wire_value().to_le_bytes().to_vec();
        assert_eq!(observed, expected_write_value);
        assert_eq!(
            signal, None,
            "no slot names were given, so no namespace signal must be computed"
        );

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
    fn interrupt_after_probe_write_restores_snapshot_before_stopping() {
        let _sequence = sequence_lock();
        let mut io = FakeProbeIo::new(vec![0xAA]);
        io.interrupt_after_write_at_call = Some(0);
        let tempdir = tempfile::tempdir().expect("tempdir must be created");
        let backup = NewOutput::from_str(
            tempdir
                .path()
                .join("backup.fbk")
                .to_str()
                .expect("path must be valid UTF-8"),
        )
        .expect("backup path must parse");
        let audit_log = tempdir.path().join("audit.jsonl");
        let fingerprint = "a".repeat(64);

        let error = run_guarded_sequence(
            &mut io,
            CustomSettingSlot::C1,
            &fingerprint,
            REQUIRED_ACKNOWLEDGEMENT,
            &fingerprint,
            None,
            None,
            &backup,
            &audit_log,
            sample_context(),
        )
        .expect_err("Ctrl-C after the probe write must stop only after restore verification");

        assert!(error.to_string().contains("interrupt requested"));
        assert_eq!(*io.prop_value.borrow(), vec![0xAA]);
        assert_eq!(io.write_call_count(), 2);

        let lines = read_audit_lines(&audit_log);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0]["outcome"], "attempted");
        assert_eq!(lines[1]["outcome"], "interrupted_after_restore");
    }

    #[test]
    fn full_sequence_exports_backup_writes_audit_record_and_never_retries() {
        let _sequence = sequence_lock();
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
            None,
            None,
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
            2,
            "one pre-write attempted line and one terminal outcome line must be appended"
        );
        assert!(!audit_contents.contains("backup-bytes"));
    }

    /// Parses each line of `path` as a JSON object, asserting it parses.
    fn read_audit_lines(path: &std::path::Path) -> Vec<serde_json::Value> {
        std::fs::read_to_string(path)
            .expect("audit log must be readable")
            .lines()
            .map(|line| serde_json::from_str(line).expect("each audit line must be valid JSON"))
            .collect()
    }

    #[test]
    fn success_path_appends_attempted_then_restored_sharing_one_invocation_id() {
        let _sequence = sequence_lock();
        let mut io = FakeProbeIo::new(vec![0x01]);
        let tempdir = tempfile::tempdir().expect("tempdir must be created");
        let backup = NewOutput::from_str(
            tempdir
                .path()
                .join("backup.fbk")
                .to_str()
                .expect("path must be valid UTF-8"),
        )
        .expect("backup path must parse");
        let audit_log = tempdir.path().join("audit.jsonl");
        let fingerprint = "a".repeat(64);

        run_guarded_sequence(
            &mut io,
            CustomSettingSlot::C3,
            &fingerprint,
            REQUIRED_ACKNOWLEDGEMENT,
            &fingerprint,
            None,
            None,
            &backup,
            &audit_log,
            sample_context(),
        )
        .expect("a fully gated successful sequence must produce a verdict");

        let lines = read_audit_lines(&audit_log);
        assert_eq!(lines.len(), 2, "exactly two audit lines per attempt");
        assert_eq!(lines[0]["outcome"], "attempted");
        assert_eq!(lines[1]["outcome"], "restored");
        assert_eq!(
            lines[0]["invocation_id"], lines[1]["invocation_id"],
            "both lines of one attempt must share one invocation_id"
        );
    }

    #[test]
    fn restore_failure_still_appends_a_terminal_record_and_returns_the_original_error() {
        let _sequence = sequence_lock();
        let mut io = FakeProbeIo::new(vec![0x01]);
        // Fail the second write call: the restore write, not the probe write.
        io.fail_write_at_call = Some(1);
        let tempdir = tempfile::tempdir().expect("tempdir must be created");
        let backup = NewOutput::from_str(
            tempdir
                .path()
                .join("backup.fbk")
                .to_str()
                .expect("path must be valid UTF-8"),
        )
        .expect("backup path must parse");
        let audit_log = tempdir.path().join("audit.jsonl");
        let fingerprint = "a".repeat(64);

        let error = run_guarded_sequence(
            &mut io,
            CustomSettingSlot::C4,
            &fingerprint,
            REQUIRED_ACKNOWLEDGEMENT,
            &fingerprint,
            None,
            None,
            &backup,
            &audit_log,
            sample_context(),
        )
        .expect_err("a restore-write failure must still return an error, never exit zero");

        assert!(
            error.to_string().contains("simulated write failure"),
            "the original probe error must reach the operator unmasked: {error}"
        );

        let lines = read_audit_lines(&audit_log);
        assert_eq!(
            lines.len(),
            2,
            "the terminal record must still be appended on the failure path"
        );
        assert_eq!(lines[0]["outcome"], "attempted");
        assert_eq!(lines[1]["outcome"], "restore_failed");
        assert_eq!(
            lines[0]["invocation_id"], lines[1]["invocation_id"],
            "both lines of one attempt must share one invocation_id"
        );
    }

    #[test]
    fn backup_export_failure_aborts_before_any_write() {
        let _sequence = sequence_lock();
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
            None,
            None,
            &backup,
            &audit_log,
            sample_context(),
        )
        .expect_err("a failed pre-backup export must abort the sequence");

        assert!(error.to_string().contains("backup"));
        assert_eq!(io.write_call_count(), 0);
    }

    #[test]
    fn equal_slot_names_are_refused_before_any_write_or_backup() {
        let _sequence = sequence_lock();
        let mut io = FakeProbeIo::new(vec![0x01]);
        let tempdir = tempfile::tempdir().expect("tempdir must be created");
        let backup_path = tempdir.path().join("backup.fbk");
        let backup =
            NewOutput::from_str(backup_path.to_str().unwrap()).expect("backup path must parse");
        let audit_log = tempdir.path().join("audit.jsonl");
        let fingerprint = "a".repeat(64);

        let error = run_guarded_sequence(
            &mut io,
            CustomSettingSlot::C1,
            &fingerprint,
            REQUIRED_ACKNOWLEDGEMENT,
            &fingerprint,
            Some("same-name"),
            Some("same-name"),
            &backup,
            &audit_log,
            sample_context(),
        )
        .expect_err("equal still/movie slot names cannot discriminate the namespaces");

        assert!(
            error.to_string().contains("differ"),
            "the error must explain that the two names must differ: {error}"
        );
        assert_eq!(io.write_call_count(), 0);
        assert!(io.calls().is_empty());
        assert!(
            !backup_path.exists(),
            "no backup must be written when the slot-name gate refuses"
        );
        assert!(
            !audit_log.exists(),
            "no audit record must be written when the slot-name gate refuses"
        );
    }

    #[test]
    fn matching_d18d_readback_resolves_still_verdict_and_restores_the_snapshot() {
        let _sequence = sequence_lock();
        let mut io = FakeProbeIo::new(vec![0xAA]);
        io.d18d_payload = Some(ptp_string_payload("still-c1"));
        let tempdir = tempfile::tempdir().expect("tempdir must be created");
        let backup = NewOutput::from_str(
            tempdir
                .path()
                .join("backup.fbk")
                .to_str()
                .expect("path must be valid UTF-8"),
        )
        .expect("backup path must parse");
        let audit_log = tempdir.path().join("audit.jsonl");
        let fingerprint = "a".repeat(64);

        let verdict = run_guarded_sequence(
            &mut io,
            CustomSettingSlot::C1,
            &fingerprint,
            REQUIRED_ACKNOWLEDGEMENT,
            &fingerprint,
            Some("still-c1"),
            Some("movie-c1"),
            &backup,
            &audit_log,
            sample_context(),
        )
        .expect("a 0xD18D readback matching the still name must resolve a verdict");

        assert_eq!(verdict, Verdict::Still);
        assert_eq!(io.write_call_count(), 2);
        assert_eq!(
            *io.prop_value.borrow(),
            vec![0xAA],
            "the pre-probe 0xD18C value must be restored"
        );
    }

    #[test]
    fn unreadable_d18d_stays_ambiguous_but_still_restores_the_snapshot() {
        let _sequence = sequence_lock();
        let mut io = FakeProbeIo::new(vec![0xAA]);
        io.d18d_payload = None;
        let tempdir = tempfile::tempdir().expect("tempdir must be created");
        let backup = NewOutput::from_str(
            tempdir
                .path()
                .join("backup.fbk")
                .to_str()
                .expect("path must be valid UTF-8"),
        )
        .expect("backup path must parse");
        let audit_log = tempdir.path().join("audit.jsonl");
        let fingerprint = "a".repeat(64);

        let verdict = run_guarded_sequence(
            &mut io,
            CustomSettingSlot::C1,
            &fingerprint,
            REQUIRED_ACKNOWLEDGEMENT,
            &fingerprint,
            Some("still-c1"),
            Some("movie-c1"),
            &backup,
            &audit_log,
            sample_context(),
        )
        .expect("an unreadable 0xD18D readback must not abort the sequence");

        assert_eq!(verdict, Verdict::Ambiguous);
        assert_eq!(io.write_call_count(), 2);
        assert_eq!(
            *io.prop_value.borrow(),
            vec![0xAA],
            "the pre-probe 0xD18C value must still be restored on an unreadable 0xD18D"
        );
    }

    #[test]
    fn namespace_signal_computed_only_when_both_slot_names_are_given() {
        let mut io = FakeProbeIo::new(vec![0xAA]);
        io.d18d_payload = Some(ptp_string_payload("still-c1"));
        let names = super::SlotNames {
            still: "still-c1".to_owned(),
            movie: "movie-c1".to_owned(),
        };

        let (_observed, signal) = run_write_sequence(&mut io, CustomSettingSlot::C1, Some(&names))
            .expect("sequence must succeed against the fake");

        assert_eq!(signal, Some(NamespaceSignal::OnlyStillChanged));
    }
}
