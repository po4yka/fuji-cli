use fujicli::{
    features::simulation::{
        SimulationFailureState, SimulationListItem, SimulationTransactionError,
        TemporarySimulationSelectorError, TemporarySimulationSelectorState,
    },
    generated::{cli::SimulationArgs, options::CustomSetting, simulations::SimulationBase},
    policy::SerialFingerprint,
};

use super::common::file::{Input, Output, write_stdout_line};
use crate::cli::{
    GlobalOptions,
    common::{camera_state::CameraStateUnknown, interrupt, usb},
};
use clap::Subcommand;

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

/// Attach the [`CameraStateUnknown`] marker to a simulation read failure
/// caused by a `TemporarySimulationSelectorError` whose selector-restore
/// outcome is unknown: a `get_simulation`/`get_simulations` call that
/// succeeds but then fails to restore the previously selected slot leaves
/// selector state unknown, exactly like a failed write. Any other error
/// (including a selector failure the library already restored and
/// verified) passes through unchanged.
fn tag_selector_state_unknown<T>(result: anyhow::Result<T>) -> anyhow::Result<T> {
    result.map_err(
        |error| match error.downcast::<TemporarySimulationSelectorError>() {
            Ok(selector_error) if selector_state_is_unknown(selector_error.state()) => {
                tag_state_unknown(selector_error)
            }
            Ok(selector_error) => anyhow::Error::new(selector_error),
            Err(other) => other,
        },
    )
}

#[derive(Subcommand, Debug)]
pub enum SimulationCmd {
    /// List simulations
    #[command(alias = "l")]
    List,

    /// Get simulation
    #[command(alias = "g")]
    Get {
        /// Simulation slot number
        slot: CustomSetting,
    },

    /// Set simulation parameters
    #[command(alias = "s")]
    Set {
        /// Simulation slot number
        slot: CustomSetting,

        #[command(flatten)]
        simulation: SimulationArgs,

        /// SHA-256 fingerprint of the exact physical camera serial number
        #[arg(long, required_unless_present = "emulate")]
        target_serial_sha256: Option<SerialFingerprint>,
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
    },

    /// Import simulation
    #[command(alias = "i")]
    Import {
        /// Simulation slot number
        slot: CustomSetting,

        /// Input file (use '-' to read from stdin)
        input: Input,

        /// SHA-256 fingerprint of the exact physical camera serial number
        #[arg(long, required_unless_present = "emulate")]
        target_serial_sha256: Option<SerialFingerprint>,
    },
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "command handlers consume parsed CLI values"
)]
fn handle_list(options: GlobalOptions) -> anyhow::Result<()> {
    let GlobalOptions {
        json,
        device,
        emulate,
        ..
    } = options;

    let mut camera = usb::get_native_camera(device, emulate)?;
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
fn handle_get(options: GlobalOptions, slot: CustomSetting) -> anyhow::Result<()> {
    let GlobalOptions {
        json,
        device,
        emulate,
        ..
    } = options;

    let mut camera = usb::get_native_camera(device, emulate)?;
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
    options: GlobalOptions,
    simulation: SimulationArgs,
    slot: CustomSetting,
    target_serial_sha256: Option<SerialFingerprint>,
) -> anyhow::Result<()> {
    let GlobalOptions {
        device, emulate, ..
    } = options;

    let mut camera = usb::get_native_camera(device, emulate)?;
    let target_serial_sha256 = target_serial_sha256
        .ok_or_else(|| anyhow::anyhow!("simulation write requires --target-serial-sha256"))?;
    let partial: SimulationBase = simulation.into();
    let mut session = camera.preflight_simulation_write(&target_serial_sha256)?;
    interrupt::critical_camera_write("simulation update", || {
        tag_transaction_state_unknown(session.update_simulation(slot, partial))
    })?;
    Ok(())
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "command handlers consume parsed CLI values"
)]
fn handle_export(
    options: GlobalOptions,
    slot: CustomSetting,
    output: Output,
    force: bool,
) -> anyhow::Result<()> {
    let GlobalOptions {
        device, emulate, ..
    } = options;
    let mut output_transaction = output.begin_write(force)?;

    let mut camera = usb::get_native_camera(device, emulate)?;
    let mut session = camera.preflight_simulation_access()?;
    let simulation = tag_selector_state_unknown(session.get_simulation(slot))?;
    drop(session);
    let simulation = camera.serialize_simulation(&*simulation)?;
    std::io::Write::write_all(&mut output_transaction, &simulation)?;
    output_transaction.commit()?;

    Ok(())
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "command handlers consume parsed CLI values"
)]
fn handle_import(
    options: GlobalOptions,
    slot: CustomSetting,
    input: Input,
    target_serial_sha256: Option<SerialFingerprint>,
) -> anyhow::Result<()> {
    let GlobalOptions {
        device, emulate, ..
    } = options;

    let buffer = input.read_limited(MAX_SIMULATION_INPUT_BYTES, "simulation JSON")?;
    let mut camera = usb::get_native_camera(device, emulate)?;
    let simulation = camera.deserialize_simulation(&buffer)?;
    let target_serial_sha256 = target_serial_sha256
        .ok_or_else(|| anyhow::anyhow!("simulation write requires --target-serial-sha256"))?;
    let mut session = camera.preflight_simulation_write(&target_serial_sha256)?;
    interrupt::critical_camera_write("simulation write", || {
        tag_transaction_state_unknown(session.set_simulation(slot, &*simulation))
    })?;

    Ok(())
}

pub fn handle(cmd: SimulationCmd, options: GlobalOptions) -> anyhow::Result<()> {
    match cmd {
        SimulationCmd::List => handle_list(options),
        SimulationCmd::Get { slot } => handle_get(options, slot),
        SimulationCmd::Set {
            slot,
            simulation,
            target_serial_sha256,
        } => handle_set(options, simulation, slot, target_serial_sha256),
        SimulationCmd::Export {
            slot,
            output,
            force,
        } => handle_export(options, slot, output, force),
        SimulationCmd::Import {
            slot,
            input,
            target_serial_sha256,
        } => handle_import(options, slot, input, target_serial_sha256),
    }
}

#[cfg(test)]
mod tests {
    use std::fmt;

    use super::{
        CameraStateUnknown, SimulationFailureState, TemporarySimulationSelectorState,
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
}
