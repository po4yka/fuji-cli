use anyhow::{Context, anyhow};
use clap::Subcommand;
use fujicli::{Camera, generated::cli::SIMULATION_PROP_CODES, reverse::PropertySurvey};
use log::debug;

use crate::{
    output::NewOutput,
    usb::{self, Location},
};

const USB_MODE_PROPERTY: u16 = 0xD16E;
const FUJI_BATTERY_INFO2_PROPERTY: u16 = 0xD36B;

#[derive(Debug, Subcommand)]
pub enum DiscoverCommand {
    /// Probe PTP identity and common informational properties
    Info {
        /// Print each property payload (length, hex, decoded scalar/string).
        /// Never prints the serial number; review the output before sharing.
        #[arg(long)]
        print_values: bool,
    },
    /// Probe schema-known simulation properties in the camera's current slot
    Simulation {
        /// Print each property payload; includes the custom setting name text
        #[arg(long)]
        print_values: bool,
    },
    /// Read and validate the standard Fujifilm backup object
    #[command(subcommand)]
    Backup(BackupCommand),
    /// Capture the exact D185 descriptor and payload without camera mutation
    RenderProfile { output: NewOutput },
    /// Survey the whole advertised PTP surface and compare it with FML
    ///
    /// Reads `GetDeviceInfo`, then the descriptor and value of every property
    /// the camera advertises. The artifact records shapes and digests, never
    /// payload bytes, so it is shareable without a privacy review.
    Surface { output: NewOutput },
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

/// Human-readable, privacy-neutral rendering of a raw property payload:
/// length, hex, and a decoded little-endian scalar or PTP string when the
/// bytes have exactly that shape.
fn describe_value(code: u16, bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    let hex = bytes.iter().fold(String::new(), |mut hex, byte| {
        let _ = write!(hex, "{byte:02x}");
        hex
    });
    let decoded = match bytes {
        [low, high] => Some(format!("uint16 {}", u16::from_le_bytes([*low, *high]))),
        [a, b, c, d] => Some(format!("uint32 {}", u32::from_le_bytes([*a, *b, *c, *d]))),
        [count, rest @ ..] if rest.len() == usize::from(*count) * 2 && *count > 0 => {
            let (pairs, _) = rest.as_chunks::<2>();
            let units: Vec<u16> = pairs.iter().map(|&pair| u16::from_le_bytes(pair)).collect();
            match units.split_last() {
                Some((0, text)) if !text.is_empty() => String::from_utf16(text)
                    .ok()
                    .map(|text| format!("string {text:?}")),
                _ => None,
            }
        }
        _ => None,
    };
    let len = bytes.len();
    decoded.map_or_else(
        || format!("0x{code:04X}: {len} bytes {hex}"),
        |decoded| format!("0x{code:04X}: {len} bytes {hex} ({decoded})"),
    )
}

fn probe_property(camera: &mut Camera, probes: &mut ProbeSummary, code: u16, print_values: bool) {
    let result = camera.reverse_device_property(code);
    match &result {
        Ok(bytes) => {
            debug!("Property 0x{code:04X}: succeeded");
            if print_values {
                println!("{}", describe_value(code, bytes));
            }
        }
        Err(error) => debug!("Property 0x{code:04X}: unavailable ({error:#})"),
    }
    probes.observe(&result);
}

fn info(location: Location, print_values: bool) -> anyhow::Result<()> {
    let mut camera = open(location)?;
    let mut probes = ProbeSummary::default();
    let result = camera.reverse_device_info();
    probes.observe(&result);
    probe_property(&mut camera, &mut probes, USB_MODE_PROPERTY, print_values);
    probe_property(
        &mut camera,
        &mut probes,
        FUJI_BATTERY_INFO2_PROPERTY,
        print_values,
    );
    probes.finish("camera info")
}

fn simulation(location: Location, print_values: bool) -> anyhow::Result<()> {
    let mut camera = open(location)?;
    let mut probes = ProbeSummary::default();
    for &code in SIMULATION_PROP_CODES {
        probe_property(&mut camera, &mut probes, code, print_values);
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
    let discovery = camera.reverse_raw_profile_discovery().context(
        "capturing the D185 conversion profile; the X-T5 (firmware 4.31, USB mode 0x6) answers GeneralError to GetDevicePropDesc and GetDevicePropValue for D185 unless a RAF is loaded for conversion, which this read-only probe never does - use the vendor USB capture procedure in docs/contributors/reversing.md",
    )?;
    let mut json = serde_json::to_vec_pretty(&discovery)?;
    json.push(b'\n');
    output.write_all(&json)
}

/// One line per advertised property, so a device run is readable without
/// opening the artifact: what the camera served, and whether the value shape
/// agrees with the datatype FML pins for this model.
fn report_surface(survey: &PropertySurvey) {
    println!(
        "{} {} firmware {} (USB mode {})",
        survey.manufacturer,
        survey.model,
        survey.firmware,
        survey
            .usb_mode
            .map_or_else(|| "unknown".to_owned(), |mode| format!("0x{mode:x}"))
    );
    println!(
        "Advertised: {} operations, {} events, {} properties, {} image formats",
        survey.operations_supported.len(),
        survey.events_supported.len(),
        survey.summary.advertised,
        survey.image_formats.len()
    );
    for property in &survey.properties {
        let descriptor = if property.descriptor_available {
            property.descriptor_data_type.unwrap_or("unknown")
        } else {
            "refused"
        };
        let value = match (property.value_shape, property.value_length) {
            (Some(shape), Some(length)) => format!("{shape} ({length} bytes)"),
            _ => "refused".to_owned(),
        };
        let verdict = match (property.declared_data_type, property.declaration_matches) {
            (Some(declared), Some(true)) => format!(", matches pinned 0x{declared:04X}"),
            (Some(declared), Some(false)) => {
                format!(", CONTRADICTS pinned 0x{declared:04X}")
            }
            (Some(declared), None) => format!(", pinned 0x{declared:04X} unchecked"),
            (None, _) => String::new(),
        };
        println!(
            "0x{:04X}: descriptor {descriptor}, value {value}{verdict}",
            property.code
        );
    }
    println!(
        "Descriptors served {}/{}, values served {}/{}, FML pins checked {}, contradictions {}",
        survey.summary.descriptors_read,
        survey.summary.advertised,
        survey.summary.values_read,
        survey.summary.advertised,
        survey.summary.declared,
        survey.summary.declaration_mismatches
    );
    if survey.declared_camera.is_none() {
        println!("No registry entry matches this PTP identity; nothing was cross-checked");
    }
}

fn surface(location: Location, output: &NewOutput) -> anyhow::Result<()> {
    let mut camera = open(location)?;
    let survey = camera
        .reverse_property_survey()
        .context("surveying the advertised PTP property surface")?;
    report_surface(&survey);
    let mut json = serde_json::to_vec_pretty(&survey)?;
    json.push(b'\n');
    output.write_all(&json)
}

pub fn handle(command: DiscoverCommand, location: Location) -> anyhow::Result<()> {
    match command {
        DiscoverCommand::Info { print_values } => info(location, print_values),
        DiscoverCommand::Simulation { print_values } => simulation(location, print_values),
        DiscoverCommand::Backup(BackupCommand::Export { output }) => backup(location, &output),
        DiscoverCommand::RenderProfile { output } => render_profile(location, &output),
        DiscoverCommand::Surface { output } => surface(location, &output),
    }
}

#[cfg(test)]
mod tests {
    use super::describe_value;

    #[test]
    fn describe_value_decodes_scalars_and_ptp_strings_and_keeps_raw_hex() {
        assert_eq!(
            describe_value(0xD16E, &[0x06, 0x00]),
            "0xD16E: 2 bytes 0600 (uint16 6)"
        );
        assert_eq!(
            describe_value(0xD36A, &[0x0b, 0, 0, 0]),
            "0xD36A: 4 bytes 0b000000 (uint32 11)"
        );
        let battery = [0x03, b'6', 0, b'5', 0, 0, 0];
        assert_eq!(
            describe_value(0xD36B, &battery),
            "0xD36B: 7 bytes 03360035000000 (string \"65\")"
        );
        assert_eq!(describe_value(0xD20B, &[1, 0, 0]), "0xD20B: 3 bytes 010000");
    }
}
