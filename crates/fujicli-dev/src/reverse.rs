use anyhow::anyhow;
use clap::Subcommand;
use fujicli::{Camera, generated::cli::SIMULATION_PROP_CODES};
use log::debug;

use crate::{
    Command,
    output::NewOutput,
    usb::{self, Location},
};

const USB_MODE_PROPERTY: u16 = 0xD16E;
const FUJI_BATTERY_INFO2_PROPERTY: u16 = 0xD36B;

#[derive(Debug, Subcommand)]
pub enum DiscoverCommand {
    /// Probe PTP identity and common informational properties
    Info,
    /// Probe schema-known simulation properties in the camera's current slot
    Simulation,
    /// Read and validate the standard Fujifilm backup object
    #[command(subcommand)]
    Backup(BackupCommand),
    /// Capture the exact D185 descriptor and payload without camera mutation
    RenderProfile { output: NewOutput },
}

#[derive(Debug, Subcommand)]
pub enum BackupCommand {
    /// Export to a new file without overwriting an existing artifact
    Export { output: NewOutput },
}

#[derive(Default)]
struct ProbeSummary {
    succeeded: usize,
    failed: usize,
}

impl ProbeSummary {
    const fn observe<T, E>(&mut self, result: &Result<T, E>) {
        if result.is_ok() {
            self.succeeded += 1;
        } else {
            self.failed += 1;
        }
    }

    fn finish(self, context: &'static str) -> anyhow::Result<()> {
        debug!(
            "Read-only {context} probes complete: {} succeeded, {} failed",
            self.succeeded, self.failed
        );
        if self.succeeded == 0 {
            return Err(anyhow!("no {context} probes succeeded"));
        }
        Ok(())
    }
}

fn open(location: Location) -> anyhow::Result<Camera> {
    let device = usb::exact_device(location)?;
    let descriptor = device.device_descriptor()?;
    eprintln!("WARNING: fujicli-dev is an unpublished reverse-engineering tool");
    eprintln!(
        "Target: USB {location}, VID:PID {:04x}:{:04x}",
        descriptor.vendor_id(),
        descriptor.product_id()
    );
    eprintln!("Mode: read-only discovery; no automatic retry");
    Camera::open_unknown(&device)
}

fn info(location: Location) -> anyhow::Result<()> {
    let mut camera = open(location)?;
    let mut probes = ProbeSummary::default();
    let result = camera.reverse_device_info();
    probes.observe(&result);
    let result = camera.reverse_device_property(USB_MODE_PROPERTY);
    probes.observe(&result);
    let result = camera.reverse_device_property(FUJI_BATTERY_INFO2_PROPERTY);
    probes.observe(&result);
    probes.finish("camera info")
}

fn simulation(location: Location) -> anyhow::Result<()> {
    let mut camera = open(location)?;
    let mut probes = ProbeSummary::default();
    for &code in SIMULATION_PROP_CODES {
        let result = camera.reverse_device_property(code);
        match &result {
            Ok(_) => debug!("Simulation property 0x{code:04X}: succeeded"),
            Err(_) => debug!("Simulation property 0x{code:04X}: unavailable"),
        }
        probes.observe(&result);
    }
    probes.finish("simulation properties")
}

fn backup(location: Location, output: &NewOutput) -> anyhow::Result<()> {
    let mut camera = open(location)?;
    let backup = camera.reverse_export_backup()?;
    output.write_all(&backup)
}

fn render_profile(location: Location, output: &NewOutput) -> anyhow::Result<()> {
    let mut camera = open(location)?;
    let discovery = camera.reverse_raw_profile_discovery()?;
    let mut json = serde_json::to_vec_pretty(&discovery)?;
    json.push(b'\n');
    output.write_all(&json)
}

pub fn handle(command: Command, location: Location) -> anyhow::Result<()> {
    match command {
        Command::Discover(DiscoverCommand::Info) => info(location),
        Command::Discover(DiscoverCommand::Simulation) => simulation(location),
        Command::Discover(DiscoverCommand::Backup(BackupCommand::Export { output })) => {
            backup(location, &output)
        }
        Command::Discover(DiscoverCommand::RenderProfile { output }) => {
            render_profile(location, &output)
        }
    }
}
