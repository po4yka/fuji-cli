use std::{
    fmt::{Display, Formatter},
    str::FromStr,
    thread::sleep,
    time::{Duration, Instant},
};

use anyhow::{Context as _, anyhow, bail};
use fujicli::{
    Camera,
    features::base::info::CameraInfoListItem,
    policy::{EmulationAcknowledgement, SerialFingerprint},
};
use log::trace;

#[derive(Default)]
struct UsbScanSummary {
    scanned: usize,
    matched: usize,
    probe_errors: usize,
}

impl Display for UsbScanSummary {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "scanned={}, matched={}", self.scanned, self.matched)?;
        if self.probe_errors > 0 {
            write!(f, ", probe_errors={}", self.probe_errors)?;
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
struct UsbIdentity {
    bus: u8,
    address: u8,
    vendor: u16,
    product: u16,
}

fn discover_camera_info<T, E>(
    devices: impl IntoIterator<Item = T>,
    mut inspect: impl FnMut(&T) -> Result<UsbIdentity, E>,
) -> (Vec<CameraInfoListItem>, UsbScanSummary) {
    let mut cameras = Vec::new();
    let mut summary = UsbScanSummary::default();

    for device in devices {
        summary.scanned += 1;
        let Ok(identity) = inspect(&device) else {
            summary.probe_errors += 1;
            continue;
        };

        let Some(camera) = fujicli::generated::cameras::SUPPORTED
            .iter()
            .find(|camera| camera.vendor == identity.vendor && camera.product == identity.product)
        else {
            continue;
        };

        summary.matched += 1;
        cameras.push(CameraInfoListItem {
            name: camera.name,
            usb_id: format!("{}.{}", identity.bus, identity.address),
            vendor_id: format!("0x{:04x}", identity.vendor),
            product_id: format!("0x{:04x}", identity.product),
        });
    }

    (cameras, summary)
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

pub fn get_all_cameras() -> anyhow::Result<Vec<CameraInfoListItem>> {
    let (cameras, summary) = discover_camera_info(rusb::devices()?.iter(), |device| {
        let descriptor = device.device_descriptor()?;
        Ok::<_, rusb::Error>(UsbIdentity {
            bus: device.bus_number(),
            address: device.address(),
            vendor: descriptor.vendor_id(),
            product: descriptor.product_id(),
        })
    });

    trace!("USB camera scan complete: {summary}");
    if cameras.is_empty() && summary.probe_errors > 0 {
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

pub fn get_camera(
    device: Option<Location>,
    emulate: Option<Identity>,
    acknowledgement: EmulationAcknowledgement,
) -> anyhow::Result<Camera> {
    if let Some(location) = device {
        let device = get_usb_device_by_location(location)?;

        emulate.as_ref().map_or_else(
            || Camera::open(&device),
            |identity| Camera::open_as(&device, identity.vendor, identity.product, acknowledgement),
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
            |identity| Camera::open_as(&device, identity.vendor, identity.product, acknowledgement),
        )
    }
}

pub fn reconnect_camera_by_serial(
    serial: &SerialFingerprint,
    timeout: Duration,
) -> anyhow::Result<Camera> {
    retry_reconnect(
        || find_camera_by_serial(serial),
        sleep,
        Instant::now,
        timeout,
    )
}

fn retry_reconnect<T>(
    mut attempt: impl FnMut() -> anyhow::Result<Option<T>>,
    mut sleep_between_attempts: impl FnMut(Duration),
    mut now: impl FnMut() -> Instant,
    timeout: Duration,
) -> anyhow::Result<T> {
    let deadline = now()
        .checked_add(timeout)
        .ok_or_else(|| anyhow!("camera reconnect deadline overflow"))?;
    let mut last_error = None;
    loop {
        match attempt() {
            Ok(Some(camera)) => return Ok(camera),
            Ok(None) => {}
            Err(error) => last_error = Some(error),
        }

        let remaining = deadline.saturating_duration_since(now());
        if remaining.is_zero() {
            return last_error.map_or_else(
                || {
                    Err(anyhow!(
                        "camera did not reconnect before the verification deadline"
                    ))
                },
                |error| {
                    Err(error).context("camera did not reconnect before the verification deadline")
                },
            );
        }
        sleep_between_attempts(remaining.min(Duration::from_millis(250)));
    }
}

fn find_camera_by_serial(serial: &SerialFingerprint) -> anyhow::Result<Option<Camera>> {
    let (candidates, summary) = probe_candidates(rusb::devices()?.iter(), Camera::probe);
    trace!("USB camera reconnect scan complete: {summary}");

    for device in candidates {
        let Ok(mut camera) = Camera::open(&device) else {
            continue;
        };
        let Ok(identity) = camera.backup_identity() else {
            continue;
        };
        if identity.serial_sha256 == serial.as_str() {
            return Ok(Some(camera));
        }
    }

    Ok(None)
}

#[cfg(test)]
mod tests {
    use std::{
        cell::{Cell, RefCell},
        collections::VecDeque,
        io,
        time::{Duration, Instant},
    };

    use super::{
        UsbIdentity, UsbScanSummary, discover_camera_info, probe_candidates, retry_reconnect,
        select_only,
    };

    #[test]
    fn reconnect_retries_by_logical_serial_and_accepts_a_new_usb_address() {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        struct EnumeratedDevice {
            serial: &'static str,
            bus: u8,
            address: u8,
        }

        let target_serial = "CAMERA-0001";
        let enumerations = RefCell::new(VecDeque::from([
            vec![EnumeratedDevice {
                serial: "STALE-DEVICE",
                bus: 1,
                address: 4,
            }],
            vec![EnumeratedDevice {
                serial: target_serial,
                bus: 1,
                address: 9,
            }],
        ]));
        let attempts = Cell::new(0_u8);
        let sleeps = Cell::new(0_u8);
        let clock = Cell::new(Instant::now());

        let reconnected = retry_reconnect(
            || {
                attempts.set(attempts.get() + 1);
                Ok(enumerations
                    .borrow_mut()
                    .pop_front()
                    .expect("test enumeration")
                    .into_iter()
                    .find(|device| device.serial == target_serial))
            },
            |duration| {
                sleeps.set(sleeps.get() + 1);
                clock.set(clock.get() + duration);
            },
            || clock.get(),
            Duration::from_secs(1),
        )
        .expect("the target camera must be found after re-enumeration");

        assert_eq!(attempts.get(), 2);
        assert_eq!(sleeps.get(), 1);
        assert_eq!(reconnected.serial, target_serial);
        assert_eq!((reconnected.bus, reconnected.address), (1, 9));
        assert_ne!((reconnected.bus, reconnected.address), (1, 4));
    }

    #[test]
    fn discovery_lists_supported_descriptors_without_opening_a_camera() {
        let descriptor_reads = Cell::new(0);
        let (cameras, summary) = discover_camera_info([()], |()| {
            descriptor_reads.set(descriptor_reads.get() + 1);
            Ok::<_, io::Error>(UsbIdentity {
                bus: 1,
                address: 4,
                vendor: 0x04cb,
                product: 0x02fc,
            })
        });

        assert_eq!(descriptor_reads.get(), 1);
        assert_eq!(summary.matched, 1);
        assert_eq!(cameras.len(), 1);
        assert_eq!(cameras[0].name, "FUJIFILM X-T5");
        assert_eq!(cameras[0].usb_id, "1.4");
    }

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
