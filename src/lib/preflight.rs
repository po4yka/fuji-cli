use std::{
    collections::{BTreeMap, BTreeSet},
    marker::PhantomData,
};

use anyhow::{bail, ensure};

use crate::{
    Camera, SupportedCamera,
    features::{
        backup::BackupIdentity,
        simulation::{Simulation, SimulationTransactionError, SimulationTransactionSuccess},
    },
    generated::{
        cameras::{
            CameraFirmwareCapabilityProfile, CameraPreflightDataType, CameraPreflightOperation,
            CameraPreflightProfile, CameraPreflightProfileStatus,
        },
        options::CustomSetting,
        renders::RenderBase,
        simulations::SimulationBase,
    },
    policy::{ModelBindingKind, PhysicalUsbIdentity, SerialFingerprint},
    ptp::{DevicePropDesc, Ptp, codec::PtpString},
};

#[derive(Debug)]
pub struct BackupRestore;

#[derive(Debug)]
pub struct RawConversion;

#[derive(Debug)]
pub struct RawRecoveryFetch;

#[derive(Debug)]
pub struct RawRecoveryCleanup;

#[derive(Debug)]
pub struct SimulationAccess;

#[derive(Debug)]
pub struct SimulationWrite;

#[derive(Debug)]
pub(super) struct MutationPermit {
    authorization: MutationAuthorization,
    id: u64,
    bus: u8,
    address: u8,
    interface: u8,
    operation: CameraPreflightOperation,
}

#[derive(Debug)]
struct MutationAuthorization {
    operations: BTreeSet<u16>,
    properties: BTreeMap<u16, DevicePropDesc>,
    capability_profile: &'static CameraFirmwareCapabilityProfile,
    raw_conversion_read_fingerprint_validated: bool,
    raw_conversion_profile_validated: bool,
}

impl MutationAuthorization {
    fn new(
        operations: &[u16],
        properties: Vec<DevicePropDesc>,
        capability_profile: &'static CameraFirmwareCapabilityProfile,
    ) -> Self {
        Self {
            operations: operations.iter().copied().collect(),
            properties: properties
                .into_iter()
                .map(|descriptor| (descriptor.property_code, descriptor))
                .collect(),
            capability_profile,
            raw_conversion_read_fingerprint_validated: false,
            raw_conversion_profile_validated: false,
        }
    }

    fn validate(
        &self,
        code: crate::ptp::CommandCode,
        params: &[u32],
        data: Option<&[u8]>,
    ) -> anyhow::Result<()> {
        let operation = u16::from(code);
        ensure!(
            self.operations.contains(&operation),
            "PTP mutation 0x{operation:04x} is not authorized by the validated preflight profile"
        );
        if matches!(
            code,
            crate::ptp::CommandCode::FujiSendObjectInfo | crate::ptp::CommandCode::FujiSendObject
        ) {
            ensure!(
                self.raw_conversion_profile_validated,
                "RAW image upload requires a validated firmware conversion profile"
            );
        }
        if code == crate::ptp::CommandCode::SetDevicePropValue {
            let property = params
                .first()
                .and_then(|value| u16::try_from(*value).ok())
                .ok_or_else(|| {
                    anyhow::anyhow!("SetDevicePropValue requires one u16 property code")
                })?;
            self.validate_property_candidate(
                property,
                data.ok_or_else(|| anyhow::anyhow!("SetDevicePropValue requires serialized data"))?,
            )?;
        }
        Ok(())
    }

    fn validate_property_candidate(&self, property: u16, data: &[u8]) -> anyhow::Result<()> {
        let descriptor = self.properties.get(&property).ok_or_else(|| {
            anyhow::anyhow!("PTP property 0x{property:04x} was not validated by preflight")
        })?;
        descriptor.validate_serialized_candidate(data)
    }
}

impl MutationPermit {
    fn new(
        id: u64,
        transport_binding: (u8, u8, u8),
        operation: CameraPreflightOperation,
        operations: &[u16],
        properties: Vec<DevicePropDesc>,
        capability_profile: &'static CameraFirmwareCapabilityProfile,
    ) -> Self {
        Self {
            authorization: MutationAuthorization::new(operations, properties, capability_profile),
            id,
            bus: transport_binding.0,
            address: transport_binding.1,
            interface: transport_binding.2,
            operation,
        }
    }

    pub(crate) fn is_active_for(
        &self,
        bus: u8,
        address: u8,
        interface: u8,
        active_id: Option<u64>,
    ) -> bool {
        active_id == Some(self.id)
            && self.bus == bus
            && self.address == address
            && self.interface == interface
    }

    pub(crate) const fn operation(&self) -> CameraPreflightOperation {
        self.operation
    }

    pub(crate) fn validate_mutation(
        &self,
        code: crate::ptp::CommandCode,
        params: &[u32],
        data: Option<&[u8]>,
    ) -> anyhow::Result<()> {
        self.authorization.validate(code, params, data)
    }

    pub(crate) fn firmware_option_write_value(
        &self,
        option: &str,
        logical_value: &str,
    ) -> anyhow::Result<i32> {
        self.authorization
            .capability_profile
            .write_wire_value(option, logical_value)
    }

    pub(crate) const fn firmware_capability_profile(
        &self,
    ) -> &'static CameraFirmwareCapabilityProfile {
        self.authorization.capability_profile
    }

    pub(crate) fn firmware_option_read_logical_value(
        &self,
        option: &str,
        wire_value: i32,
    ) -> anyhow::Result<&'static str> {
        self.authorization
            .capability_profile
            .read_logical_value(option, wire_value)
    }

    pub(crate) fn validate_raw_conversion_profile(
        &mut self,
        profile_code: u32,
        header_padding: usize,
        fields: &[&str],
        bytes: &[u8],
    ) -> anyhow::Result<()> {
        self.authorization
            .capability_profile
            .validate_raw_conversion_signature(profile_code, header_padding, fields, bytes.len())?;
        ensure!(
            self.authorization.raw_conversion_read_fingerprint_validated,
            "RAW conversion write requires a matching live read fingerprint"
        );
        self.authorization.validate_property_candidate(
            u16::from(crate::ptp::DevicePropCode::FujiRawConversionProfile),
            bytes,
        )?;
        self.authorization.raw_conversion_profile_validated = true;
        Ok(())
    }

    pub(crate) fn validate_raw_conversion_read_fingerprint(
        &mut self,
        profile_code: u32,
        header_padding: usize,
        declared_field_count: u16,
        fields: &[&str],
        bytes: &[u8],
    ) -> anyhow::Result<()> {
        self.authorization
            .capability_profile
            .validate_raw_conversion_read_fingerprint(
                profile_code,
                header_padding,
                declared_field_count,
                fields,
                bytes.len(),
            )?;
        crate::ptp::validate_raw_conversion_live_envelope(
            bytes,
            profile_code,
            header_padding,
            declared_field_count,
            fields.len(),
        )?;
        self.authorization.raw_conversion_read_fingerprint_validated = true;
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreflightEvidence {
    pub camera_name: &'static str,
    pub physical_vendor_id: u16,
    pub physical_product_id: u16,
    pub manufacturer: String,
    pub model: String,
    pub firmware: String,
    pub serial_sha256: String,
    pub usb_mode: u32,
    pub battery_percent: u8,
}

pub struct ValidatedCameraSession<'camera, Operation> {
    pub(crate) camera: &'camera mut Camera,
    permit: MutationPermit,
    evidence: PreflightEvidence,
    capability_profile: &'static CameraFirmwareCapabilityProfile,
    operation: PhantomData<Operation>,
}

pub(crate) trait OperationMarker {
    const KIND: CameraPreflightOperation;
}

impl OperationMarker for BackupRestore {
    const KIND: CameraPreflightOperation = CameraPreflightOperation::BackupRestore;
}

impl OperationMarker for RawConversion {
    const KIND: CameraPreflightOperation = CameraPreflightOperation::RawConversion;
}

impl OperationMarker for RawRecoveryFetch {
    const KIND: CameraPreflightOperation = CameraPreflightOperation::RawRecoveryFetch;
}

impl OperationMarker for RawRecoveryCleanup {
    const KIND: CameraPreflightOperation = CameraPreflightOperation::RawRecoveryCleanup;
}

impl OperationMarker for SimulationAccess {
    const KIND: CameraPreflightOperation = CameraPreflightOperation::SimulationAccess;
}

impl OperationMarker for SimulationWrite {
    const KIND: CameraPreflightOperation = CameraPreflightOperation::SimulationWrite;
}

impl<Operation> ValidatedCameraSession<'_, Operation> {
    pub fn evidence(&self) -> &PreflightEvidence {
        &self.evidence
    }

    pub(crate) const fn capability_profile(&self) -> &'static CameraFirmwareCapabilityProfile {
        self.capability_profile
    }

    fn camera_and_permit(&mut self) -> (&mut Camera, &mut MutationPermit) {
        (self.camera, &mut self.permit)
    }
}

impl ValidatedCameraSession<'_, BackupRestore> {
    pub fn target_identity(&self) -> BackupIdentity {
        backup_identity_from_evidence(&self.evidence)
    }

    pub fn export_recovery(&mut self) -> anyhow::Result<crate::features::backup::BackupArtifact> {
        self.camera
            .export_backup_unchecked(crate::features::backup::BackupPurpose::Recovery)
    }

    pub fn restore(
        &mut self,
        artifact: &crate::features::backup::BackupArtifact,
        expected_target_serial_sha256: Option<&str>,
    ) -> anyhow::Result<crate::features::backup::BackupRestoreAccepted> {
        artifact.validate_target(
            &backup_identity_from_evidence(&self.evidence),
            expected_target_serial_sha256,
        )?;
        let (camera, permit) = self.camera_and_permit();
        camera.import_backup_unchecked(permit, artifact)
    }
}

impl ValidatedCameraSession<'_, RawConversion> {
    pub fn render(
        &mut self,
        image: &[u8],
        partial: RenderBase,
        draft: bool,
    ) -> anyhow::Result<crate::features::render::RenderOutcome> {
        partial.validate_firmware_capabilities(self.capability_profile())?;
        let (camera, permit) = self.camera_and_permit();
        camera.render_unchecked(permit, image, partial, draft)
    }

    pub fn cleanup_rendered_object(
        &mut self,
        handle: u32,
    ) -> anyhow::Result<crate::features::outcome::StateChangeAudit> {
        let (camera, permit) = self.camera_and_permit();
        camera.cleanup_rendered_object_unchecked(permit, handle)
    }
}

impl ValidatedCameraSession<'_, RawRecoveryFetch> {
    pub fn recover_rendered_object(
        &mut self,
        handle: u32,
    ) -> anyhow::Result<crate::features::render::RenderedObject> {
        let (camera, permit) = self.camera_and_permit();
        camera.recover_rendered_object_unchecked(permit, handle)
    }
}

impl ValidatedCameraSession<'_, RawRecoveryCleanup> {
    pub fn cleanup_rendered_object(
        &mut self,
        handle: u32,
    ) -> anyhow::Result<crate::features::outcome::StateChangeAudit> {
        let (camera, permit) = self.camera_and_permit();
        camera.cleanup_rendered_object_unchecked(permit, handle)
    }
}

impl ValidatedCameraSession<'_, SimulationAccess> {
    pub fn get_simulation(&mut self, slot: CustomSetting) -> anyhow::Result<Box<dyn Simulation>> {
        let (camera, permit) = self.camera_and_permit();
        camera.get_simulation_unchecked(permit, slot)
    }

    pub fn get_simulations(
        &mut self,
        slots: &[CustomSetting],
    ) -> anyhow::Result<Vec<(CustomSetting, Box<dyn Simulation>)>> {
        let (camera, permit) = self.camera_and_permit();
        camera.get_simulations_unchecked(permit, slots)
    }
}

impl ValidatedCameraSession<'_, SimulationWrite> {
    pub fn get_simulation(&mut self, slot: CustomSetting) -> anyhow::Result<Box<dyn Simulation>> {
        let (camera, permit) = self.camera_and_permit();
        camera.get_simulation_unchecked(permit, slot)
    }

    pub fn update_simulation(
        &mut self,
        slot: CustomSetting,
        partial: SimulationBase,
    ) -> Result<SimulationTransactionSuccess, SimulationTransactionError> {
        partial
            .validate_firmware_capabilities(self.capability_profile())
            .map_err(|error| {
                SimulationTransactionError::preparation(self.camera.ptp.is_healthy(), error)
            })?;
        let (camera, permit) = self.camera_and_permit();
        camera.update_simulation_unchecked(permit, slot, partial)
    }

    pub fn set_simulation(
        &mut self,
        slot: CustomSetting,
        simulation: &dyn Simulation,
    ) -> Result<SimulationTransactionSuccess, SimulationTransactionError> {
        simulation
            .to_base()
            .validate_firmware_capabilities(self.capability_profile())
            .map_err(|error| {
                SimulationTransactionError::preparation(self.camera.ptp.is_healthy(), error)
            })?;
        let (camera, permit) = self.camera_and_permit();
        camera.set_simulation_unchecked(permit, slot, simulation)
    }
}

impl<Operation> Drop for ValidatedCameraSession<'_, Operation> {
    fn drop(&mut self) {
        self.camera.ptp.clear_mutation_permit();
    }
}

/// Everything `run` reads from the camera (or already knows) before any
/// authorization decision is made. `run` does not build this struct itself —
/// it interleaves the same checks with the device reads that produce these
/// values, in order to preserve the exact sequence of PTP operations it
/// issues (see `decide_profile_and_device_info` and
/// `decide_mode_battery_and_binding`). This struct and `decide_preflight`
/// exist so tests can drive the *entire* ordered decision — including the
/// native-binding and physical-identity gates that run before any device
/// read — as a single contract, without a USB device.
///
/// Only tests construct this today, since `run` deliberately keeps its own
/// checks interleaved with device reads instead of gathering facts upfront
/// (see `decide_preflight`'s doc comment) — hence `#[cfg(test)]`.
#[cfg(test)]
struct PreflightFacts<'a> {
    binding: ModelBindingKind,
    definition: &'static SupportedCamera,
    physical_identity: PhysicalUsbIdentity,
    info: &'a crate::ptp::DeviceInfo,
    usb_mode: u32,
    battery_percent: u8,
    serial_binding: Option<&'a SerialFingerprint>,
}

/// The complete preflight authorization decision, with no device I/O.
/// Returns the selected profiles on success. The order of checks here is the
/// contract `run` must preserve: native binding, physical identity, firmware
/// profile selection, capability profile selection, device info, USB
/// mode/battery, then serial binding.
///
/// `run` never calls this directly: gathering every fact before deciding
/// anything would force it to issue the USB-mode and battery PTP reads even
/// when an earlier, cheaper check (e.g. an unsupported firmware) would
/// otherwise have short-circuited before those reads happen today. Instead
/// `run` calls the two `decide_*` stage functions below at the same points
/// in its own control flow where the equivalent checks run today. This
/// function exists to let a test exercise the full ordered contract.
#[cfg(test)]
fn decide_preflight(
    facts: &PreflightFacts<'_>,
    operation: CameraPreflightOperation,
) -> anyhow::Result<(
    &'static CameraPreflightProfile,
    &'static CameraFirmwareCapabilityProfile,
)> {
    validate_native_binding(facts.binding)?;
    validate_physical_identity(facts.definition, facts.physical_identity)?;
    let (profile, capability_profile) =
        decide_profile_and_device_info(facts.definition, facts.info, operation)?;
    decide_mode_battery_and_binding(
        profile,
        operation,
        facts.usb_mode,
        facts.battery_percent,
        facts.serial_binding,
        facts.info,
    )?;
    Ok((profile, capability_profile))
}

/// Stage 1 of the preflight decision, evaluated once `GetDeviceInfo` has
/// returned: selects the firmware compatibility profile, selects the
/// firmware capability profile, then validates the device info against the
/// selected profile. Order: firmware profile selection, capability profile
/// selection, device info validation — matching `run`'s sequence exactly.
fn decide_profile_and_device_info(
    definition: &'static SupportedCamera,
    info: &crate::ptp::DeviceInfo,
    operation: CameraPreflightOperation,
) -> anyhow::Result<(
    &'static CameraPreflightProfile,
    &'static CameraFirmwareCapabilityProfile,
)> {
    let profile = select_profile(definition, operation, &info.device_version)?;
    let capability_profile = select_capability_profile(definition, &info.device_version)?;
    validate_device_info(definition, profile, info)?;
    Ok((profile, capability_profile))
}

/// Stage 2 of the preflight decision, evaluated once the USB mode and
/// battery percentage have been read: validates them against the selected
/// profile, then hashes and validates the serial binding. Order: mode and
/// battery, then serial binding — matching `run`'s sequence exactly. Returns
/// the hashed serial so `run` can reuse it for `PreflightEvidence` without
/// hashing twice.
fn decide_mode_battery_and_binding(
    profile: &CameraPreflightProfile,
    operation: CameraPreflightOperation,
    usb_mode: u32,
    battery_percent: u8,
    serial_binding: Option<&SerialFingerprint>,
    info: &crate::ptp::DeviceInfo,
) -> anyhow::Result<String> {
    validate_mode_and_battery(profile, operation, usb_mode, battery_percent)?;
    let serial_sha256 = crate::features::backup::sha256_hex(info.serial_number.as_bytes());
    validate_serial_binding(serial_binding, &serial_sha256)?;
    Ok(serial_sha256)
}

pub(crate) fn run<'camera, Operation: OperationMarker>(
    camera: &'camera mut Camera,
    serial_binding: Option<&SerialFingerprint>,
) -> anyhow::Result<ValidatedCameraSession<'camera, Operation>> {
    camera.ptp.clear_mutation_permit();
    validate_native_binding(camera.binding)?;

    let definition = camera.r#impl.camera_definition();
    validate_physical_identity(definition, camera.physical_identity)?;
    let info = camera.ptp.get_info()?;
    let (profile, capability_profile) =
        decide_profile_and_device_info(definition, &info, Operation::KIND)?;

    let usb_mode = u32::from(camera.ptp.get_prop::<u16>(0xD16E_u16)?);
    let battery_percent = read_battery_percent(&mut camera.ptp)?;
    let serial_sha256 = decide_mode_battery_and_binding(
        profile,
        Operation::KIND,
        usb_mode,
        battery_percent,
        serial_binding,
        &info,
    )?;

    let descriptors = read_and_validate_descriptors(&mut camera.ptp, profile)?;
    let permit_id = camera.ptp.activate_mutation_permit()?;
    let (bus, address, interface) = camera.ptp.transport_binding();
    let permit = MutationPermit::new(
        permit_id,
        (bus, address, interface),
        Operation::KIND,
        profile.required_operations,
        descriptors,
        capability_profile,
    );

    let evidence = PreflightEvidence {
        camera_name: definition.name,
        physical_vendor_id: camera.physical_identity.vendor_id,
        physical_product_id: camera.physical_identity.product_id,
        manufacturer: info.manufacturer,
        model: info.model,
        firmware: info.device_version,
        serial_sha256,
        usb_mode,
        battery_percent,
    };

    Ok(ValidatedCameraSession {
        camera,
        permit,
        evidence,
        capability_profile,
        operation: PhantomData,
    })
}

fn select_profile(
    camera: &'static SupportedCamera,
    operation: CameraPreflightOperation,
    firmware: &str,
) -> anyhow::Result<&'static CameraPreflightProfile> {
    let mut matches = camera
        .preflight_profiles
        .iter()
        .filter(|profile| profile.operation == operation && profile.firmware == firmware);
    let Some(profile) = matches.next() else {
        let supported = camera
            .preflight_profiles
            .iter()
            .filter(|profile| profile.operation == operation)
            .map(|profile| profile.firmware)
            .collect::<Vec<_>>()
            .join(", ");
        bail!(
            "firmware {} is not in the {:?} compatibility matrix for {}; supported firmware: {}",
            sanitize_for_display(firmware),
            operation,
            camera.name,
            if supported.is_empty() {
                "none"
            } else {
                &supported
            }
        );
    };
    ensure!(
        matches.next().is_none(),
        "ambiguous preflight profiles for firmware {} and {operation:?}",
        sanitize_for_display(firmware)
    );
    ensure!(
        profile.status == CameraPreflightProfileStatus::Verified,
        "firmware {} has only an unverified {operation:?} profile",
        sanitize_for_display(firmware)
    );
    Ok(profile)
}

fn select_capability_profile(
    camera: &'static SupportedCamera,
    firmware: &str,
) -> anyhow::Result<&'static CameraFirmwareCapabilityProfile> {
    CameraFirmwareCapabilityProfile::find_exact(camera.firmware_capability_profiles, firmware)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "firmware {} has no exact capability profile for {}",
                sanitize_for_display(firmware),
                camera.name
            )
        })
}

const MAX_DISPLAY_TEXT_CHARS: usize = 64;

/// Renders an untrusted device-supplied string safely for terminal error
/// messages: strips control characters (ANSI escapes included) and caps
/// length so a spoofed device cannot inject terminal sequences via stderr.
fn sanitize_for_display(value: &str) -> String {
    let filtered: Vec<char> = value
        .chars()
        .filter(|character| !character.is_control())
        .collect();
    if filtered.len() > MAX_DISPLAY_TEXT_CHARS {
        let mut sanitized: String = filtered[..MAX_DISPLAY_TEXT_CHARS].iter().collect();
        sanitized.push_str("...");
        sanitized
    } else {
        filtered.into_iter().collect()
    }
}

fn validate_native_binding(binding: ModelBindingKind) -> anyhow::Result<()> {
    ensure!(
        binding == ModelBindingKind::Native,
        "state-changing camera operations require a native physical model binding"
    );
    Ok(())
}

fn validate_physical_identity(
    camera: &SupportedCamera,
    physical: PhysicalUsbIdentity,
) -> anyhow::Result<()> {
    ensure!(
        physical.vendor_id == camera.vendor && physical.product_id == camera.product,
        "physical USB VID/PID does not match the selected camera definition"
    );
    Ok(())
}

fn validate_device_info(
    camera: &SupportedCamera,
    profile: &CameraPreflightProfile,
    info: &crate::ptp::DeviceInfo,
) -> anyhow::Result<()> {
    let identity = camera
        .ptp_identity
        .ok_or_else(|| anyhow::anyhow!("camera has no exact PTP identity metadata"))?;
    ensure!(
        info.manufacturer == identity.manufacturer,
        "PTP manufacturer mismatch: expected {}, received {}",
        identity.manufacturer,
        sanitize_for_display(&info.manufacturer)
    );
    ensure!(
        info.model == identity.model,
        "PTP model mismatch: expected {}, received {}",
        identity.model,
        sanitize_for_display(&info.model)
    );
    ensure!(!info.serial_number.is_empty(), "PTP serial number is empty");
    ensure!(
        !info.device_version.is_empty(),
        "PTP firmware version is empty"
    );
    for operation in profile.required_operations {
        ensure!(
            info.operations_supported.contains(operation),
            "camera does not advertise required PTP operation 0x{operation:04x}"
        );
    }
    for property in profile.required_properties {
        ensure!(
            info.device_properties_supported.contains(&property.code),
            "camera does not advertise required PTP device property 0x{:04x}",
            property.code
        );
    }
    Ok(())
}

fn read_battery_percent(ptp: &mut Ptp) -> anyhow::Result<u8> {
    let battery = ptp.get_prop::<PtpString>(0xD36B_u16)?.into_inner();
    let value = battery
        .split(',')
        .next()
        .ok_or_else(|| anyhow::anyhow!("camera battery response is empty"))?
        .trim()
        .parse::<u8>()?;
    ensure!(value <= 100, "camera battery percentage exceeds 100");
    Ok(value)
}

fn validate_mode_and_battery(
    profile: &CameraPreflightProfile,
    operation: CameraPreflightOperation,
    usb_mode: u32,
    battery_percent: u8,
) -> anyhow::Result<()> {
    ensure!(
        profile.allowed_usb_modes.contains(&usb_mode),
        "USB mode {usb_mode} is not allowed for {operation:?}; configure the X-T5 connection mode required by this operation"
    );
    ensure!(
        battery_percent >= profile.minimum_battery_percent,
        "camera battery is {battery_percent}%, below the required {}%",
        profile.minimum_battery_percent
    );
    Ok(())
}

fn validate_serial_binding(
    serial_binding: Option<&SerialFingerprint>,
    observed_serial_sha256: &str,
) -> anyhow::Result<()> {
    if let Some(binding) = serial_binding {
        ensure!(
            binding.as_str() == observed_serial_sha256,
            "connected camera serial fingerprint does not match --target-serial-sha256"
        );
    }
    Ok(())
}

fn read_and_validate_descriptors(
    ptp: &mut Ptp,
    profile: &CameraPreflightProfile,
) -> anyhow::Result<Vec<DevicePropDesc>> {
    let mut descriptors = Vec::with_capacity(profile.required_properties.len());
    for requirement in profile.required_properties {
        let descriptor = ptp.get_prop_desc(requirement.code)?;
        ensure!(
            descriptor.property_code == requirement.code,
            "GetDevicePropDesc returned the wrong property code"
        );
        if let CameraPreflightDataType::Exact(expected) = requirement.data_type {
            ensure!(
                descriptor.data_type.code() == expected,
                "PTP datatype mismatch for property 0x{:04x}: expected 0x{expected:04x}, received 0x{:04x}",
                requirement.code,
                descriptor.data_type.code()
            );
        }
        if requirement.writable {
            ensure!(
                descriptor.writable,
                "required PTP device property 0x{:04x} is read-only",
                requirement.code
            );
        }
        descriptors.push(descriptor);
    }
    Ok(descriptors)
}

fn backup_identity_from_evidence(evidence: &PreflightEvidence) -> BackupIdentity {
    BackupIdentity {
        camera_name: evidence.camera_name.to_owned(),
        vendor_id: evidence.physical_vendor_id,
        product_id: evidence.physical_product_id,
        manufacturer: evidence.manufacturer.clone(),
        model: evidence.model.clone(),
        firmware: evidence.firmware.clone(),
        serial_sha256: evidence.serial_sha256.clone(),
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use crate::{
        SupportedCamera,
        generated::cameras::{
            CameraFirmwareCapabilityProfile, CameraPreflightDataType, CameraPreflightOperation,
            CameraPreflightProfile, CameraPreflightProfileStatus, CameraPreflightProperty,
            CameraPtpIdentity,
        },
        policy::{ModelBindingKind, PhysicalUsbIdentity, SerialFingerprint},
        ptp::{CommandCode, DevicePropDataType, DevicePropDesc, DevicePropForm, DevicePropValue},
    };

    use super::{
        MAX_DISPLAY_TEXT_CHARS, MutationAuthorization, MutationPermit, PreflightFacts,
        decide_preflight, sanitize_for_display, select_capability_profile, select_profile,
        validate_device_info, validate_mode_and_battery, validate_physical_identity,
        validate_serial_binding,
    };

    const PROFILE: CameraPreflightProfile = CameraPreflightProfile {
        operation: CameraPreflightOperation::RawConversion,
        status: CameraPreflightProfileStatus::Verified,
        firmware: "4.31",
        minimum_battery_percent: 100,
        allowed_usb_modes: &[6],
        required_operations: &[0x1001],
        required_properties: &[CameraPreflightProperty {
            code: 0xD16E,
            data_type: CameraPreflightDataType::Any,
            writable: false,
        }],
    };

    const CAMERA: SupportedCamera = SupportedCamera {
        name: "FUJIFILM X-T5",
        vendor: 0x04cb,
        product: 0x02fc,
        ptp_identity: Some(CameraPtpIdentity {
            manufacturer: "FUJIFILM",
            model: "X-T5",
        }),
        preflight_profiles: &[PROFILE],
        firmware_capability_profiles: &[],
        camera_factory: crate::features::base::UNKNOWN_CAMERA.camera_factory,
    };

    const EMPTY_CAPABILITY_PROFILE: CameraFirmwareCapabilityProfile =
        CameraFirmwareCapabilityProfile {
            firmware: "test",
            options: &[],
            raw_conversion: None,
        };
    const RAW_CAPABILITY_PROFILE: CameraFirmwareCapabilityProfile =
        CameraFirmwareCapabilityProfile {
            firmware: "test",
            options: &[],
            raw_conversion: Some(crate::generated::cameras::CameraRawConversionDescriptor {
                id: "unverified-test",
                evidence_status:
                    crate::generated::cameras::CameraRawConversionEvidenceStatus::Unverified,
                evidence_manifests: &[],
                usb_modes: &[6],
                camera_state: None,
                read: crate::generated::cameras::CameraRawConversionLayout {
                    profile_code: "1",
                    header_padding: 2,
                    declared_field_count: 1,
                    total_length: 9,
                    fields: &["field"],
                },
                write: Some(crate::generated::cameras::CameraRawConversionLayout {
                    profile_code: "1",
                    header_padding: 2,
                    declared_field_count: 1,
                    total_length: 9,
                    fields: &["field"],
                }),
            }),
        };

    const FIRMWARE_4_31_CAPABILITY_PROFILE: CameraFirmwareCapabilityProfile =
        CameraFirmwareCapabilityProfile {
            firmware: "4.31",
            options: &[],
            raw_conversion: None,
        };

    /// `CAMERA` plus a matching firmware capability profile, so
    /// `decide_preflight` can select both a preflight profile and a
    /// capability profile for firmware "4.31".
    const FULL_CAMERA: SupportedCamera = SupportedCamera {
        firmware_capability_profiles: &[FIRMWARE_4_31_CAPABILITY_PROFILE],
        ..CAMERA
    };

    /// A `DeviceInfo` that satisfies every check `PROFILE`/`FULL_CAMERA`
    /// require: matching manufacturer/model, firmware "4.31", and the
    /// required operation/property advertised.
    fn valid_device_info() -> crate::ptp::DeviceInfo {
        crate::ptp::DeviceInfo {
            version: 100,
            vendor_ex_id: 0,
            vendor_ex_version: 0,
            vendor_extension_desc: "Fujifilm".to_owned(),
            functional_mode: 0,
            operations_supported: vec![0x1001],
            events_supported: vec![],
            device_properties_supported: vec![0xD16E],
            capture_formats: vec![],
            image_formats: vec![],
            manufacturer: "FUJIFILM".to_owned(),
            model: "X-T5".to_owned(),
            device_version: "4.31".to_owned(),
            serial_number: "serial".to_owned(),
        }
    }

    const VALID_PHYSICAL_IDENTITY: PhysicalUsbIdentity = PhysicalUsbIdentity {
        vendor_id: 0x04cb,
        product_id: 0x02fc,
    };

    #[test]
    fn rejects_non_native_binding_before_any_other_check() {
        let info = valid_device_info();
        // Physical identity and firmware are also invalid, to prove the
        // binding gate fires first regardless.
        let facts = PreflightFacts {
            binding: ModelBindingKind::Unknown,
            definition: &FULL_CAMERA,
            physical_identity: PhysicalUsbIdentity {
                vendor_id: 0xdead,
                product_id: 0xbeef,
            },
            info: &info,
            usb_mode: 1,
            battery_percent: 0,
            serial_binding: None,
        };

        let error = decide_preflight(&facts, CameraPreflightOperation::RawConversion)
            .expect_err("a non-native binding must fail before any other check");

        assert!(error.to_string().contains("native physical model binding"));
    }

    #[test]
    fn rejects_wrong_physical_identity_before_profile_selection() {
        let mut info = valid_device_info();
        info.device_version = "9.99".to_owned(); // also unsupported firmware
        let facts = PreflightFacts {
            binding: ModelBindingKind::Native,
            definition: &FULL_CAMERA,
            physical_identity: PhysicalUsbIdentity {
                vendor_id: 0xdead,
                product_id: 0xbeef,
            },
            info: &info,
            usb_mode: 1,
            battery_percent: 0,
            serial_binding: None,
        };

        let error = decide_preflight(&facts, CameraPreflightOperation::RawConversion)
            .expect_err("wrong physical identity must fail before firmware profile selection");

        assert!(
            error
                .to_string()
                .contains("physical USB VID/PID does not match")
        );
    }

    #[test]
    fn rejects_unknown_firmware_before_mode_and_battery() {
        let mut info = valid_device_info();
        info.device_version = "9.99".to_owned();
        let facts = PreflightFacts {
            binding: ModelBindingKind::Native,
            definition: &FULL_CAMERA,
            physical_identity: VALID_PHYSICAL_IDENTITY,
            info: &info,
            // Also invalid, to prove the firmware gate fires first.
            usb_mode: 1,
            battery_percent: 0,
            serial_binding: None,
        };

        let error = decide_preflight(&facts, CameraPreflightOperation::RawConversion)
            .expect_err("unknown firmware must fail before mode/battery are checked");

        assert!(error.to_string().contains("not in the"));
    }

    #[test]
    fn rejects_wrong_usb_mode_and_low_battery() {
        let info = valid_device_info();
        let wrong_mode_facts = PreflightFacts {
            binding: ModelBindingKind::Native,
            definition: &FULL_CAMERA,
            physical_identity: VALID_PHYSICAL_IDENTITY,
            info: &info,
            usb_mode: 1,
            battery_percent: 100,
            serial_binding: None,
        };
        let wrong_mode_error =
            decide_preflight(&wrong_mode_facts, CameraPreflightOperation::RawConversion)
                .expect_err("disallowed USB mode must fail closed");
        assert!(wrong_mode_error.to_string().contains("USB mode"));

        let low_battery_facts = PreflightFacts {
            binding: ModelBindingKind::Native,
            definition: &FULL_CAMERA,
            physical_identity: VALID_PHYSICAL_IDENTITY,
            info: &info,
            usb_mode: 6,
            battery_percent: 99,
            serial_binding: None,
        };
        let low_battery_error =
            decide_preflight(&low_battery_facts, CameraPreflightOperation::RawConversion)
                .expect_err("insufficient battery must fail closed");
        assert!(low_battery_error.to_string().contains("battery"));
    }

    #[test]
    fn rejects_mismatched_serial_binding() {
        let info = valid_device_info();
        let binding = SerialFingerprint::from_str(&"0".repeat(64)).unwrap();
        let facts = PreflightFacts {
            binding: ModelBindingKind::Native,
            definition: &FULL_CAMERA,
            physical_identity: VALID_PHYSICAL_IDENTITY,
            info: &info,
            usb_mode: 6,
            battery_percent: 100,
            serial_binding: Some(&binding),
        };

        let error = decide_preflight(&facts, CameraPreflightOperation::RawConversion)
            .expect_err("a mismatched serial fingerprint must fail closed");

        assert!(
            error
                .to_string()
                .contains("does not match --target-serial-sha256")
        );
    }

    #[test]
    fn accepts_a_fully_valid_x_t5_4_31_case() {
        let info = valid_device_info();
        let facts = PreflightFacts {
            binding: ModelBindingKind::Native,
            definition: &FULL_CAMERA,
            physical_identity: VALID_PHYSICAL_IDENTITY,
            info: &info,
            usb_mode: 6,
            battery_percent: 100,
            serial_binding: None,
        };

        let (profile, capability_profile) =
            decide_preflight(&facts, CameraPreflightOperation::RawConversion)
                .expect("every check is satisfied, so the decision must succeed");

        assert_eq!(profile.firmware, "4.31");
        assert_eq!(profile.operation, CameraPreflightOperation::RawConversion);
        assert_eq!(capability_profile.firmware, "4.31");
    }

    #[test]
    fn permit_validation_still_enforces_dynamic_property_enumeration() {
        let descriptor = DevicePropDesc {
            property_code: 0xD001,
            data_type: DevicePropDataType::UInt16,
            writable: true,
            factory_default: DevicePropValue::UInt(1),
            current: DevicePropValue::UInt(1),
            form: DevicePropForm::Enumeration(vec![DevicePropValue::UInt(1)]),
        };
        let authorization =
            MutationAuthorization::new(&[0x1016], vec![descriptor], &EMPTY_CAPABILITY_PROFILE);

        let result = authorization.validate(
            CommandCode::SetDevicePropValue,
            &[0xD001],
            Some(&2_u16.to_le_bytes()),
        );

        assert!(result.is_err());
    }

    #[test]
    fn raw_upload_requires_a_validated_conversion_profile() {
        let authorization =
            MutationAuthorization::new(&[0x900d], vec![], &EMPTY_CAPABILITY_PROFILE);

        let error = authorization
            .validate(CommandCode::FujiSendObject, &[], Some(b"RAF"))
            .expect_err("RAW image upload must wait until the conversion profile is validated");

        assert!(error.to_string().contains("profile"));
    }

    #[test]
    fn unrelated_property_validation_cannot_authorize_raw_upload() {
        let selector = DevicePropDesc {
            property_code: 0xD18C,
            data_type: DevicePropDataType::UInt16,
            writable: true,
            factory_default: DevicePropValue::UInt(1),
            current: DevicePropValue::UInt(1),
            form: DevicePropForm::None,
        };
        let mut permit = MutationPermit::new(
            1,
            (1, 2, 3),
            CameraPreflightOperation::RawConversion,
            &[0x900d],
            vec![selector],
            &RAW_CAPABILITY_PROFILE,
        );

        permit
            .validate_raw_conversion_profile(1, 2, &["field"], &1_u16.to_le_bytes())
            .expect_err("only the RAW conversion profile descriptor can unlock upload");
        let error = permit
            .validate_mutation(CommandCode::FujiSendObject, &[], Some(b"RAF"))
            .expect_err("selector validation must not authorize RAW upload");

        assert!(error.to_string().contains("profile"));
    }

    #[test]
    fn permit_rejects_inactive_or_mismatched_transport_binding() {
        let permit = MutationPermit::new(
            7,
            (1, 2, 3),
            CameraPreflightOperation::SimulationWrite,
            &[0x1016],
            vec![],
            &EMPTY_CAPABILITY_PROFILE,
        );

        assert!(!permit.is_active_for(1, 2, 3, None));
        assert!(!permit.is_active_for(1, 9, 3, Some(7)));
        assert!(!permit.is_active_for(1, 2, 3, Some(8)));
        assert!(permit.is_active_for(1, 2, 3, Some(7)));
        assert_eq!(
            permit.operation(),
            CameraPreflightOperation::SimulationWrite
        );
    }

    #[test]
    fn firmware_capability_selection_does_not_fall_back_to_another_version() {
        const CAPABILITIES: [CameraFirmwareCapabilityProfile; 1] =
            [CameraFirmwareCapabilityProfile {
                firmware: "4.31",
                options: &[],
                raw_conversion: None,
            }];
        const CAPABILITY_CAMERA: SupportedCamera = SupportedCamera {
            firmware_capability_profiles: &CAPABILITIES,
            ..CAMERA
        };

        let error = select_capability_profile(&CAPABILITY_CAMERA, "4.32")
            .expect_err("unknown firmware must not inherit another capability profile");

        assert!(error.to_string().contains("4.32"));
    }

    #[test]
    fn x_t5_reala_ace_capability_starts_at_firmware_4_00() {
        let camera = crate::generated::cameras::SUPPORTED
            .iter()
            .find(|camera| camera.name == "FUJIFILM X-T5")
            .expect("X-T5 must be generated");
        let before = select_capability_profile(camera, "3.01").expect("3.01 profile must exist");
        let after = select_capability_profile(camera, "4.00").expect("4.00 profile must exist");
        assert!(before.raw_conversion.is_none());
        assert!(after.raw_conversion.is_none());

        assert!(
            before
                .validate_option_value("film_simulation", "reala_ace")
                .is_err()
        );
        assert_eq!(
            after
                .write_wire_value("film_simulation", "reala_ace")
                .expect("Reala Ace must have an exact post-4.00 wire value"),
            0x14
        );

        let current = select_capability_profile(camera, "4.31").expect("4.31 profile must exist");
        let raw = current
            .raw_conversion
            .expect("4.31 RAW assumptions must remain inspectable for discovery");
        assert_eq!(
            raw.evidence_status,
            crate::generated::cameras::CameraRawConversionEvidenceStatus::Unverified
        );
        assert!(raw.evidence_manifests.is_empty());
        assert_eq!(raw.read.profile_code, "ff179502");
        assert_eq!(raw.read.header_padding, 0x1ee);
        assert_eq!(raw.read.declared_field_count, 29);
        assert_eq!(raw.read.total_length, 625);
        assert_eq!(raw.read.fields.len(), 28);
        assert_eq!(raw.read.fields.first(), Some(&"head_0"));
        assert_eq!(raw.read.fields.last(), Some(&"teleconverter"));
        let write = raw
            .write
            .expect("the assumed write shape must remain inspectable");
        assert_eq!(write.declared_field_count, 29);
        assert_eq!(write.total_length, 629);
        assert_eq!(write.fields.len(), 29);
        assert_eq!(write.fields.last(), Some(&"tail_0"));
        let write_error = current
            .validate_raw_conversion_signature(0xff17_9502, 0x1ee, write.fields, 629)
            .expect_err("unverified 4.31 RAW assumptions must not authorize writes");
        assert!(write_error.to_string().contains("not write-verified"));

        let preflight_error =
            select_profile(camera, CameraPreflightOperation::RawConversion, "4.31")
                .expect_err("4.31 RAW conversion must stay disabled until HIL evidence exists");
        assert!(preflight_error.to_string().contains("unverified"));

        // Missing entries here would break the selector and otherwise valid RAW
        // enum fields before any device I/O.
        assert_eq!(current.write_wire_value("custom_setting", "c1").unwrap(), 1);
        assert!(current.write_wire_value("file_type", "jpeg").is_ok());
        assert!(current.write_wire_value("dynamic_range", "hdr100").is_ok());
    }

    #[test]
    fn unknown_firmware_fails_closed() {
        let error = select_profile(&CAMERA, CameraPreflightOperation::RawConversion, "4.32")
            .expect_err("unknown firmware must not inherit a nearby compatibility profile");

        assert!(error.to_string().contains("not in the"));
    }

    #[test]
    fn sanitize_for_display_strips_control_characters() {
        let sanitized = sanitize_for_display("\u{1b}[31mEVIL\u{1b}[0m4.31");

        assert!(!sanitized.chars().any(char::is_control));
        assert!(sanitized.contains("EVIL"));
        assert!(sanitized.contains("4.31"));
    }

    #[test]
    fn sanitize_for_display_truncates_long_strings() {
        let long_value = "a".repeat(200);

        let sanitized = sanitize_for_display(&long_value);

        assert_eq!(
            sanitized.chars().count(),
            MAX_DISPLAY_TEXT_CHARS + "...".len()
        );
        assert!(sanitized.ends_with("..."));
    }

    #[test]
    fn sanitize_for_display_passes_through_clean_strings() {
        assert_eq!(sanitize_for_display("4.31"), "4.31");
    }

    #[test]
    fn wrong_physical_camera_fails_closed() {
        let result = validate_physical_identity(
            &CAMERA,
            PhysicalUsbIdentity {
                vendor_id: 0x04cb,
                product_id: 0x02f7,
            },
        );

        assert!(result.is_err());
    }

    #[test]
    fn unsupported_usb_mode_and_low_battery_fail_closed() {
        let wrong_mode =
            validate_mode_and_battery(&PROFILE, CameraPreflightOperation::RawConversion, 1, 100);
        let low_battery =
            validate_mode_and_battery(&PROFILE, CameraPreflightOperation::RawConversion, 6, 99);

        assert!(wrong_mode.is_err());
        assert!(low_battery.is_err());
    }

    #[test]
    fn wrong_serial_binding_fails_closed() {
        let binding = SerialFingerprint::from_str(&"0".repeat(64)).unwrap();

        let result = validate_serial_binding(Some(&binding), &"1".repeat(64));

        assert!(result.is_err());
    }

    #[test]
    fn missing_capability_advertisement_fails_closed() {
        let info = crate::ptp::DeviceInfo {
            version: 100,
            vendor_ex_id: 0,
            vendor_ex_version: 0,
            vendor_extension_desc: "Fujifilm".to_owned(),
            functional_mode: 0,
            operations_supported: vec![],
            events_supported: vec![],
            device_properties_supported: vec![],
            capture_formats: vec![],
            image_formats: vec![],
            manufacturer: "FUJIFILM".to_owned(),
            model: "X-T5".to_owned(),
            device_version: "4.31".to_owned(),
            serial_number: "serial".to_owned(),
        };

        let result = validate_device_info(&CAMERA, &PROFILE, &info);

        assert!(result.is_err());
    }

    #[test]
    fn unverified_matrix_entry_fails_closed() {
        const UNVERIFIED_PROFILE: CameraPreflightProfile = CameraPreflightProfile {
            status: CameraPreflightProfileStatus::Unverified,
            ..PROFILE
        };
        const UNVERIFIED_CAMERA: SupportedCamera = SupportedCamera {
            preflight_profiles: &[UNVERIFIED_PROFILE],
            ..CAMERA
        };

        let result = select_profile(
            &UNVERIFIED_CAMERA,
            CameraPreflightOperation::RawConversion,
            "4.31",
        );

        assert!(result.is_err());
    }
}
