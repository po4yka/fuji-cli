use std::fmt;

use erased_serde::serialize_trait_object;
use serde::Serialize;

use crate::{Camera, generated::options::UsbMode};

pub trait CameraInfo: fmt::Display + erased_serde::Serialize {}
serialize_trait_object!(CameraInfo);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DefaultCameraInfo {
    pub manufacturer: String,
    pub model: String,
    pub device_version: String,
    pub serial_sha256: String,
    pub mode: UsbMode,
    pub battery: u32,
}

impl fmt::Display for DefaultCameraInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Manufacturer: {}", self.manufacturer.escape_debug())?;
        writeln!(f, "Model: {}", self.model.escape_debug())?;
        writeln!(f, "Version: {}", self.device_version.escape_debug())?;
        writeln!(f, "Serial SHA-256: {}", self.serial_sha256)?;
        writeln!(f, "Mode: {}", self.mode)?;
        write!(f, "Battery: {}%", self.battery)
    }
}

impl CameraInfo for DefaultCameraInfo {}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CameraInfoListItem {
    pub name: &'static str,
    pub usb_id: String,
    pub vendor_id: String,
    pub product_id: String,
}

impl From<&Camera> for CameraInfoListItem {
    fn from(camera: &Camera) -> Self {
        Self {
            name: camera.name(),
            usb_id: camera.connected_usb_id(),
            vendor_id: format!("0x{:04x}", camera.vendor_id()),
            product_id: format!("0x{:04x}", camera.product_id()),
        }
    }
}

impl fmt::Display for CameraInfoListItem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} ({}:{}) (USB ID: {})",
            self.name, self.vendor_id, self.product_id, self.usb_id
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::generated::options::UsbMode;

    use super::DefaultCameraInfo;

    #[test]
    fn device_info_exposes_serial_fingerprint_instead_of_raw_serial() {
        let info = DefaultCameraInfo {
            manufacturer: "FUJIFILM".to_owned(),
            model: "X-T5".to_owned(),
            device_version: "4.31".to_owned(),
            serial_sha256: "0".repeat(64),
            mode: UsbMode::RawConversion,
            battery: 100,
        };

        let display = info.to_string();
        let json = serde_json::to_value(&info).unwrap();

        assert!(display.contains("Serial SHA-256: "));
        assert!(!display.contains("Serial Number"));
        assert_eq!(json["serialSha256"], "0".repeat(64));
        assert!(json.get("serialNumber").is_none());
    }

    #[test]
    fn device_info_human_output_escapes_terminal_controls() {
        let info = DefaultCameraInfo {
            manufacturer: "FUJI\u{1b}]0;pwned\u{7}".to_owned(),
            model: "X-T5\rspoofed".to_owned(),
            device_version: "4.31\ninjected".to_owned(),
            serial_sha256: "0".repeat(64),
            mode: UsbMode::RawConversion,
            battery: 100,
        };

        let display = info.to_string();

        assert_eq!(
            display,
            concat!(
                "Manufacturer: FUJI\\u{1b}]0;pwned\\u{7}\n",
                "Model: X-T5\\rspoofed\n",
                "Version: 4.31\\ninjected\n",
                "Serial SHA-256: ",
                "0000000000000000000000000000000000000000000000000000000000000000\n",
                "Mode: Raw Conversion\n",
                "Battery: 100%",
            )
        );
    }
}
