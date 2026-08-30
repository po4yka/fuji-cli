use clap::Subcommand;

use crate::cli::{
    GlobalOptions,
    common::{file::write_stdout_line, usb},
};
use fujicli::{features::base::info::CameraInfoListItem, policy::EmulationAcknowledgement};

#[derive(Subcommand, Debug, Clone, Copy)]
pub enum DeviceCmd {
    /// List cameras
    #[command(alias = "l")]
    List,

    /// Get camera info
    #[command(alias = "i")]
    Info,
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "command handlers consume parsed CLI values"
)]
fn handle_list(options: GlobalOptions) -> anyhow::Result<()> {
    let GlobalOptions { json, .. } = options;

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
fn handle_info(options: GlobalOptions) -> anyhow::Result<()> {
    let GlobalOptions {
        json,
        device,
        emulate,
        ..
    } = options;

    let mut camera = usb::get_camera(device, emulate, EmulationAcknowledgement::NotProvided)?;

    let repr = camera.get_info()?;

    if json {
        write_stdout_line(format_args!("{}", serde_json::to_string_pretty(&repr)?))?;
        return Ok(());
    }

    write_stdout_line(format_args!("{repr}"))?;
    Ok(())
}

pub fn handle(cmd: DeviceCmd, options: GlobalOptions) -> anyhow::Result<()> {
    match cmd {
        DeviceCmd::List => handle_list(options),
        DeviceCmd::Info => handle_info(options),
    }
}
