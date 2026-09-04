use std::str::FromStr;

use anyhow::{Context, anyhow, bail};
use clap::Subcommand;
use fujicli::{
    Camera,
    generated::{cli::SIMULATION_PROP_CODES, options::prop_codes},
    reverse::{FUJI_BATTERY_INFO2_PROPERTY, PropertySurvey},
};
use log::debug;

use crate::{
    output::NewOutput,
    usb::{self, Location},
};

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
    /// Read one operator-supplied PTP property code, read-only
    Property {
        /// Property selector as hex, with an optional 0x/0X prefix, e.g.
        /// 0xD18D or d18d. A digit-only value such as 5005 is read as hex,
        /// not decimal; there is no decimal parsing path
        code: PropertyCode,
        /// Print the property payload; may include a custom-setting slot
        /// name or an identity string
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
    /// the camera advertises. The artifact never records payload bytes: every
    /// value gets a length and a wire shape, and only a string-valued
    /// property also gets a SHA-256 digest, so it is shareable without a
    /// privacy review.
    Surface { output: NewOutput },
}

#[derive(Debug, Subcommand)]
pub enum BackupCommand {
    /// Export to a new file without overwriting an existing artifact
    Export { output: NewOutput },
}

/// An operator-supplied PTP property selector, parsed as hex. Accepts an
/// optional `0x`/`0X` prefix and 1 to 4 hex digits (`u16` covers the full
/// range a PTP property code can take). There is deliberately no decimal
/// parsing path: every PTP property code in this codebase and its
/// documentation is written in hex, so a digit-only value such as `5005`
/// parses as hex `0x5005`, not decimal 5005; supplying `0x` makes the base
/// explicit but is never required.
#[derive(Debug, Clone, Copy)]
pub struct PropertyCode(u16);

impl FromStr for PropertyCode {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let digits = value
            .strip_prefix("0x")
            .or_else(|| value.strip_prefix("0X"))
            .unwrap_or(value);
        if digits.is_empty()
            || digits.len() > 4
            || !digits.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            bail!(
                "invalid property code {value:?}; expected 1 to 4 hex digits, with an optional 0x/0X prefix, e.g. 0xD18D"
            );
        }
        let code = u16::from_str_radix(digits, 16)
            .map_err(|_| anyhow!("invalid property code {value:?}"))?;
        Ok(Self(code))
    }
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

/// Decodes a raw property payload shaped like a PTP string: one length byte
/// giving the UTF-16 code-unit count (terminator included), that many
/// little-endian UTF-16 code units, with the last one required to be the NUL
/// terminator. Returns `None` if `bytes` does not have exactly that shape --
/// including an all-terminator payload with no text, which this heuristic
/// declines to call a string rather than risk misreading a plain integer
/// pair. Shared so every raw-payload caller in this crate decodes a PTP
/// string the same way instead of reimplementing this shape check.
pub fn decode_ptp_string(bytes: &[u8]) -> Option<String> {
    let [count, rest @ ..] = bytes else {
        return None;
    };
    if *count == 0 || rest.len() != usize::from(*count) * 2 {
        return None;
    }
    let (pairs, _) = rest.as_chunks::<2>();
    let units: Vec<u16> = pairs.iter().map(|&pair| u16::from_le_bytes(pair)).collect();
    match units.split_last() {
        Some((0, text)) if !text.is_empty() => String::from_utf16(text).ok(),
        _ => None,
    }
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
        _ => decode_ptp_string(bytes).map(|text| format!("string {text:?}")),
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
    probe_property(&mut camera, &mut probes, prop_codes::USB_MODE, print_values);
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
    probe_property(
        &mut camera,
        &mut probes,
        prop_codes::CUSTOM_SETTING,
        print_values,
    );
    for &code in SIMULATION_PROP_CODES {
        probe_property(&mut camera, &mut probes, code, print_values);
    }
    probes.finish("simulation properties")
}

fn property(location: Location, code: PropertyCode, print_values: bool) -> anyhow::Result<()> {
    let mut camera = open(location)?;
    let mut probes = ProbeSummary::default();
    probe_property(&mut camera, &mut probes, code.0, print_values);
    probes.finish("property")
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
        DiscoverCommand::Property { code, print_values } => property(location, code, print_values),
        DiscoverCommand::Backup(BackupCommand::Export { output }) => backup(location, &output),
        DiscoverCommand::RenderProfile { output } => render_profile(location, &output),
        DiscoverCommand::Surface { output } => surface(location, &output),
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr as _;

    use super::{PropertyCode, describe_value};

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

    #[test]
    fn property_code_accepts_an_uppercase_hex_value_with_prefix() {
        let code = PropertyCode::from_str("0xD18D").expect("valid hex code");
        assert_eq!(code.0, 0xD18D);
    }

    #[test]
    fn property_code_accepts_a_lowercase_hex_value_without_prefix() {
        let code = PropertyCode::from_str("d18d").expect("valid hex code");
        assert_eq!(code.0, 0xD18D);
    }

    #[test]
    fn property_code_accepts_a_short_hex_value_with_prefix() {
        let code = PropertyCode::from_str("0x5005").expect("valid hex code");
        assert_eq!(code.0, 0x5005);
    }

    #[test]
    fn property_code_treats_a_digit_only_value_as_hex_not_decimal() {
        // There is no decimal parsing path: a digit-only value such as
        // "5005" parses as hex 0x5005, not decimal 5005. Documented and
        // pinned here so the behavior cannot drift silently.
        let code = PropertyCode::from_str("5005").expect("digit-only value parses as hex");
        assert_eq!(code.0, 0x5005);
    }

    #[test]
    fn property_code_rejects_an_empty_value() {
        PropertyCode::from_str("").expect_err("empty value must be rejected");
    }

    #[test]
    fn property_code_rejects_a_bare_prefix() {
        PropertyCode::from_str("0x").expect_err("prefix alone, with no digits, must be rejected");
    }

    #[test]
    fn property_code_rejects_more_than_four_hex_digits() {
        let error = PropertyCode::from_str("53645")
            .expect_err("five digits cannot fit in a u16 property code");
        assert!(
            error.to_string().contains("invalid property code"),
            "{error}"
        );
    }

    #[test]
    fn property_code_rejects_non_hex_characters() {
        PropertyCode::from_str("zz").expect_err("non-hex characters must be rejected");
    }
}
