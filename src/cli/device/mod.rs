use clap::Subcommand;

use crate::cli::{
    DeviceOptions, EmulationOptions, JsonOptions,
    common::{file::write_stdout_line, usb},
};
use fujicli::CameraInfoListItem;

#[derive(Subcommand, Debug, Clone)]
pub enum DeviceCmd {
    /// List cameras
    #[command(alias = "l")]
    List {
        #[command(flatten)]
        output: JsonOptions,
    },

    /// Get camera info
    #[command(alias = "i")]
    Info {
        #[command(flatten)]
        output: JsonOptions,

        #[command(flatten)]
        device: DeviceOptions,

        #[command(flatten)]
        emulation: EmulationOptions,
    },
}

fn handle_list(json: bool) -> anyhow::Result<()> {
    let cameras: Vec<CameraInfoListItem> = usb::get_all_cameras()?;

    if json {
        write_stdout_line(format_args!("{}", serde_json::to_string_pretty(&cameras)?))?;
        return Ok(());
    }

    if cameras.is_empty() {
        write_stdout_line(format_args!("No supported cameras connected"))?;
        return Ok(());
    }

    for d in cameras {
        write_stdout_line(format_args!("- {d}"))?;
    }

    Ok(())
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "command handlers consume parsed CLI values"
)]
fn handle_info(
    json: bool,
    device: DeviceOptions,
    emulation: EmulationOptions,
) -> anyhow::Result<()> {
    let DeviceOptions { device } = device;
    let EmulationOptions { emulate } = emulation;
    let mut camera = usb::get_native_camera(device, emulate)?;

    let repr = camera.get_info()?;

    if json {
        write_stdout_line(format_args!("{}", serde_json::to_string_pretty(&repr)?))?;
        return Ok(());
    }

    write_stdout_line(format_args!("{repr}"))?;
    Ok(())
}

pub fn handle(cmd: DeviceCmd) -> anyhow::Result<()> {
    match cmd {
        DeviceCmd::List { output } => handle_list(output.json),
        DeviceCmd::Info {
            output,
            device,
            emulation,
        } => handle_info(output.json, device, emulation),
    }
}
