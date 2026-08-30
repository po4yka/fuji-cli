#![forbid(unsafe_code)]

pub mod features;
include!(concat!(env!("OUT_DIR"), "/generated_module.rs"));
pub mod input;
pub mod policy;
pub mod preflight;
pub mod ptp;

#[cfg(test)]
mod tests;

use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, ensure};
use features::{
    backup::{BackupArtifact, BackupIdentity, BackupPurpose, sha256_hex},
    base::{CameraBase, info::CameraInfo},
    render::{RenderOutcome, RenderedObject},
    simulation::{Simulation, SimulationTransactionError, SimulationTransactionSuccess},
};
use log::{debug, error};
use ptp::{Ptp, validate_bulk_read_geometry};
use rusb::{GlobalContext, constants::LIBUSB_CLASS_IMAGE};

#[cfg(feature = "reverse-tools")]
use crate::features::base::UNKNOWN_CAMERA;
use crate::{
    generated::{
        cameras::SUPPORTED, options::CustomSetting, renders::RenderBase,
        simulations::SimulationBase,
    },
    policy::{
        CommandRisk, EmulationAcknowledgement, LogicalCameraIdentity, ModelBindingKind,
        PhysicalUsbIdentity, SerialFingerprint, authorize,
    },
    preflight::{
        BackupRestore, RawConversion, RawRecoveryCleanup, RawRecoveryFetch, SimulationAccess,
        SimulationWrite, ValidatedCameraSession,
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
        let interface_descriptor = config_descriptor
            .interfaces()
            .flat_map(|i| i.descriptors())
            .find(|x| x.class_code() == LIBUSB_CLASS_IMAGE)
            .ok_or(rusb::Error::NotFound)?;

        let interface = interface_descriptor.interface_number();
        debug!("Found interface {interface}");

        let handle = device.open()?;
        handle.claim_interface(interface)?;
        debug!("Claimed interface");

        let find_endpoint = |direction: rusb::Direction,
                             transfer_type: rusb::TransferType|
         -> Result<(u8, usize), rusb::Error> {
            interface_descriptor
                .endpoint_descriptors()
                .find(|ep| ep.direction() == direction && ep.transfer_type() == transfer_type)
                .map(|endpoint| (endpoint.address(), usize::from(endpoint.max_packet_size())))
                .ok_or(rusb::Error::NotFound)
        };

        let (bulk_in, bulk_in_max_packet_size) =
            find_endpoint(rusb::Direction::In, rusb::TransferType::Bulk)?;
        debug!("Found Bulk In endpoint");

        let (bulk_out, _) = find_endpoint(rusb::Direction::Out, rusb::TransferType::Bulk)?;
        debug!("Found Bulk Out endpoint");

        let transaction_id = 0;
        let r#impl = (resolved.factory)();
        let chunk_size = r#impl.chunk_size();
        validate_bulk_read_geometry(chunk_size, bulk_in_max_packet_size)?;

        let mut ptp = Ptp {
            bus,
            address,
            interface,
            bulk_in,
            bulk_out,
            handle,
            transaction_id,
            chunk_size,
            poisoned: false,
            camera_processing_active: false,
            mutation_authorization: None,
        };

        ptp.open_session(SESSION)?;

        Ok(Self {
            ptp,
            r#impl,
            physical_identity,
            logical_identity: resolved.logical_identity,
            binding: resolved.binding,
            emulation_acknowledgement: resolved.acknowledgement,
            session_open: true,
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
    pub fn reverse_device_info(&mut self) -> anyhow::Result<ptp::DeviceInfo> {
        self.ptp.get_info()
    }

    #[doc(hidden)]
    #[cfg(feature = "reverse-tools")]
    pub fn reverse_device_property(&mut self, code: u16) -> anyhow::Result<Vec<u8>> {
        self.ptp.get_prop_raw(code)
    }

    #[doc(hidden)]
    #[cfg(feature = "reverse-tools")]
    pub fn reverse_export_backup_raw(&mut self) -> anyhow::Result<Vec<u8>> {
        self.ptp.send(
            ptp::CommandCode::GetObjectInfo,
            &features::backup::EXPORT_OBJECT_INFO_HANDLE,
            None,
        )?;
        self.ptp.send_for_operation(
            ptp::PtpOperation::LargeTransfer,
            ptp::CommandCode::GetObject,
            &features::backup::OBJECT_HANDLE,
            None,
        )
    }

    pub fn close(mut self) -> anyhow::Result<()> {
        ensure_session_safe_to_close(self.ptp.is_healthy())?;
        self.session_open = false;
        self.ptp.close_session(SESSION)
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
        best_effort_close_session(
            self.session_open,
            self.ptp.is_healthy(),
            Instant::now,
            |deadline| self.ptp.close_session_until(deadline),
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

type CameraFactory = fn() -> Box<dyn CameraBase<Context = GlobalContext>>;

#[derive(Debug, Clone, Copy)]
pub struct SupportedCamera {
    pub name: &'static str,
    pub vendor: u16,
    pub product: u16,
    pub ptp_identity: Option<generated::cameras::CameraPtpIdentity>,
    pub preflight_profiles: &'static [generated::cameras::CameraPreflightProfile],
    pub firmware_capability_profiles:
        &'static [generated::cameras::CameraFirmwareCapabilityProfile],
    pub camera_factory: CameraFactory,
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
        format!("{}.{}", self.ptp.bus, self.ptp.address)
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

    pub(crate) fn export_backup_unchecked(
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

    pub(crate) fn import_backup_unchecked(
        &mut self,
        artifact: &BackupArtifact,
    ) -> anyhow::Result<()> {
        if let Some(backups) = self.r#impl.as_backup_manager() {
            backups.import_backup(&mut self.ptp, artifact)
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

    pub(crate) fn get_simulation_unchecked(
        &mut self,
        slot: CustomSetting,
    ) -> anyhow::Result<Box<dyn Simulation>> {
        if let Some(sim) = self.r#impl.as_simulation_manager() {
            sim.get_simulation(&mut self.ptp, slot)
        } else {
            bail!(ERROR_CAMERA_DOES_NOT_SUPPORT_SIMULATION_MANAGEMENT);
        }
    }

    pub(crate) fn get_simulations_unchecked(
        &mut self,
        slots: &[CustomSetting],
    ) -> anyhow::Result<Vec<(CustomSetting, Box<dyn Simulation>)>> {
        if let Some(sim) = self.r#impl.as_simulation_manager() {
            sim.get_simulations(&mut self.ptp, slots)
        } else {
            bail!(ERROR_CAMERA_DOES_NOT_SUPPORT_SIMULATION_MANAGEMENT);
        }
    }

    pub(crate) fn update_simulation_unchecked(
        &mut self,
        slot: CustomSetting,
        partial: SimulationBase,
    ) -> Result<SimulationTransactionSuccess, SimulationTransactionError> {
        if let Some(sim) = self.r#impl.as_simulation_manager() {
            sim.update_simulation(&mut self.ptp, slot, partial)
        } else {
            Err(SimulationTransactionError::preparation(
                self.ptp.is_healthy(),
                anyhow!(ERROR_CAMERA_DOES_NOT_SUPPORT_SIMULATION_MANAGEMENT),
            ))
        }
    }

    pub(crate) fn set_simulation_unchecked(
        &mut self,
        slot: CustomSetting,
        simulation: &dyn Simulation,
    ) -> Result<SimulationTransactionSuccess, SimulationTransactionError> {
        if let Some(sim) = self.r#impl.as_simulation_manager() {
            sim.set_simulation(&mut self.ptp, slot, simulation)
        } else {
            Err(SimulationTransactionError::preparation(
                self.ptp.is_healthy(),
                anyhow!(ERROR_CAMERA_DOES_NOT_SUPPORT_SIMULATION_MANAGEMENT),
            ))
        }
    }

    pub(crate) fn render_unchecked(
        &mut self,
        image: &[u8],
        partial: RenderBase,
        draft: bool,
    ) -> anyhow::Result<RenderOutcome> {
        if let Some(renders) = self.r#impl.as_render_manager() {
            renders.render(&mut self.ptp, image, partial, draft)
        } else {
            bail!(ERROR_CAMERA_DOES_NOT_SUPPORT_RENDER_MANAGEMENT);
        }
    }

    pub(crate) fn recover_rendered_object_unchecked(
        &mut self,
        handle: u32,
    ) -> anyhow::Result<RenderedObject> {
        if let Some(renders) = self.r#impl.as_render_manager() {
            renders.recover_rendered_object(&mut self.ptp, handle)
        } else {
            bail!(ERROR_CAMERA_DOES_NOT_SUPPORT_RENDER_MANAGEMENT);
        }
    }

    pub(crate) fn cleanup_rendered_object_unchecked(&mut self, handle: u32) -> anyhow::Result<()> {
        if let Some(renders) = self.r#impl.as_render_manager() {
            renders.cleanup_rendered_object(&mut self.ptp, handle)
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
