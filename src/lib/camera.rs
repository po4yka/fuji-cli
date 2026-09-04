#![forbid(unsafe_code)]

mod authorized;
pub(crate) use authorized::AuthorizedPtp;

#[path = "preflight.rs"]
pub mod preflight;
#[path = "ptp/mod.rs"]
pub(crate) mod ptp;

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

use std::time::{Duration, Instant};

use crate::{features, generated};

use crate::features::{
    backup::{BackupArtifact, BackupIdentity, BackupPurpose, sha256_hex},
    base::{CameraBase, info::CameraInfo},
    render::{RenderOutcome, RenderedObject},
    simulation::{Simulation, SimulationTransactionError, SimulationTransactionSuccess},
};
use anyhow::{anyhow, bail, ensure};
use log::{debug, error};
use ptp::{Ptp, validate_bulk_read_geometry};
use rusb::{GlobalContext, constants::LIBUSB_CLASS_IMAGE};

#[cfg(feature = "reverse-tools")]
use crate::features::base::UNKNOWN_CAMERA;
use crate::{
    camera::preflight::{
        BackupRestore, RawConversion, RawRecoveryCleanup, RawRecoveryFetch, SimulationAccess,
        SimulationWrite, ValidatedCameraSession,
    },
    generated::{
        cameras::SUPPORTED, options::CustomSetting, renders::RenderBase,
        simulations::SimulationBase,
    },
    policy::{
        CommandRisk, EmulationAcknowledgement, LogicalCameraIdentity, ModelBindingKind,
        PhysicalUsbIdentity, SerialFingerprint, authorize,
    },
};

const ERROR_DEVICE_NOT_SUPPORTED: &str = "Device not supported";
const ERROR_CAMERA_DOES_NOT_SUPPORT_BACKUP_MANAGEMENT: &str =
    "This camera does not support backups yet";
const ERROR_CAMERA_DOES_NOT_SUPPORT_SIMULATION_PARSING: &str =
    "This camera does not support simulation parsing yet";
const ERROR_CAMERA_DOES_NOT_SUPPORT_SIMULATION_MANAGEMENT: &str =
    "This camera does not support simulation management yet";
const ERROR_CAMERA_DOES_NOT_SUPPORT_RENDER_MANAGEMENT: &str =
    "This camera does not support rendering images yet";

const SESSION: u32 = 1;
const CAMERA_DROP_CLOSE_TIMEOUT: Duration = Duration::from_secs(1);

pub struct Camera {
    ptp: Ptp,
    r#impl: Box<dyn CameraBase<Context = GlobalContext>>,
    physical_identity: PhysicalUsbIdentity,
    logical_identity: LogicalCameraIdentity,
    binding: ModelBindingKind,
    emulation_acknowledgement: EmulationAcknowledgement,
    session_open: bool,
    session_control_permit: Option<ptp::SessionControlPermit>,
}

/// See [`Camera::reverse_probe_device_identity`]. Carries the raw serial
/// number; the caller MUST hash it before display, logging, or persistence.
#[doc(hidden)]
#[cfg(feature = "dangerous-reverse-engineering")]
pub struct ProbeDeviceIdentity {
    pub manufacturer: String,
    pub model: String,
    pub firmware: String,
    pub serial_number: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CameraMode {
    Supported,
    Emulated {
        vendor: u16,
        product: u16,
        acknowledgement: EmulationAcknowledgement,
    },
    #[cfg(feature = "reverse-tools")]
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PtpUsbCandidate {
    interface: u8,
    setting: u8,
    bulk_in: Vec<(u8, usize)>,
    bulk_out: Vec<(u8, usize)>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PtpUsbBinding {
    interface: u8,
    setting: u8,
    bulk_in: u8,
    bulk_out: u8,
    bulk_in_max_packet_size: usize,
    bulk_out_max_packet_size: usize,
}

fn select_ptp_usb_binding(
    candidates: impl IntoIterator<Item = PtpUsbCandidate>,
) -> anyhow::Result<PtpUsbBinding> {
    let mut binding = None;
    for candidate in candidates {
        if candidate.bulk_in.is_empty() || candidate.bulk_out.is_empty() {
            continue;
        }
        ensure!(
            candidate.bulk_in.len() == 1 && candidate.bulk_out.len() == 1,
            "PTP USB interface alternate setting has ambiguous bulk endpoints"
        );
        let (bulk_in, bulk_in_max_packet_size) = candidate.bulk_in[0];
        let (bulk_out, bulk_out_max_packet_size) = candidate.bulk_out[0];
        let candidate_binding = PtpUsbBinding {
            interface: candidate.interface,
            setting: candidate.setting,
            bulk_in,
            bulk_out,
            bulk_in_max_packet_size,
            bulk_out_max_packet_size,
        };
        ensure!(
            binding.replace(candidate_binding).is_none(),
            "multiple complete PTP USB interface alternate settings found"
        );
    }
    binding.ok_or_else(|| anyhow!("no complete PTP USB interface alternate setting found"))
}

#[derive(Clone, Copy)]
struct ResolvedCamera {
    factory: CameraFactory,
    logical_identity: LogicalCameraIdentity,
    binding: ModelBindingKind,
    acknowledgement: EmulationAcknowledgement,
}

fn supported_camera(identity: PhysicalUsbIdentity) -> Option<&'static SupportedCamera> {
    SUPPORTED
        .iter()
        .find(|camera| camera.vendor == identity.vendor_id && camera.product == identity.product_id)
}

fn resolve_camera(
    mode: CameraMode,
    physical_identity: PhysicalUsbIdentity,
) -> anyhow::Result<ResolvedCamera> {
    let native = supported_camera(physical_identity);

    match mode {
        CameraMode::Supported => {
            let camera = native.ok_or_else(|| anyhow!(ERROR_DEVICE_NOT_SUPPORTED))?;
            Ok(ResolvedCamera {
                factory: camera.camera_factory,
                logical_identity: LogicalCameraIdentity {
                    vendor_id: camera.vendor,
                    product_id: camera.product,
                },
                binding: ModelBindingKind::Native,
                acknowledgement: EmulationAcknowledgement::NotProvided,
            })
        }
        CameraMode::Emulated {
            vendor,
            product,
            acknowledgement,
        } => {
            anyhow::ensure!(
                native.is_some(),
                "--emulate requires a physically connected supported camera"
            );
            let camera = supported_camera(PhysicalUsbIdentity {
                vendor_id: vendor,
                product_id: product,
            })
            .ok_or_else(|| anyhow!(ERROR_DEVICE_NOT_SUPPORTED))?;
            Ok(ResolvedCamera {
                factory: camera.camera_factory,
                logical_identity: LogicalCameraIdentity {
                    vendor_id: camera.vendor,
                    product_id: camera.product,
                },
                binding: ModelBindingKind::Emulated,
                acknowledgement,
            })
        }
        #[cfg(feature = "reverse-tools")]
        CameraMode::Unknown => Ok(ResolvedCamera {
            factory: UNKNOWN_CAMERA.camera_factory,
            logical_identity: LogicalCameraIdentity {
                vendor_id: physical_identity.vendor_id,
                product_id: physical_identity.product_id,
            },
            binding: ModelBindingKind::Unknown,
            acknowledgement: EmulationAcknowledgement::NotProvided,
        }),
    }
}

/// Owns a claimed USB interface until it is either dropped (releasing the
/// interface) or handed off to a longer-lived owner via [`into_handle`].
///
/// [`into_handle`]: ClaimedInterface::into_handle
struct ClaimedInterface {
    handle: Option<rusb::DeviceHandle<GlobalContext>>,
    interface: u8,
}

impl ClaimedInterface {
    /// Claims `interface` on `handle`, returning a guard that releases it on
    /// drop unless ownership is transferred out via [`into_handle`].
    ///
    /// [`into_handle`]: ClaimedInterface::into_handle
    fn claim(handle: rusb::DeviceHandle<GlobalContext>, interface: u8) -> anyhow::Result<Self> {
        handle
            .claim_interface(interface)
            .map_err(|error| claim_failure(error, interface, cfg!(target_os = "macos")))?;
        Ok(Self {
            handle: Some(handle),
            interface,
        })
    }

    /// Borrows the claimed handle for further setup (e.g.
    /// `set_alternate_setting`) while the guard still owns it.
    fn handle(&self) -> anyhow::Result<&rusb::DeviceHandle<GlobalContext>> {
        self.handle
            .as_ref()
            .ok_or_else(|| anyhow!("claimed USB interface handle is unavailable"))
    }

    /// Transfers ownership of the claimed handle to the caller. The guard's
    /// `Drop` becomes a no-op afterward, so the interface is released exactly
    /// once: either here (never, since the handle moves out) or by whatever
    /// later takes ownership of it (e.g. `Ptp`'s own `Drop`).
    fn into_handle(mut self) -> anyhow::Result<rusb::DeviceHandle<GlobalContext>> {
        self.handle
            .take()
            .ok_or_else(|| anyhow!("claimed USB interface handle is unavailable"))
    }
}

impl Drop for ClaimedInterface {
    fn drop(&mut self) {
        if let Some(handle) = &self.handle
            && let Err(error) = handle.release_interface(self.interface)
        {
            error!("Failed to release USB interface after open failure: {error}");
        }
    }
}

impl Camera {
    pub fn probe(device: &rusb::Device<GlobalContext>) -> anyhow::Result<bool> {
        let descriptor = device.device_descriptor()?;

        let vendor = descriptor.vendor_id();
        let product = descriptor.product_id();

        let supported = SUPPORTED
            .iter()
            .any(|c| c.vendor == vendor && c.product == product);

        Ok(supported)
    }

    pub fn open_with(
        mode: CameraMode,
        device: &rusb::Device<GlobalContext>,
    ) -> anyhow::Result<Self> {
        let descriptor = device.device_descriptor()?;

        let physical_identity = PhysicalUsbIdentity {
            vendor_id: descriptor.vendor_id(),
            product_id: descriptor.product_id(),
        };
        let resolved = resolve_camera(mode, physical_identity)?;
        if let Some(camera) = supported_camera(PhysicalUsbIdentity {
            vendor_id: resolved.logical_identity.vendor_id,
            product_id: resolved.logical_identity.product_id,
        }) {
            debug!("Selected logical camera model: {}", camera.name);
        }

        let bus = device.bus_number();
        let address = device.address();

        let config_descriptor = device.active_config_descriptor()?;
        let binding = select_ptp_usb_binding(
            config_descriptor
                .interfaces()
                .flat_map(|i| i.descriptors())
                .filter(|descriptor| descriptor.class_code() == LIBUSB_CLASS_IMAGE)
                .map(|descriptor| {
                    let mut bulk_in = Vec::new();
                    let mut bulk_out = Vec::new();
                    for endpoint in descriptor
                        .endpoint_descriptors()
                        .filter(|endpoint| endpoint.transfer_type() == rusb::TransferType::Bulk)
                    {
                        match endpoint.direction() {
                            rusb::Direction::In => bulk_in.push((
                                endpoint.address(),
                                usize::from(endpoint.max_packet_size()),
                            )),
                            rusb::Direction::Out => bulk_out.push((
                                endpoint.address(),
                                usize::from(endpoint.max_packet_size()),
                            )),
                        }
                    }
                    PtpUsbCandidate {
                        interface: descriptor.interface_number(),
                        setting: descriptor.setting_number(),
                        bulk_in,
                        bulk_out,
                    }
                }),
        )?;
        debug!(
            "Found PTP interface {} alternate setting {}",
            binding.interface, binding.setting
        );

        let r#impl = (resolved.factory)();
        let speed = device.speed();
        let chunk_policy = ptp::ChunkPolicy::for_transport(
            r#impl.chunk_size_ceiling(),
            speed,
            binding.bulk_in_max_packet_size,
            binding.bulk_out_max_packet_size,
        )?;
        validate_bulk_read_geometry(
            chunk_policy.read.initial_bytes,
            binding.bulk_in_max_packet_size,
        )?;
        validate_bulk_read_geometry(
            chunk_policy.read.ceiling_bytes,
            binding.bulk_in_max_packet_size,
        )?;
        let bulk_read_state = ptp::BulkReadState::new(chunk_policy.read.initial_bytes)?;
        debug!(
            "PTP USB transport policy: os={}, libusb={:?}, speed={speed:?}, interface={}, alternate_setting={}, bulk_in=0x{:02x}, bulk_in_packet_bytes={}, bulk_out=0x{:02x}, bulk_out_packet_bytes={}, read_initial_bytes={}, read_ceiling_bytes={}, write_initial_bytes={}, write_ceiling_bytes={}, source=conservative",
            std::env::consts::OS,
            rusb::version(),
            binding.interface,
            binding.setting,
            binding.bulk_in,
            binding.bulk_in_max_packet_size,
            binding.bulk_out,
            binding.bulk_out_max_packet_size,
            chunk_policy.read.initial_bytes,
            chunk_policy.read.ceiling_bytes,
            chunk_policy.write.initial_bytes,
            chunk_policy.write.ceiling_bytes,
        );

        let handle = device.open()?;
        let claimed = ClaimedInterface::claim(handle, binding.interface)?;
        debug!("Claimed interface");
        claimed
            .handle()?
            .set_alternate_setting(binding.interface, binding.setting)?;
        debug!("Activated alternate setting {}", binding.setting);

        let mut ptp = Ptp::new(
            (bus, address, binding.interface),
            binding.bulk_in,
            binding.bulk_out,
            claimed.into_handle()?,
            chunk_policy,
            bulk_read_state,
        );

        let session_control_permit = ptp.open_session(SESSION)?;

        Ok(Self {
            ptp,
            r#impl,
            physical_identity,
            logical_identity: resolved.logical_identity,
            binding: resolved.binding,
            emulation_acknowledgement: resolved.acknowledgement,
            session_open: true,
            session_control_permit: Some(session_control_permit),
        })
    }

    pub fn open(device: &rusb::Device<GlobalContext>) -> anyhow::Result<Self> {
        Self::open_with(CameraMode::Supported, device)
    }

    pub fn open_as(
        device: &rusb::Device<GlobalContext>,
        vendor: u16,
        product: u16,
        acknowledgement: EmulationAcknowledgement,
    ) -> anyhow::Result<Self> {
        Self::open_with(
            CameraMode::Emulated {
                vendor,
                product,
                acknowledgement,
            },
            device,
        )
    }

    #[cfg(feature = "reverse-tools")]
    pub fn open_unknown(device: &rusb::Device<GlobalContext>) -> anyhow::Result<Self> {
        Self::open_with(CameraMode::Unknown, device)
    }

    #[doc(hidden)]
    #[cfg(feature = "reverse-tools")]
    pub fn reverse_device_info(&mut self) -> anyhow::Result<()> {
        self.ptp.get_info().map(|_| ())
    }

    #[doc(hidden)]
    #[cfg(feature = "reverse-tools")]
    pub fn reverse_device_property(&mut self, code: u16) -> anyhow::Result<Vec<u8>> {
        self.ptp.get_prop_raw(code)
    }

    /// Probe-only single-property write. See
    /// [`crate::ptp::Ptp::probe_write_single_property_unverified`]; sanctioned for
    /// the 0xD18C namespace probe, compiled only under
    /// `dangerous-reverse-engineering`.
    #[doc(hidden)]
    #[cfg(feature = "dangerous-reverse-engineering")]
    pub fn reverse_probe_write_single_property(
        &mut self,
        prop: u16,
        value: &[u8],
    ) -> anyhow::Result<Vec<u8>> {
        self.ptp.probe_write_single_property_unverified(prop, value)
    }

    /// Probe-only device identity, including the raw serial number. The
    /// caller MUST hash the serial (see `crate::features::backup::sha256_hex`)
    /// before ever displaying, logging, or persisting it; nothing in this
    /// crate does that hashing on the caller's behalf. Compiled only under
    /// `dangerous-reverse-engineering` because it is the only path that
    /// exposes the raw serial outside the crate — the read-only
    /// `reverse_device_info`/`discover info` surface deliberately never
    /// returns it. Read-only: issues only `GetDeviceInfo`.
    #[doc(hidden)]
    #[cfg(feature = "dangerous-reverse-engineering")]
    pub fn reverse_probe_device_identity(&mut self) -> anyhow::Result<ProbeDeviceIdentity> {
        let info = self.ptp.get_info()?;
        Ok(ProbeDeviceIdentity {
            manufacturer: info.manufacturer,
            model: info.model,
            firmware: info.device_version,
            serial_number: info.serial_number,
        })
    }

    #[doc(hidden)]
    #[cfg(feature = "reverse-tools")]
    pub fn reverse_raw_profile_discovery(
        &mut self,
    ) -> anyhow::Result<crate::reverse::RawProfileDiscovery> {
        const USB_MODE_PROPERTY: u16 = 0xD16E;
        const RAW_PROFILE_PROPERTY: u16 = 0xD185;

        let info = self.ptp.get_info()?;
        let descriptor = self.ptp.get_prop_desc_raw(RAW_PROFILE_PROPERTY).ok();
        let payload = self.ptp.get_prop_raw(RAW_PROFILE_PROPERTY)?;
        let usb_mode = self
            .ptp
            .get_prop::<u16>(USB_MODE_PROPERTY)
            .ok()
            .map(u32::from);
        let descriptor_summary = descriptor
            .as_deref()
            .and_then(|raw| ptp::DevicePropDesc::decode(raw).ok())
            .filter(|parsed| parsed.property_code == RAW_PROFILE_PROPERTY)
            .map(|parsed| {
                (
                    parsed.data_type.name().to_owned(),
                    parsed.writable,
                    parsed.form.name().to_owned(),
                )
            });

        crate::reverse::RawProfileDiscovery::from_observation(
            info.manufacturer,
            info.model,
            info.device_version,
            usb_mode,
            descriptor.as_deref(),
            descriptor_summary,
            &payload,
        )
    }

    /// Reads the camera's entire advertised PTP surface without changing a
    /// single byte of its state: `GetDeviceInfo`, then `GetDevicePropDesc` and
    /// `GetDevicePropValue` for every property the device advertises. Failures
    /// are recorded per property instead of aborting the survey, because a
    /// camera refusing a descriptor or a value is itself the finding (the X-T5
    /// on 4.31 refuses every descriptor in USB mode 0x6).
    ///
    /// Payload bytes never enter the result; see
    /// [`crate::reverse::PropertyObservation`].
    #[doc(hidden)]
    #[cfg(feature = "reverse-tools")]
    pub fn reverse_property_survey(&mut self) -> anyhow::Result<crate::reverse::PropertySurvey> {
        const USB_MODE_PROPERTY: u16 = 0xD16E;

        let info = self.ptp.get_info()?;
        let usb_mode = self
            .ptp
            .get_prop::<u16>(USB_MODE_PROPERTY)
            .ok()
            .map(u32::from);
        let declared = declared_property_data_types(&info.manufacturer, &info.model);
        let mut summary = crate::reverse::PropertySurveySummary {
            advertised: info.device_properties_supported.len(),
            ..Default::default()
        };
        let mut properties = Vec::with_capacity(info.device_properties_supported.len());

        for &code in &info.device_properties_supported {
            let descriptor = self
                .ptp
                .get_prop_desc_raw(code)
                .ok()
                .and_then(|bytes| ptp::DevicePropDesc::decode(&bytes).ok());
            if descriptor.is_some() {
                summary.descriptors_read += 1;
            }
            let value = self.ptp.get_prop_raw(code).ok();
            if value.is_some() {
                summary.values_read += 1;
            }
            let declared_data_type = declared.as_ref().and_then(|map| map.get(&code).copied());
            if declared_data_type.is_some() {
                summary.declared += 1;
            }
            let declaration_matches = declared_data_type.and_then(|data_type| {
                value
                    .as_ref()
                    .map(|bytes| crate::reverse::value_matches_data_type(bytes, data_type))
            });
            if declaration_matches == Some(false) {
                summary.declaration_mismatches += 1;
            }

            properties.push(crate::reverse::PropertyObservation {
                code,
                descriptor_available: descriptor.is_some(),
                descriptor_data_type: descriptor.as_ref().map(|desc| desc.data_type.name()),
                descriptor_writable: descriptor.as_ref().map(|desc| desc.writable),
                descriptor_form: descriptor.as_ref().map(|desc| desc.form.name()),
                value_available: value.is_some(),
                value_length: value.as_ref().map(Vec::len),
                value_shape: value.as_deref().map(crate::reverse::classify_value_shape),
                value_sha256: value.as_deref().map(crate::features::backup::sha256_hex),
                declared_data_type,
                declaration_matches,
            });
        }

        Ok(crate::reverse::PropertySurvey {
            schema_version: 1,
            declared_camera: declared_camera_name(&info.manufacturer, &info.model),
            manufacturer: info.manufacturer,
            model: info.model,
            firmware: info.device_version,
            usb_mode,
            operations_supported: info.operations_supported,
            events_supported: info.events_supported,
            capture_formats: info.capture_formats,
            image_formats: info.image_formats,
            properties,
            summary,
        })
    }

    #[doc(hidden)]
    #[cfg(feature = "reverse-tools")]
    pub fn reverse_export_backup(&mut self) -> anyhow::Result<Vec<u8>> {
        features::backup::manager::export_backup_with_transport(&mut self.ptp)
    }

    pub fn close(mut self) -> anyhow::Result<()> {
        ensure_session_safe_to_close(self.ptp.is_healthy())?;
        self.session_open = false;
        let permit = self
            .session_control_permit
            .take()
            .ok_or_else(|| anyhow!("PTP session-control permit is unavailable"))?;
        self.ptp.close_session(&permit)
    }
}

fn ensure_session_safe_to_close(is_healthy: bool) -> anyhow::Result<()> {
    ensure!(
        is_healthy,
        "refusing CloseSession because the PTP stream or camera processing state is unsafe"
    );
    Ok(())
}

impl Drop for Camera {
    fn drop(&mut self) {
        let Some(permit) = self.session_control_permit.as_ref() else {
            return;
        };
        best_effort_close_session(
            self.session_open,
            self.ptp.is_healthy(),
            Instant::now,
            |deadline| self.ptp.close_session_until(permit, deadline),
        );
    }
}

fn best_effort_close_session<N, F>(session_open: bool, is_healthy: bool, now: N, close_session: F)
where
    N: FnOnce() -> Instant,
    F: FnOnce(Instant) -> anyhow::Result<()>,
{
    if !session_open {
        return;
    }

    if !is_healthy {
        debug!("Skipping CloseSession because the PTP stream may be desynchronized");
        return;
    }

    let Some(deadline) = now().checked_add(CAMERA_DROP_CLOSE_TIMEOUT) else {
        error!("Cannot establish CloseSession deadline while dropping camera");
        return;
    };

    if let Err(error) = close_session(deadline) {
        error!("Error closing session: {error}");
    }
}

/// Wraps a failed `claim_interface` with the most likely cause. The typed
/// `rusb::Error` stays in the chain for outcome classification.
fn claim_failure(error: rusb::Error, interface: u8, macos: bool) -> anyhow::Error {
    let hint = match error {
        rusb::Error::Access | rusb::Error::Busy if macos => {
            "; another process holds the camera, on macOS usually Image Capture's ptpcamerad: run `pkill -x ptpcamerad` and retry"
        }
        rusb::Error::Access | rusb::Error::Busy => {
            "; another process holds the camera or the udev rules deny access, see the installation notes"
        }
        _ => "",
    };
    anyhow::Error::new(error).context(format!("claiming USB interface {interface} failed{hint}"))
}

type CameraFactory = fn() -> Box<dyn CameraBase<Context = GlobalContext>>;

/// Registry entry whose exact PTP identity matches what the device reports.
#[cfg(feature = "reverse-tools")]
fn declared_camera(manufacturer: &str, model: &str) -> Option<&'static SupportedCamera> {
    SUPPORTED.iter().find(|camera| {
        camera.ptp_identity.is_some_and(|identity| {
            identity.manufacturer == manufacturer && identity.model == model
        })
    })
}

#[cfg(feature = "reverse-tools")]
fn declared_camera_name(manufacturer: &str, model: &str) -> Option<&'static str> {
    declared_camera(manufacturer, model).map(|camera| camera.name)
}

/// PTP datatypes the camera's preflight profiles pin, keyed by property code.
/// A code declared with different datatypes by different profiles is left out:
/// the survey reports what FML asserts unambiguously, nothing more.
#[cfg(feature = "reverse-tools")]
fn declared_property_data_types(
    manufacturer: &str,
    model: &str,
) -> Option<std::collections::BTreeMap<u16, u16>> {
    use generated::cameras::CameraPreflightDataType;

    let camera = declared_camera(manufacturer, model)?;
    use std::collections::{BTreeMap, BTreeSet};

    let mut declared: BTreeMap<u16, u16> = BTreeMap::new();
    let mut ambiguous: BTreeSet<u16> = BTreeSet::new();
    for profile in camera.preflight_profiles {
        for property in profile.required_properties {
            let CameraPreflightDataType::Exact(data_type) = property.data_type else {
                continue;
            };
            match declared.insert(property.code, data_type) {
                Some(previous) if previous != data_type => {
                    ambiguous.insert(property.code);
                }
                _ => {}
            }
        }
    }
    for code in ambiguous {
        declared.remove(&code);
    }
    Some(declared)
}

#[derive(Debug, Clone, Copy)]
pub struct SupportedCamera {
    pub name: &'static str,
    pub vendor: u16,
    pub product: u16,
    pub ptp_identity: Option<generated::cameras::CameraPtpIdentity>,
    pub preflight_profiles: &'static [generated::cameras::CameraPreflightProfile],
    pub firmware_capability_profiles:
        &'static [generated::cameras::CameraFirmwareCapabilityProfile],
    pub(crate) camera_factory: CameraFactory,
}

impl Camera {
    pub fn name(&self) -> &'static str {
        self.r#impl.camera_definition().name
    }

    pub const fn vendor_id(&self) -> u16 {
        self.physical_identity.vendor_id
    }

    pub const fn product_id(&self) -> u16 {
        self.physical_identity.product_id
    }

    pub const fn physical_usb_identity(&self) -> PhysicalUsbIdentity {
        self.physical_identity
    }

    pub const fn logical_camera_identity(&self) -> LogicalCameraIdentity {
        self.logical_identity
    }

    pub fn connected_usb_id(&self) -> String {
        self.ptp.usb_id()
    }

    pub fn get_info(&mut self) -> anyhow::Result<Box<dyn CameraInfo>> {
        self.authorize(CommandRisk::ReadOnly)?;
        self.r#impl.get_info(&mut self.ptp)
    }

    pub fn preflight_backup_restore(
        &mut self,
        serial_binding: &SerialFingerprint,
    ) -> anyhow::Result<ValidatedCameraSession<'_, BackupRestore>> {
        preflight::run(self, Some(serial_binding))
    }

    pub fn preflight_simulation_access(
        &mut self,
    ) -> anyhow::Result<ValidatedCameraSession<'_, SimulationAccess>> {
        preflight::run(self, None)
    }

    pub fn preflight_simulation_write(
        &mut self,
        serial_binding: &SerialFingerprint,
    ) -> anyhow::Result<ValidatedCameraSession<'_, SimulationWrite>> {
        preflight::run(self, Some(serial_binding))
    }

    pub fn preflight_raw_conversion(
        &mut self,
        serial_binding: &SerialFingerprint,
    ) -> anyhow::Result<ValidatedCameraSession<'_, RawConversion>> {
        preflight::run(self, Some(serial_binding))
    }

    pub fn preflight_raw_recovery_fetch(
        &mut self,
        serial_binding: &SerialFingerprint,
    ) -> anyhow::Result<ValidatedCameraSession<'_, RawRecoveryFetch>> {
        preflight::run(self, Some(serial_binding))
    }

    pub fn preflight_raw_recovery_cleanup(
        &mut self,
        serial_binding: &SerialFingerprint,
    ) -> anyhow::Result<ValidatedCameraSession<'_, RawRecoveryCleanup>> {
        preflight::run(self, Some(serial_binding))
    }

    pub fn backup_identity(&mut self) -> anyhow::Result<BackupIdentity> {
        self.authorize(CommandRisk::EmulationForbidden)?;
        let info = self.ptp.get_info()?;
        ensure_backup_identity_fields(&info)?;

        let physical_camera = supported_camera(self.physical_identity)
            .ok_or_else(|| anyhow!(ERROR_DEVICE_NOT_SUPPORTED))?;

        Ok(BackupIdentity {
            camera_name: physical_camera.name.to_owned(),
            vendor_id: self.physical_identity.vendor_id,
            product_id: self.physical_identity.product_id,
            manufacturer: info.manufacturer,
            model: info.model,
            firmware: info.device_version,
            serial_sha256: sha256_hex(info.serial_number.as_bytes()),
        })
    }

    pub fn export_backup(&mut self, purpose: BackupPurpose) -> anyhow::Result<BackupArtifact> {
        self.export_backup_unchecked(purpose)
    }

    fn export_backup_unchecked(
        &mut self,
        purpose: BackupPurpose,
    ) -> anyhow::Result<BackupArtifact> {
        let identity = self.backup_identity()?;
        if let Some(backups) = self.r#impl.as_backup_manager() {
            let payload = backups.export_backup(&mut self.ptp)?;
            BackupArtifact::create(purpose, identity, &payload)
        } else {
            bail!(ERROR_CAMERA_DOES_NOT_SUPPORT_BACKUP_MANAGEMENT);
        }
    }

    pub fn validate_backup(
        &mut self,
        artifact: &BackupArtifact,
        expected_target_serial_sha256: Option<&str>,
    ) -> anyhow::Result<BackupIdentity> {
        let target = self.backup_identity()?;
        artifact.validate_target(&target, expected_target_serial_sha256)?;
        Ok(target)
    }

    fn import_backup_unchecked(
        &mut self,
        permit: &mut preflight::MutationPermit,
        artifact: &BackupArtifact,
    ) -> anyhow::Result<features::backup::BackupRestoreAccepted> {
        if let Some(backups) = self.r#impl.as_backup_manager() {
            let authorized = AuthorizedPtp::new(&mut self.ptp, permit, authorized::BACKUP_RESTORE)?;
            let mut transport =
                features::backup::manager::AuthorizedBackupTransport::new(authorized);
            backups.import_backup(&mut transport, artifact)
        } else {
            bail!(ERROR_CAMERA_DOES_NOT_SUPPORT_BACKUP_MANAGEMENT);
        }
    }

    pub fn serialize_simulation(&self, simulation: &dyn Simulation) -> anyhow::Result<Vec<u8>> {
        if let Some(simulations) = self.r#impl.as_simulation_parser() {
            simulations.serialize_simulation(simulation)
        } else {
            bail!(ERROR_CAMERA_DOES_NOT_SUPPORT_SIMULATION_PARSING);
        }
    }

    pub fn deserialize_simulation(&self, simulation: &[u8]) -> anyhow::Result<Box<dyn Simulation>> {
        if let Some(simulations) = self.r#impl.as_simulation_parser() {
            simulations.deserialize_simulation(simulation)
        } else {
            bail!(ERROR_CAMERA_DOES_NOT_SUPPORT_SIMULATION_PARSING);
        }
    }

    pub fn custom_settings_slots(&self) -> anyhow::Result<Vec<CustomSetting>> {
        if let Some(sim) = self.r#impl.as_simulation_manager() {
            Ok(sim.custom_settings_slots())
        } else {
            bail!(ERROR_CAMERA_DOES_NOT_SUPPORT_SIMULATION_MANAGEMENT);
        }
    }

    fn get_simulation_unchecked(
        &mut self,
        permit: &mut preflight::MutationPermit,
        slot: CustomSetting,
    ) -> anyhow::Result<Box<dyn Simulation>> {
        if let Some(sim) = self.r#impl.as_simulation_manager() {
            let authorized =
                AuthorizedPtp::new(&mut self.ptp, permit, authorized::SIMULATION_READ)?;
            let mut io = features::simulation::AuthorizedSimulationIo::new(authorized);
            sim.get_simulation(&mut io, slot)
        } else {
            bail!(ERROR_CAMERA_DOES_NOT_SUPPORT_SIMULATION_MANAGEMENT);
        }
    }

    fn get_simulations_unchecked(
        &mut self,
        permit: &mut preflight::MutationPermit,
        slots: &[CustomSetting],
    ) -> anyhow::Result<Vec<(CustomSetting, Box<dyn Simulation>)>> {
        if let Some(sim) = self.r#impl.as_simulation_manager() {
            let authorized =
                AuthorizedPtp::new(&mut self.ptp, permit, authorized::SIMULATION_READ)?;
            let mut io = features::simulation::AuthorizedSimulationIo::new(authorized);
            sim.get_simulations(&mut io, slots)
        } else {
            bail!(ERROR_CAMERA_DOES_NOT_SUPPORT_SIMULATION_MANAGEMENT);
        }
    }

    fn update_simulation_unchecked(
        &mut self,
        permit: &mut preflight::MutationPermit,
        slot: CustomSetting,
        partial: SimulationBase,
    ) -> Result<SimulationTransactionSuccess, SimulationTransactionError> {
        if let Some(sim) = self.r#impl.as_simulation_manager() {
            let healthy = self.ptp.is_healthy();
            let authorized =
                AuthorizedPtp::new(&mut self.ptp, permit, authorized::SIMULATION_WRITE)
                    .map_err(|error| SimulationTransactionError::preparation(healthy, error))?;
            let mut io = features::simulation::AuthorizedSimulationIo::new(authorized);
            sim.update_simulation(&mut io, slot, partial)
        } else {
            Err(SimulationTransactionError::preparation(
                self.ptp.is_healthy(),
                anyhow!(ERROR_CAMERA_DOES_NOT_SUPPORT_SIMULATION_MANAGEMENT),
            ))
        }
    }

    fn set_simulation_unchecked(
        &mut self,
        permit: &mut preflight::MutationPermit,
        slot: CustomSetting,
        simulation: &dyn Simulation,
    ) -> Result<SimulationTransactionSuccess, SimulationTransactionError> {
        if let Some(sim) = self.r#impl.as_simulation_manager() {
            let healthy = self.ptp.is_healthy();
            let authorized =
                AuthorizedPtp::new(&mut self.ptp, permit, authorized::SIMULATION_WRITE)
                    .map_err(|error| SimulationTransactionError::preparation(healthy, error))?;
            let mut io = features::simulation::AuthorizedSimulationIo::new(authorized);
            sim.set_simulation(&mut io, slot, simulation)
        } else {
            Err(SimulationTransactionError::preparation(
                self.ptp.is_healthy(),
                anyhow!(ERROR_CAMERA_DOES_NOT_SUPPORT_SIMULATION_MANAGEMENT),
            ))
        }
    }

    fn render_unchecked(
        &mut self,
        permit: &mut preflight::MutationPermit,
        image: &[u8],
        partial: RenderBase,
        draft: bool,
    ) -> anyhow::Result<RenderOutcome> {
        if let Some(renders) = self.r#impl.as_render_manager() {
            let authorized = AuthorizedPtp::new(&mut self.ptp, permit, authorized::RENDER)?;
            let mut io = features::render::manager::AuthorizedRenderIo::new(authorized);
            renders.render(&mut io, image, partial, draft)
        } else {
            bail!(ERROR_CAMERA_DOES_NOT_SUPPORT_RENDER_MANAGEMENT);
        }
    }

    fn recover_rendered_object_unchecked(
        &mut self,
        permit: &mut preflight::MutationPermit,
        handle: u32,
    ) -> anyhow::Result<RenderedObject> {
        if let Some(renders) = self.r#impl.as_render_manager() {
            let authorized =
                AuthorizedPtp::new(&mut self.ptp, permit, authorized::RENDER_RECOVERY_FETCH)?;
            let mut io = features::render::manager::AuthorizedRenderIo::new(authorized);
            renders.recover_rendered_object(&mut io, handle)
        } else {
            bail!(ERROR_CAMERA_DOES_NOT_SUPPORT_RENDER_MANAGEMENT);
        }
    }

    fn cleanup_rendered_object_unchecked(
        &mut self,
        permit: &mut preflight::MutationPermit,
        handle: u32,
    ) -> anyhow::Result<features::outcome::StateChangeAudit> {
        if let Some(renders) = self.r#impl.as_render_manager() {
            let authorized = AuthorizedPtp::new(&mut self.ptp, permit, authorized::RENDER_CLEANUP)?;
            let mut io = features::render::manager::AuthorizedRenderIo::new(authorized);
            renders.cleanup_rendered_object(&mut io, handle)
        } else {
            bail!(ERROR_CAMERA_DOES_NOT_SUPPORT_RENDER_MANAGEMENT);
        }
    }

    fn authorize(&self, risk: CommandRisk) -> anyhow::Result<()> {
        authorize(self.binding, risk, self.emulation_acknowledgement)
    }
}

fn ensure_backup_identity_fields(info: &ptp::DeviceInfo) -> anyhow::Result<()> {
    ensure!(
        !info.manufacturer.is_empty(),
        "camera manufacturer is empty"
    );
    ensure!(!info.model.is_empty(), "camera model is empty");
    ensure!(!info.device_version.is_empty(), "camera firmware is empty");
    ensure!(
        !info.serial_number.is_empty(),
        "camera serial number is empty"
    );
    Ok(())
}

#[cfg(test)]
mod claim_tests {
    use super::claim_failure;

    #[test]
    fn access_denied_names_ptpcamerad_on_macos_and_keeps_the_usb_error() {
        let error = claim_failure(rusb::Error::Access, 0, true);

        assert!(error.to_string().contains("ptpcamerad"), "{error}");
        assert!(error.downcast_ref::<rusb::Error>().is_some());

        let linux = claim_failure(rusb::Error::Access, 0, false);
        assert!(linux.to_string().contains("udev"), "{linux}");

        let other = claim_failure(rusb::Error::NoDevice, 0, true);
        assert!(!other.to_string().contains("ptpcamerad"), "{other}");
    }
}
