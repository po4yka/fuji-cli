use fujicli::{
    features::render::RenderCleanupError,
    generated::{
        cli::RenderArgs, options::CustomSetting, renders::RenderBase, simulations::SimulationBase,
    },
};

use super::common::file::{Input, Output};
use crate::cli::{GlobalOptions, common::usb};
use clap::Subcommand;

const MAX_IMAGE_INPUT_BYTES: usize = 512 * 1024 * 1024;

fn write_render_result(
    output: &Output,
    render_result: anyhow::Result<Vec<u8>>,
) -> anyhow::Result<()> {
    match render_result {
        Ok(rendered) => output.write_all(&rendered),
        Err(error) => {
            if let Some(cleanup) = error.downcast_ref::<RenderCleanupError>() {
                output.write_all(cleanup.rendered_data())?;
            }
            Err(error)
        }
    }
}

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
        device,
        emulate,
        allow_emulated_transient_write,
        ..
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

    let mut camera = usb::get_camera(device, emulate, allow_emulated_transient_write)?;

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

    write_render_result(&output, camera.render(&image, base, draft))
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

#[cfg(test)]
mod tests {
    use std::fs;

    use anyhow::anyhow;
    use fujicli::features::render::RenderCleanupError;
    use tempfile::tempdir;

    use super::{Output, write_render_result};

    #[test]
    fn saves_rendered_image_before_returning_camera_cleanup_failure() -> anyhow::Result<()> {
        let directory = tempdir()?;
        let destination = directory.path().join("rendered.jpg");
        let rendered = b"rendered JPEG".to_vec();
        let render_result = Err(RenderCleanupError::new(
            rendered.clone(),
            anyhow!("simulated camera cleanup failure"),
        )
        .into());

        let error = write_render_result(&Output::Path(destination.clone()), render_result)
            .expect_err("camera cleanup failure must keep the command unsuccessful");

        assert!(
            destination.exists(),
            "successfully fetched JPEG must be saved despite cleanup failure"
        );
        assert_eq!(fs::read(destination)?, rendered);
        assert!(
            error
                .to_string()
                .contains("rendered image was fetched, but camera cleanup failed"),
            "unexpected cleanup error: {error:#}"
        );
        Ok(())
    }
}
