#![forbid(unsafe_code)]

pub mod features;
include!(concat!(env!("OUT_DIR"), "/generated_module.rs"));
pub mod input;
pub mod policy;
pub mod ptp;

#[cfg(test)]
mod tests;

use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, ensure};
use features::{
    backup::{BackupArtifact, BackupIdentity, BackupPurpose, sha256_hex},
    base::{CameraBase, info::CameraInfo},
    simulation::Simulation,
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
        CommandRisk, CommandSpec, EmulationAcknowledgement, EmulationPolicy, ModelBindingKind,
        authorize,
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
pub struct PhysicalUsbIdentity {
    pub vendor: u16,
    pub product: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LogicalCameraIdentity {
    pub name: &'static str,
    pub vendor: u16,
    pub product: u16,
}

#[derive(Clone, Copy, Debug)]
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
    definition: &'static SupportedCamera,
    binding: ModelBindingKind,
    acknowledgement: EmulationAcknowledgement,
}

fn find_supported(identity: PhysicalUsbIdentity) -> Option<&'static SupportedCamera> {
    SUPPORTED
        .iter()
        .find(|camera| camera.vendor == identity.vendor && camera.product == identity.product)
}

#[cfg(feature = "reverse-tools")]
fn authorize_reverse_transport(binding: ModelBindingKind) -> anyhow::Result<()> {
    ensure!(
        binding == ModelBindingKind::Unknown,
        "raw reverse transport requires an unknown-camera session"
    );
    Ok(())
}

fn resolve_supported_camera(
    physical: PhysicalUsbIdentity,
    mode: CameraMode,
) -> anyhow::Result<Option<ResolvedCamera>> {
    match mode {
        #[cfg(feature = "reverse-tools")]
        CameraMode::Unknown => Ok(None),
        CameraMode::Supported => {
            let definition =
                find_supported(physical).ok_or_else(|| anyhow!(ERROR_DEVICE_NOT_SUPPORTED))?;
            Ok(Some(ResolvedCamera {
                definition,
                binding: ModelBindingKind::Native,
                acknowledgement: EmulationAcknowledgement::NotProvided,
            }))
        }
        CameraMode::Emulated {
            vendor,
            product,
            acknowledgement,
        } => {
            ensure!(
                find_supported(physical).is_some(),
                "Physical USB device {:04x}:{:04x} is not a supported camera",
                physical.vendor,
                physical.product
            );
            let definition = find_supported(PhysicalUsbIdentity { vendor, product })
                .ok_or_else(|| anyhow!(ERROR_DEVICE_NOT_SUPPORTED))?;
            Ok(Some(ResolvedCamera {
                definition,
                binding: ModelBindingKind::Emulated,
                acknowledgement,
            }))
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
            vendor: descriptor.vendor_id(),
            product: descriptor.product_id(),
        };
        let resolved = resolve_supported_camera(physical_identity, mode)?;
        let (definition, binding, emulation_acknowledgement, factory) = match resolved {
            Some(resolved) => {
                debug!("Using logical camera model: {}", resolved.definition.name);
                (
                    resolved.definition,
                    resolved.binding,
                    resolved.acknowledgement,
                    resolved.definition.camera_factory,
                )
            }
            #[cfg(feature = "reverse-tools")]
            None => (
                &UNKNOWN_CAMERA,
                ModelBindingKind::Unknown,
                EmulationAcknowledgement::NotProvided,
                UNKNOWN_CAMERA.camera_factory,
            ),
            #[cfg(not(feature = "reverse-tools"))]
            None => return Err(anyhow!("camera definition resolution failed")),
        };

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
        let r#impl = (factory)();
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
        };

        ptp.open_session(SESSION)?;

        Ok(Self {
            ptp,
            r#impl,
            physical_identity,
            logical_identity: LogicalCameraIdentity {
                name: definition.name,
                vendor: definition.vendor,
                product: definition.product,
            },
            binding,
            emulation_acknowledgement,
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

    pub fn close(mut self) -> anyhow::Result<()> {
        self.session_open = false;
        self.ptp.close_session(SESSION)
    }
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
    pub camera_factory: CameraFactory,
}

impl Camera {
    fn authorize(&self, risk: CommandRisk, emulation: EmulationPolicy) -> anyhow::Result<()> {
        authorize(
            self.binding,
            CommandSpec { risk, emulation },
            self.emulation_acknowledgement,
        )
    }

    pub fn logical_name(&self) -> &'static str {
        self.logical_identity.name
    }

    pub fn logical_vendor_id(&self) -> u16 {
        self.logical_identity.vendor
    }

    pub fn logical_product_id(&self) -> u16 {
        self.logical_identity.product
    }

    pub const fn physical_usb_identity(&self) -> PhysicalUsbIdentity {
        self.physical_identity
    }

    pub fn physical_model_name(&self) -> Option<&'static str> {
        find_supported(self.physical_identity).map(|camera| camera.name)
    }

    pub const fn logical_camera_identity(&self) -> LogicalCameraIdentity {
        self.logical_identity
    }

    pub const fn model_binding_kind(&self) -> ModelBindingKind {
        self.binding
    }

    pub fn connected_usb_id(&self) -> String {
        format!("{}.{}", self.ptp.bus, self.ptp.address)
    }

    pub fn get_info(&mut self) -> anyhow::Result<Box<dyn CameraInfo>> {
        self.authorize(CommandRisk::ReadOnly, EmulationPolicy::Allowed)?;
        self.r#impl.get_info(&mut self.ptp)
    }

    pub fn backup_identity(&mut self) -> anyhow::Result<BackupIdentity> {
        self.authorize(CommandRisk::ReadOnly, EmulationPolicy::Forbidden)?;
        let info = self.ptp.get_info()?;
        ensure_backup_identity_fields(&info)?;

        let physical_definition = find_supported(self.physical_identity)
            .ok_or_else(|| anyhow!(ERROR_DEVICE_NOT_SUPPORTED))?;

        Ok(BackupIdentity {
            camera_name: physical_definition.name.to_owned(),
            vendor_id: self.physical_identity.vendor,
            product_id: self.physical_identity.product,
            manufacturer: info.manufacturer,
            model: info.model,
            firmware: info.device_version,
            serial_sha256: sha256_hex(info.serial_number.as_bytes()),
        })
    }

    pub fn export_backup(&mut self, purpose: BackupPurpose) -> anyhow::Result<BackupArtifact> {
        self.authorize(CommandRisk::ReadOnly, EmulationPolicy::Forbidden)?;
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
        self.authorize(CommandRisk::ReadOnly, EmulationPolicy::Forbidden)?;
        let target = self.backup_identity()?;
        artifact.validate_target(&target, expected_target_serial_sha256)?;
        Ok(target)
    }

    pub fn import_backup(
        &mut self,
        artifact: &BackupArtifact,
        expected_target_serial_sha256: Option<&str>,
    ) -> anyhow::Result<()> {
        self.authorize(CommandRisk::OpaqueRestore, EmulationPolicy::Forbidden)?;
        drop(self.validate_backup(artifact, expected_target_serial_sha256)?);
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

    pub fn get_simulation(&mut self, slot: CustomSetting) -> anyhow::Result<Box<dyn Simulation>> {
        self.authorize(
            CommandRisk::TransientStateChange,
            EmulationPolicy::RequireTransientWriteAcknowledgement,
        )?;
        if let Some(sim) = self.r#impl.as_simulation_manager() {
            sim.get_simulation(&mut self.ptp, slot)
        } else {
            bail!(ERROR_CAMERA_DOES_NOT_SUPPORT_SIMULATION_MANAGEMENT);
        }
    }

    pub fn update_simulation(
        &mut self,
        slot: CustomSetting,
        partial: SimulationBase,
    ) -> anyhow::Result<()> {
        self.authorize(
            CommandRisk::PersistentSettingsWrite,
            EmulationPolicy::Forbidden,
        )?;
        if let Some(sim) = self.r#impl.as_simulation_manager() {
            sim.update_simulation(&mut self.ptp, slot, partial)
        } else {
            bail!(ERROR_CAMERA_DOES_NOT_SUPPORT_SIMULATION_MANAGEMENT);
        }
    }

    pub fn set_simulation(
        &mut self,
        slot: CustomSetting,
        simulation: &dyn Simulation,
    ) -> anyhow::Result<()> {
        self.authorize(
            CommandRisk::PersistentSettingsWrite,
            EmulationPolicy::Forbidden,
        )?;
        if let Some(sim) = self.r#impl.as_simulation_manager() {
            sim.set_simulation(&mut self.ptp, slot, simulation)
        } else {
            bail!(ERROR_CAMERA_DOES_NOT_SUPPORT_SIMULATION_MANAGEMENT);
        }
    }

    pub fn render(
        &mut self,
        image: &[u8],
        partial: RenderBase,
        draft: bool,
    ) -> anyhow::Result<Vec<u8>> {
        self.authorize(
            CommandRisk::DestructiveRecoverySensitive,
            EmulationPolicy::Forbidden,
        )?;
        if let Some(renders) = self.r#impl.as_render_manager() {
            renders.render(&mut self.ptp, image, partial, draft)
        } else {
            bail!(ERROR_CAMERA_DOES_NOT_SUPPORT_RENDER_MANAGEMENT);
        }
    }

    #[cfg(feature = "reverse-tools")]
    #[doc(hidden)]
    pub fn reverse_ptp(&mut self) -> anyhow::Result<&mut Ptp> {
        authorize_reverse_transport(self.binding)?;
        Ok(&mut self.ptp)
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
