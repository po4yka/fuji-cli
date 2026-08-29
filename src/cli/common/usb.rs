use std::{
    fmt::{Display, Formatter},
    str::FromStr,
};

use anyhow::{anyhow, bail};
use fujicli::Camera;
use log::trace;

#[derive(Default)]
struct UsbScanSummary {
    scanned: usize,
    matched: usize,
    probe_errors: usize,
    open_errors: usize,
}

impl Display for UsbScanSummary {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "scanned={}, matched={}", self.scanned, self.matched)?;
        if self.probe_errors > 0 || self.open_errors > 0 {
            write!(
                f,
                ", probe_errors={}, open_errors={}",
                self.probe_errors, self.open_errors
            )?;
        }
        Ok(())
    }
}

fn probe_candidates<T, E>(
    devices: impl IntoIterator<Item = T>,
    mut probe: impl FnMut(&T) -> Result<bool, E>,
) -> (Vec<T>, UsbScanSummary) {
    let mut candidates = Vec::new();
    let mut summary = UsbScanSummary::default();
    for device in devices {
        summary.scanned += 1;
        match probe(&device) {
            Ok(true) => {
                summary.matched += 1;
                candidates.push(device);
            }
            Ok(false) => {}
            Err(_) => summary.probe_errors += 1,
        }
    }
    (candidates, summary)
}

#[derive(Debug, Clone, Copy)]
pub struct Location {
    pub bus: u8,
    pub address: u8,
}

impl FromStr for Location {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (bus, address) = s
            .split_once('.')
            .ok_or_else(|| anyhow!("Invalid device format: {s}, expected <BUS>.<ADDRESS>"))?;

        Ok(Self {
            bus: bus
                .parse()
                .map_err(|_| anyhow!("Invalid bus number: {bus}"))?,
            address: address
                .parse()
                .map_err(|_| anyhow!("Invalid address: {address}"))?,
        })
    }
}

impl Display for Location {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.bus, self.address)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Identity {
    pub vendor: u16,
    pub product: u16,
}

impl FromStr for Identity {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (vendor, product) = s.split_once(':').ok_or_else(|| {
            anyhow!("Invalid model format: {s}, expected <VENDOR_ID>:<PRODUCT_ID>")
        })?;

        Ok(Self {
            vendor: u16::from_str_radix(vendor, 16)
                .map_err(|_| anyhow!("Invalid vendor ID: {vendor}"))?,
            product: u16::from_str_radix(product, 16)
                .map_err(|_| anyhow!("Invalid product ID: {product}"))?,
        })
    }
}

impl Display for Identity {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:04x}:{:04x}", self.vendor, self.product)
    }
}

pub fn get_usb_device_by_location(
    location: Location,
) -> anyhow::Result<rusb::Device<rusb::GlobalContext>> {
    let mut summary = UsbScanSummary::default();
    for device in rusb::devices()?.iter() {
        summary.scanned += 1;
        let bus = device.bus_number();
        let address = device.address();

        if bus != location.bus || address != location.address {
            continue;
        }

        summary.matched = 1;
        trace!("USB location lookup complete: {summary}");
        return Ok(device);
    }

    trace!("USB location lookup complete: {summary}");
    bail!("No USB device found at location {location}");
}

pub fn get_all_cameras() -> anyhow::Result<Vec<Camera>> {
    let mut cameras = Vec::new();
    let (devices, mut summary) = probe_candidates(rusb::devices()?.iter(), Camera::probe);

    for device in devices {
        match Camera::open(&device) {
            Ok(camera) => cameras.push(camera),
            Err(_) => summary.open_errors += 1,
        }
    }

    trace!("USB camera scan complete: {summary}");
    if cameras.is_empty() && (summary.probe_errors > 0 || summary.open_errors > 0) {
        bail!("No supported camera found ({summary})");
    }
    Ok(cameras)
}

fn select_only<T>(candidates: Vec<T>) -> anyhow::Result<T> {
    let mut candidates = candidates.into_iter();

    match (candidates.next(), candidates.next()) {
        (None, _) => bail!("No supported camera found"),
        (Some(camera), None) => Ok(camera),
        (Some(_), Some(_)) => {
            bail!("Multiple supported cameras found; specify one with --device <BUS>.<ADDRESS>")
        }
    }
}

pub fn get_camera(device: Option<Location>, emulate: Option<Identity>) -> anyhow::Result<Camera> {
    if let Some(location) = device {
        let device = get_usb_device_by_location(location)?;

        emulate.as_ref().map_or_else(
            || Camera::open(&device),
            |identity| Camera::open_as(&device, identity.vendor, identity.product),
        )
    } else {
        let (cameras, summary) = probe_candidates(rusb::devices()?.iter(), Camera::probe);

        trace!("USB camera scan complete: {summary}");
        let device = select_only(cameras).map_err(|error| {
            if summary.probe_errors > 0 {
                anyhow!("{error} ({summary})")
            } else {
                error
            }
        })?;
        emulate.as_ref().map_or_else(
            || Camera::open(&device),
            |identity| Camera::open_as(&device, identity.vendor, identity.product),
        )
    }
}

#[cfg(test)]
mod tests {
    use std::io;

    use super::{UsbScanSummary, probe_candidates, select_only};

    #[test]
    fn scan_continues_after_one_device_probe_fails() {
        let (candidates, summary) = probe_candidates([1_u8, 2, 3], |device| match device {
            1 => Err(io::Error::new(io::ErrorKind::NotFound, "disconnected")),
            2 => Ok(false),
            3 => Ok(true),
            _ => unreachable!(),
        });

        assert_eq!(candidates, vec![3]);
        assert_eq!(summary.probe_errors, 1);
    }

    #[test]
    fn usb_scan_summary_contains_only_aggregate_counts() {
        let summary = UsbScanSummary {
            scanned: 7,
            matched: 2,
            probe_errors: 0,
            open_errors: 0,
        };

        assert_eq!(summary.to_string(), "scanned=7, matched=2");
    }

    #[test]
    fn selection_rejects_multiple_supported_cameras() {
        let error = select_only(vec!["first", "second"])
            .expect_err("an ambiguous selection must require --device");

        assert_eq!(
            error.to_string(),
            "Multiple supported cameras found; specify one with --device <BUS>.<ADDRESS>"
        );
    }
}
