use fujicli::generated::{
    cli::RenderArgs, options::CustomSetting, renders::RenderBase, simulations::SimulationBase,
};

use super::common::file::{Input, Output};
use crate::cli::{GlobalOptions, common::usb};
use clap::Subcommand;

const MAX_IMAGE_INPUT_BYTES: usize = 512 * 1024 * 1024;

#[derive(Subcommand, Debug)]
pub enum ImageCmd {
    /// Render image
    #[command(alias = "r")]
    Render {
        /// Simulation slot number
        #[arg(long, conflicts_with = "simulation_file")]
        slot: Option<CustomSetting>,

        /// Path to exported simulation file
        #[arg(long, conflicts_with = "slot")]
        simulation_file: Option<Input>,

        /// Render a lower-quality (faster) preview
        #[arg(long)]
        draft: bool,

        #[command(flatten)]
        render: RenderArgs,

        /// RAF input file (use '-' to read from stdin)
        input: Input,

        /// Output file (use '-' to write to stdout)
        output: Output,
    },
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "the handler consumes the flattened CLI render command"
)]
fn handle_render(
    options: GlobalOptions,
    slot: Option<CustomSetting>,
    simulation_file: Option<Input>,
    draft: bool,
    render: RenderArgs,
    input: Input,
    output: Output,
) -> anyhow::Result<()> {
    let GlobalOptions {
        device, emulate, ..
    } = options;

    let image = input.read_limited(MAX_IMAGE_INPUT_BYTES, "RAF image")?;
    let simulation_json = simulation_file
        .map(|file| {
            file.read_limited(
                super::simulation::MAX_SIMULATION_INPUT_BYTES,
                "simulation JSON",
            )
        })
        .transpose()?;

    let mut camera = usb::get_camera(device, emulate)?;

    let simulation_base: Option<SimulationBase> = if let Some(slot) = slot {
        Some(camera.get_simulation(slot)?.to_base())
    } else if let Some(buffer) = simulation_json {
        Some(camera.deserialize_simulation(&buffer)?.to_base())
    } else {
        None
    };

    let mut base = RenderBase::default();
    if let Some(sim) = simulation_base {
        base.try_update_from(&sim);
    }
    base.merge(render.into());

    let rendered = camera.render(&image, base, draft)?;

    output.write_all(&rendered)?;

    Ok(())
}

pub fn handle(cmd: ImageCmd, options: GlobalOptions) -> anyhow::Result<()> {
    match cmd {
        ImageCmd::Render {
            slot,
            simulation_file,
            draft,
            render,
            input,
            output,
        } => handle_render(options, slot, simulation_file, draft, render, input, output),
    }
}
