use fujicli::{
    features::simulation::{
        SimulationFailureState, SimulationListItem, SimulationTransactionError,
        SimulationTransactionSuccess, SimulationWriteReceipt, TemporarySimulationSelectorError,
        TemporarySimulationSelectorState,
    },
    generated::{cli::SimulationArgs, options::CustomSetting, simulations::SimulationBase},
    policy::SerialFingerprint,
};

use super::common::file::{Input, Output, write_stdout_line};
use crate::cli::{
    DeviceOptions, JsonOptions,
    common::{camera_state::CameraStateUnknown, interrupt, usb},
};
use clap::Subcommand;
use serde::Serialize;

pub const MAX_SIMULATION_INPUT_BYTES: usize = 1024 * 1024;

/// Whether a `SimulationTransactionError`'s `state()` means the transaction
/// left the camera's property state unconfirmed. An exhaustive match with no
/// wildcard arm, so a new `SimulationFailureState` variant fails to compile
/// here instead of silently defaulting to "known".
const fn transaction_state_is_unknown(state: SimulationFailureState) -> bool {
    match state {
        SimulationFailureState::CameraStateUnknown => true,
        SimulationFailureState::RejectedWithoutChange
        | SimulationFailureState::RollbackVerified => false,
    }
}

/// Whether a `TemporarySimulationSelectorError`'s `state()` means the
/// selector-restore outcome is unknown. An exhaustive match with no wildcard
/// arm, so a new `TemporarySimulationSelectorState` variant fails to compile
/// here instead of silently defaulting to "known".
const fn selector_state_is_unknown(state: TemporarySimulationSelectorState) -> bool {
    match state {
        TemporarySimulationSelectorState::Unknown => true,
        TemporarySimulationSelectorState::RestoredAndVerified => false,
    }
}

/// Attach the [`CameraStateUnknown`] marker to any error, underneath the
/// error's own `Display` text: the marker is constructed first and `error`
/// is pushed on top via `.context(...)`, so `error` remains the chain's head
/// and the displayed message is unchanged from `error`'s own `Display` impl.
/// `error.is::<CameraStateUnknown>()` finds the marker regardless of where
/// in the chain it sits.
fn tag_state_unknown<E>(error: E) -> anyhow::Error
where
    E: std::error::Error + Send + Sync + 'static,
{
    anyhow::Error::new(CameraStateUnknown).context(error)
}

/// Convert a `SimulationTransactionError` from a simulation write into
/// `anyhow::Error`, attaching the [`CameraStateUnknown`] marker when the
/// transaction left the camera's property state unconfirmed.
fn tag_transaction_state_unknown<T>(
    result: Result<T, SimulationTransactionError>,
) -> anyhow::Result<T> {
    result.map_err(|error| {
        if transaction_state_is_unknown(error.state()) {
            tag_state_unknown(error)
        } else {
            anyhow::Error::new(error)
        }
    })
}

/// Attach the [`CameraStateUnknown`] marker on top of `error`'s existing
/// chain via `error.context(CameraStateUnknown)`, rather than rebuilding a
/// fresh `anyhow::Error` from an extracted concrete error the way
/// [`tag_state_unknown`] does. `error` here is already a full
/// `anyhow::Error` chain rather than a bare `std::error::Error`, so it
/// cannot satisfy `tag_state_unknown`'s `E: std::error::Error` bound; more
/// importantly, extracting a concrete error out of the chain (`downcast`)
/// would discard every other layer already on it -- including an
/// [`Interrupted`](fujicli::interrupt::Interrupted) marker that
/// `InterruptLatch::after_transaction` may have attached on the read path
/// after the selector-restore transaction completed. `CameraStateUnknown`
/// becomes the new head here (its own `Display` text replaces `error`'s as
/// the outermost message), but every prior frame -- `error`'s own message
/// and any marker on it -- stays reachable through `.source()`/`downcast_ref`
/// and still appears in `anyhow`'s multi-line `Debug` output.
fn context_state_unknown(error: anyhow::Error) -> anyhow::Error {
    error.context(CameraStateUnknown)
}

/// Attach the [`CameraStateUnknown`] marker to a simulation read failure
/// caused by a `TemporarySimulationSelectorError` whose selector-restore
/// outcome is unknown: a `get_simulation`/`get_simulations` call that
/// succeeds but then fails to restore the previously selected slot leaves
/// selector state unknown, exactly like a failed write. Any other error
/// (including a selector failure the library already restored and
/// verified) passes through unchanged.
///
/// Inspects with `downcast_ref` rather than `downcast` so tagging never
/// consumes and discards `error`'s own chain; see [`context_state_unknown`]
/// for why the marker is attached to the whole original `error` rather than
/// to the extracted `TemporarySimulationSelectorError` alone.
fn tag_selector_state_unknown<T>(result: anyhow::Result<T>) -> anyhow::Result<T> {
    result.map_err(|error| {
        if error
            .downcast_ref::<TemporarySimulationSelectorError>()
            .is_some_and(|selector_error| selector_state_is_unknown(selector_error.state()))
        {
            context_state_unknown(error)
        } else {
            error
        }
    })
}

/// One entry of [`SimulationWriteOutcomeJson::written`]: the setting name
/// and PTP property code of a confirmed write, in write order.
/// `property_code` uses the `0x{:04x}` hex-string convention already used
/// for `vendorId`/`productId` in `device list --json`, rather than a bare
/// integer.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SimulationWriteReceiptJson {
    setting: &'static str,
    property_code: String,
}

impl From<&SimulationWriteReceipt> for SimulationWriteReceiptJson {
    fn from(receipt: &SimulationWriteReceipt) -> Self {
        Self {
            setting: receipt.setting,
            property_code: format!("0x{:04x}", receipt.property_code),
        }
    }
}

/// Discriminant for [`SimulationWriteOutcomeJson::outcome`], serialized as
/// `snake_case` text so it reads the same as the Rust variant names on
/// [`SimulationTransactionSuccess`].
#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum SimulationWriteOutcomeName {
    AppliedAndVerified,
    NoChangeVerified,
}

/// The `simulation set`/`simulation import` `--json` success document: which
/// slot was written, whether the write changed anything, and exactly what
/// was written.
#[derive(Debug, Serialize)]
struct SimulationWriteOutcomeJson {
    /// Serializes as `CustomSetting`'s `Display` text (e.g. `"C3"`) because
    /// the generated `CustomSetting` type derives `SerializeDisplay`.
    slot: CustomSetting,
    outcome: SimulationWriteOutcomeName,
    written: Vec<SimulationWriteReceiptJson>,
}

impl SimulationWriteOutcomeJson {
    fn new(slot: CustomSetting, success: &SimulationTransactionSuccess) -> Self {
        let outcome = match success {
            SimulationTransactionSuccess::AppliedAndVerified(_) => {
                SimulationWriteOutcomeName::AppliedAndVerified
            }
            SimulationTransactionSuccess::NoChangeVerified => {
                SimulationWriteOutcomeName::NoChangeVerified
            }
        };
        Self {
            slot,
            outcome,
            written: success
                .journal()
                .iter()
                .map(SimulationWriteReceiptJson::from)
                .collect(),
        }
    }
}

/// The `simulation set`/`simulation import` text-mode success line: exactly
/// what was written to `slot`, or that the slot already matched and nothing
/// was written. Pure so it can be unit-tested against a constructed
/// [`SimulationTransactionSuccess`] without a camera.
fn describe_simulation_write(
    slot: CustomSetting,
    success: &SimulationTransactionSuccess,
) -> String {
    match success {
        SimulationTransactionSuccess::AppliedAndVerified(journal) => {
            let settings = journal
                .iter()
                .map(|receipt| format!("{} (0x{:04x})", receipt.setting, receipt.property_code))
                .collect::<Vec<_>>()
                .join(", ");
            let count = journal.len();
            let noun = if count == 1 { "setting" } else { "settings" };
            format!("simulation {slot}: applied and verified, {count} {noun} written: {settings}")
        }
        SimulationTransactionSuccess::NoChangeVerified => {
            format!("simulation {slot}: no change, the slot already matched")
        }
    }
}

/// Report a completed simulation write on stdout: one text line by default,
/// or one JSON document with `--json`, mirroring `handle_get`'s split.
fn report_simulation_write(
    json: bool,
    slot: CustomSetting,
    success: &SimulationTransactionSuccess,
) -> anyhow::Result<()> {
    if json {
        let document = SimulationWriteOutcomeJson::new(slot, success);
        write_stdout_line(format_args!("{}", serde_json::to_string(&document)?))
    } else {
        write_stdout_line(format_args!("{}", describe_simulation_write(slot, success)))
    }
}

#[derive(Subcommand, Debug)]
pub enum SimulationCmd {
    /// List simulations
    #[command(alias = "l")]
    List {
        #[command(flatten)]
        output_format: JsonOptions,

        #[command(flatten)]
        device: DeviceOptions,
    },

    /// Get simulation
    #[command(alias = "g")]
    Get {
        /// Simulation slot number
        slot: CustomSetting,

        #[command(flatten)]
        output_format: JsonOptions,

        #[command(flatten)]
        device: DeviceOptions,
    },

    /// Set simulation parameters
    #[command(alias = "s")]
    Set {
        /// Simulation slot number
        slot: CustomSetting,

        #[command(flatten)]
        simulation: SimulationArgs,

        /// SHA-256 fingerprint of the exact physical camera serial number
        #[arg(long)]
        target_serial_sha256: SerialFingerprint,

        #[command(flatten)]
        output_format: JsonOptions,

        #[command(flatten)]
        device: DeviceOptions,
    },

    /// Export simulation
    #[command(alias = "e")]
    Export {
        /// Simulation slot number
        slot: CustomSetting,

        /// Output file (use '-' to write to stdout)
        output: Output,

        /// Replace an existing regular output file
        #[arg(long)]
        force: bool,

        #[command(flatten)]
        device: DeviceOptions,
    },

    /// Import simulation
    #[command(alias = "i")]
    Import {
        /// Simulation slot number
        slot: CustomSetting,

        /// Input file (use '-' to read from stdin)
        input: Input,

        /// SHA-256 fingerprint of the exact physical camera serial number
        #[arg(long)]
        target_serial_sha256: SerialFingerprint,

        #[command(flatten)]
        output_format: JsonOptions,

        #[command(flatten)]
        device: DeviceOptions,
    },
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "command handlers consume parsed CLI values"
)]
fn handle_list(json: bool, device: DeviceOptions) -> anyhow::Result<()> {
    let mut camera = usb::get_native_camera(device.device, None)?;
    let slots = camera.custom_settings_slots()?;
    let mut session = camera.preflight_simulation_access()?;
    let slots: Vec<SimulationListItem> =
        tag_selector_state_unknown(session.get_simulations(&slots))?
            .into_iter()
            .map(|(slot, simulation)| {
                let name = simulation.name();
                SimulationListItem { slot, name }
            })
            .collect();

    if json {
        write_stdout_line(format_args!("{}", serde_json::to_string_pretty(&slots)?))?;
    } else {
        for slot in slots {
            write_stdout_line(format_args!("- {slot}"))?;
        }
    }

    Ok(())
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "command handlers consume parsed CLI values"
)]
fn handle_get(json: bool, device: DeviceOptions, slot: CustomSetting) -> anyhow::Result<()> {
    let mut camera = usb::get_native_camera(device.device, None)?;
    let mut session = camera.preflight_simulation_access()?;
    let simulation = tag_selector_state_unknown(session.get_simulation(slot))?;

    if json {
        write_stdout_line(format_args!(
            "{}",
            serde_json::to_string_pretty(&simulation)?
        ))?;
    } else {
        write_stdout_line(format_args!("{simulation}"))?;
    }

    Ok(())
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "command handlers consume parsed CLI values"
)]
fn handle_set(
    device: DeviceOptions,
    simulation: SimulationArgs,
    slot: CustomSetting,
    target_serial_sha256: SerialFingerprint,
    json: bool,
) -> anyhow::Result<()> {
    let mut camera = usb::get_native_camera(device.device, None)?;
    let partial: SimulationBase = simulation.into();
    let mut session = camera.preflight_simulation_write(&target_serial_sha256)?;
    let success = interrupt::critical_camera_write("simulation update", || {
        tag_transaction_state_unknown(session.update_simulation(slot, partial))
    })?;
    report_simulation_write(json, slot, &success)
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "command handlers consume parsed CLI values"
)]
fn handle_export(
    device: DeviceOptions,
    slot: CustomSetting,
    output: Output,
    force: bool,
) -> anyhow::Result<()> {
    let output_transaction = output.begin_write(force)?;

    let mut camera = usb::get_native_camera(device.device, None)?;
    let mut session = camera.preflight_simulation_access()?;
    let simulation = tag_selector_state_unknown(session.get_simulation(slot))?;
    drop(session);
    let simulation = camera.serialize_simulation(&*simulation)?;
    output_transaction.write_artifact(&simulation)?;

    Ok(())
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "command handlers consume parsed CLI values"
)]
fn handle_import(
    device: DeviceOptions,
    slot: CustomSetting,
    input: Input,
    target_serial_sha256: SerialFingerprint,
    json: bool,
) -> anyhow::Result<()> {
    let buffer = input.read_limited(MAX_SIMULATION_INPUT_BYTES, "simulation JSON")?;
    let mut camera = usb::get_native_camera(device.device, None)?;
    let simulation = camera.deserialize_simulation(&buffer)?;
    let mut session = camera.preflight_simulation_write(&target_serial_sha256)?;
    let success = interrupt::critical_camera_write("simulation write", || {
        tag_transaction_state_unknown(session.set_simulation(slot, &*simulation))
    })?;

    report_simulation_write(json, slot, &success)
}

pub fn handle(cmd: SimulationCmd) -> anyhow::Result<()> {
    match cmd {
        SimulationCmd::List {
            output_format,
            device,
        } => handle_list(output_format.json, device),
        SimulationCmd::Get {
            slot,
            output_format,
            device,
        } => handle_get(output_format.json, device, slot),
        SimulationCmd::Set {
            slot,
            simulation,
            target_serial_sha256,
            output_format,
            device,
        } => handle_set(
            device,
            simulation,
            slot,
            target_serial_sha256,
            output_format.json,
        ),
        SimulationCmd::Export {
            slot,
            output,
            force,
            device,
        } => handle_export(device, slot, output, force),
        SimulationCmd::Import {
            slot,
            input,
            target_serial_sha256,
            output_format,
            device,
        } => handle_import(
            device,
            slot,
            input,
            target_serial_sha256,
            output_format.json,
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::fmt;

    use fujicli::{
        features::simulation::{SimulationTransactionSuccess, SimulationWriteReceipt},
        generated::options::CustomSetting,
        interrupt::Interrupted,
    };

    use super::{
        CameraStateUnknown, SimulationFailureState, SimulationWriteOutcomeJson,
        TemporarySimulationSelectorState, context_state_unknown, describe_simulation_write,
        selector_state_is_unknown, tag_state_unknown, transaction_state_is_unknown,
    };

    /// Minimal `std::error::Error` with a known, fixed `Display` output,
    /// standing in for `SimulationTransactionError`/
    /// `TemporarySimulationSelectorError`: neither of those two types can be
    /// constructed outside the `fujicli` library crate (their only
    /// constructors are `pub(crate)`), so the marker-attachment mechanism
    /// is tested against this dummy instead.
    #[derive(Debug)]
    struct DummyError;

    impl fmt::Display for DummyError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(formatter, "dummy error message")
        }
    }

    impl std::error::Error for DummyError {}

    #[test]
    fn transaction_predicate_is_true_only_for_camera_state_unknown() {
        assert!(transaction_state_is_unknown(
            SimulationFailureState::CameraStateUnknown
        ));
        assert!(!transaction_state_is_unknown(
            SimulationFailureState::RejectedWithoutChange
        ));
        assert!(!transaction_state_is_unknown(
            SimulationFailureState::RollbackVerified
        ));
    }

    #[test]
    fn selector_predicate_is_true_only_for_unknown() {
        assert!(selector_state_is_unknown(
            TemporarySimulationSelectorState::Unknown
        ));
        assert!(!selector_state_is_unknown(
            TemporarySimulationSelectorState::RestoredAndVerified
        ));
    }

    #[test]
    fn tagged_error_carries_the_marker_and_keeps_the_original_display() {
        let tagged = tag_state_unknown(DummyError);

        assert!(tagged.is::<CameraStateUnknown>());
        assert_eq!(tagged.to_string(), DummyError.to_string());
    }

    #[test]
    fn untagged_error_does_not_carry_the_marker() {
        let untagged = anyhow::Error::new(DummyError);

        assert!(!untagged.is::<CameraStateUnknown>());
        assert_eq!(untagged.to_string(), DummyError.to_string());
    }

    /// A selector-restore failure on the read path can already carry an
    /// `Interrupted` marker underneath it (attached by
    /// `InterruptLatch::after_transaction` once the selector-restore
    /// transaction completes). `context_state_unknown` -- the operation
    /// `tag_selector_state_unknown` applies once it confirms, via
    /// `downcast_ref`, that the selector-restore outcome is unknown -- must
    /// keep that marker reachable rather than discard it the way extracting
    /// the selector error with `downcast` would.
    #[test]
    fn tagging_a_selector_error_keeps_an_existing_interrupted_marker() {
        let error = anyhow::Error::new(Interrupted {
            after_camera_write: false,
        })
        .context("temporary simulation selector transaction failed");

        let tagged = context_state_unknown(error);

        assert!(tagged.is::<CameraStateUnknown>());
        assert!(
            tagged.downcast_ref::<Interrupted>().is_some(),
            "the Interrupted marker underneath the selector error must survive tagging: \
             {tagged:?}"
        );
    }

    #[test]
    fn describes_an_applied_write_by_setting_and_property_code() {
        let success = SimulationTransactionSuccess::AppliedAndVerified(vec![
            SimulationWriteReceipt {
                setting: "film_simulation",
                property_code: 0xd192,
            },
            SimulationWriteReceipt {
                setting: "sharpness",
                property_code: 0xd1a0,
            },
        ]);

        let line = describe_simulation_write(CustomSetting::C3, &success);

        assert_eq!(
            line,
            "simulation C3: applied and verified, 2 settings written: \
             film_simulation (0xd192), sharpness (0xd1a0)"
        );
    }

    #[test]
    fn describes_a_single_setting_write_in_the_singular() {
        let success =
            SimulationTransactionSuccess::AppliedAndVerified(vec![SimulationWriteReceipt {
                setting: "film_simulation",
                property_code: 0xd192,
            }]);

        let line = describe_simulation_write(CustomSetting::C1, &success);

        assert_eq!(
            line,
            "simulation C1: applied and verified, 1 setting written: film_simulation (0xd192)"
        );
    }

    #[test]
    fn describes_no_change_without_a_written_list() {
        let line = describe_simulation_write(
            CustomSetting::C3,
            &SimulationTransactionSuccess::NoChangeVerified,
        );

        assert_eq!(line, "simulation C3: no change, the slot already matched");
    }

    #[test]
    fn json_document_for_an_applied_write_lists_every_receipt() {
        let success = SimulationTransactionSuccess::AppliedAndVerified(vec![
            SimulationWriteReceipt {
                setting: "film_simulation",
                property_code: 0xd192,
            },
            SimulationWriteReceipt {
                setting: "sharpness",
                property_code: 0xd1a0,
            },
        ]);

        let document = SimulationWriteOutcomeJson::new(CustomSetting::C3, &success);
        let json = serde_json::to_value(&document).expect("document must serialize");

        assert_eq!(
            json,
            serde_json::json!({
                "slot": "C3",
                "outcome": "applied_and_verified",
                "written": [
                    {"setting": "film_simulation", "propertyCode": "0xd192"},
                    {"setting": "sharpness", "propertyCode": "0xd1a0"},
                ],
            })
        );
    }

    #[test]
    fn json_document_for_no_change_carries_an_empty_written_list() {
        let document = SimulationWriteOutcomeJson::new(
            CustomSetting::C3,
            &SimulationTransactionSuccess::NoChangeVerified,
        );
        let json = serde_json::to_value(&document).expect("document must serialize");

        assert_eq!(
            json,
            serde_json::json!({
                "slot": "C3",
                "outcome": "no_change_verified",
                "written": [],
            })
        );
    }
}
