use fujicli::{
    features::simulation::SimulationListItem,
    generated::{cli::SimulationArgs, options::CustomSetting, simulations::SimulationBase},
};

use super::common::file::{Input, Output, write_stdout_line};
use crate::cli::{GlobalOptions, common::usb};
use clap::Subcommand;

pub const MAX_SIMULATION_INPUT_BYTES: usize = 1024 * 1024;

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
        allow_emulated_transient_write,
        ..
    } = options;

    let mut camera = usb::get_camera(device, emulate, allow_emulated_transient_write)?;

    let slots: Vec<SimulationListItem> = camera
        .custom_settings_slots()?
        .into_iter()
        .map(|slot| -> anyhow::Result<SimulationListItem> {
            let simulation = camera.get_simulation(slot)?;
            let name = simulation.name();
            Ok(SimulationListItem { slot, name })
        })
        .collect::<anyhow::Result<Vec<SimulationListItem>>>()?;

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
        allow_emulated_transient_write,
        ..
    } = options;

    let mut camera = usb::get_camera(device, emulate, allow_emulated_transient_write)?;

    let simulation = camera.get_simulation(slot)?;

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
) -> anyhow::Result<()> {
    let GlobalOptions {
        device,
        emulate,
        allow_emulated_transient_write,
        ..
    } = options;

    let mut camera = usb::get_camera(device, emulate, allow_emulated_transient_write)?;
    let partial: SimulationBase = simulation.into();
    camera.update_simulation(slot, partial)?;
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
        device,
        emulate,
        allow_emulated_transient_write,
        ..
    } = options;

    let mut camera = usb::get_camera(device, emulate, allow_emulated_transient_write)?;

    let simulation = camera.get_simulation(slot)?;
    let simulation = camera.serialize_simulation(&*simulation)?;
    output.write_all(&simulation)?;

    Ok(())
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "command handlers consume parsed CLI values"
)]
fn handle_import(options: GlobalOptions, slot: CustomSetting, input: Input) -> anyhow::Result<()> {
    let GlobalOptions {
        device,
        emulate,
        allow_emulated_transient_write,
        ..
    } = options;

    let buffer = input.read_limited(MAX_SIMULATION_INPUT_BYTES, "simulation JSON")?;
    let mut camera = usb::get_camera(device, emulate, allow_emulated_transient_write)?;
    let simulation = camera.deserialize_simulation(&buffer)?;
    camera.set_simulation(slot, &*simulation)?;

    Ok(())
}

pub fn handle(cmd: SimulationCmd, options: GlobalOptions) -> anyhow::Result<()> {
    match cmd {
        SimulationCmd::List => handle_list(options),
        SimulationCmd::Get { slot } => handle_get(options, slot),
        SimulationCmd::Set { slot, simulation } => handle_set(options, simulation, slot),
        SimulationCmd::Export { slot, output } => handle_export(options, slot, output),
        SimulationCmd::Import { slot, input } => handle_import(options, slot, input),
    }
}
