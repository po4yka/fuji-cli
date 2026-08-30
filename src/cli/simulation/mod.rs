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

/// Convert a `SimulationTransactionError` from a simulation write into
/// `anyhow::Error`, attaching the [`CameraStateUnknown`] marker when the
/// transaction left the camera's property state unconfirmed. The marker is
/// inserted underneath the error's own `Display` text via
/// `.context(...)`, so the displayed message is unchanged from
/// `SimulationTransactionError`'s own `Display` impl.
fn tag_transaction_state_unknown<T>(
    result: Result<T, SimulationTransactionError>,
) -> anyhow::Result<T> {
    result.map_err(|error| match error.state() {
        SimulationFailureState::CameraStateUnknown => {
            anyhow::Error::new(CameraStateUnknown).context(error)
        }
        SimulationFailureState::RejectedWithoutChange
        | SimulationFailureState::RollbackVerified => anyhow::Error::new(error),
    })
}

/// Attach the [`CameraStateUnknown`] marker to a simulation read failure
/// caused by a `TemporarySimulationSelectorError` whose selector-restore
/// outcome is unknown: a `get_simulation`/`get_simulations` call that
/// succeeds but then fails to restore the previously selected slot leaves
/// selector state unknown, exactly like a failed write. Any other error
/// (including a selector failure the library already restored and
/// verified) passes through unchanged. The marker is inserted underneath
/// the error's own `Display` text, so the displayed message is unchanged.
fn tag_selector_state_unknown<T>(result: anyhow::Result<T>) -> anyhow::Result<T> {
    result.map_err(
        |error| match error.downcast::<TemporarySimulationSelectorError>() {
            Ok(selector_error) => match selector_error.state() {
                TemporarySimulationSelectorState::Unknown => {
                    anyhow::Error::new(CameraStateUnknown).context(selector_error)
                }
                TemporarySimulationSelectorState::RestoredAndVerified => {
                    anyhow::Error::new(selector_error)
                }
            },
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
) -> anyhow::Result<()> {
    let GlobalOptions {
        device, emulate, ..
    } = options;

    let mut camera = usb::get_native_camera(device, emulate)?;
    let mut session = camera.preflight_simulation_access()?;
    let simulation = tag_selector_state_unknown(session.get_simulation(slot))?;
    drop(session);
    let simulation = camera.serialize_simulation(&*simulation)?;
    output.write_all(&simulation)?;

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
        SimulationCmd::Export { slot, output } => handle_export(options, slot, output),
        SimulationCmd::Import {
            slot,
            input,
            target_serial_sha256,
        } => handle_import(options, slot, input, target_serial_sha256),
    }
}
