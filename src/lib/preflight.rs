use std::marker::PhantomData;

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
        self.camera.import_backup_unchecked(artifact)
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
        self.camera.render_unchecked(image, partial, draft)
    }

    pub fn cleanup_rendered_object(
        &mut self,
        handle: u32,
    ) -> anyhow::Result<crate::features::outcome::StateChangeAudit> {
        self.camera.cleanup_rendered_object_unchecked(handle)
    }
}

impl ValidatedCameraSession<'_, RawRecoveryFetch> {
    pub fn recover_rendered_object(
        &mut self,
        handle: u32,
    ) -> anyhow::Result<crate::features::render::RenderedObject> {
        self.camera.recover_rendered_object_unchecked(handle)
    }
}

impl ValidatedCameraSession<'_, RawRecoveryCleanup> {
    pub fn cleanup_rendered_object(
        &mut self,
        handle: u32,
    ) -> anyhow::Result<crate::features::outcome::StateChangeAudit> {
        self.camera.cleanup_rendered_object_unchecked(handle)
    }
}

impl ValidatedCameraSession<'_, SimulationAccess> {
    pub fn get_simulation(&mut self, slot: CustomSetting) -> anyhow::Result<Box<dyn Simulation>> {
        self.camera.get_simulation_unchecked(slot)
    }

    pub fn get_simulations(
        &mut self,
        slots: &[CustomSetting],
    ) -> anyhow::Result<Vec<(CustomSetting, Box<dyn Simulation>)>> {
        self.camera.get_simulations_unchecked(slots)
    }
}

impl ValidatedCameraSession<'_, SimulationWrite> {
    pub fn get_simulation(&mut self, slot: CustomSetting) -> anyhow::Result<Box<dyn Simulation>> {
        self.camera.get_simulation_unchecked(slot)
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
        self.camera.update_simulation_unchecked(slot, partial)
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
        self.camera.set_simulation_unchecked(slot, simulation)
    }
}

impl<Operation> Drop for ValidatedCameraSession<'_, Operation> {
    fn drop(&mut self) {
        self.camera.ptp.clear_mutation_authorization();
    }
}

pub(crate) fn run<'camera, Operation: OperationMarker>(
    camera: &'camera mut Camera,
    serial_binding: Option<&SerialFingerprint>,
) -> anyhow::Result<ValidatedCameraSession<'camera, Operation>> {
    camera.ptp.clear_mutation_authorization();
    ensure!(
        camera.binding == ModelBindingKind::Native,
        "state-changing camera operations require a native physical model binding"
    );

    let definition = camera.r#impl.camera_definition();
    validate_physical_identity(definition, camera.physical_identity)?;
    let info = camera.ptp.get_info()?;
    let profile = select_profile(definition, Operation::KIND, &info.device_version)?;
    let capability_profile = select_capability_profile(definition, &info.device_version)?;
    validate_device_info(definition, profile, &info)?;

    let usb_mode = u32::from(camera.ptp.get_prop::<u16>(0xD16E_u16)?);
    let battery_percent = read_battery_percent(&mut camera.ptp)?;
    validate_mode_and_battery(profile, Operation::KIND, usb_mode, battery_percent)?;

    let serial_sha256 = crate::features::backup::sha256_hex(info.serial_number.as_bytes());
    validate_serial_binding(serial_binding, &serial_sha256)?;

    let descriptors = read_and_validate_descriptors(&mut camera.ptp, profile)?;
    camera
        .ptp
        .authorize_mutations(profile.required_operations, descriptors, capability_profile);

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
            "firmware {firmware} is not in the {:?} compatibility matrix for {}; supported firmware: {}",
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
        "ambiguous preflight profiles for firmware {firmware} and {operation:?}"
    );
    ensure!(
        profile.status == CameraPreflightProfileStatus::Verified,
        "firmware {firmware} has only an unverified {operation:?} profile"
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
                "firmware {firmware} has no exact capability profile for {}",
                camera.name
            )
        })
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
        info.manufacturer
    );
    ensure!(
        info.model == identity.model,
        "PTP model mismatch: expected {}, received {}",
        identity.model,
        info.model
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
        policy::{PhysicalUsbIdentity, SerialFingerprint},
    };

    use super::{
        select_capability_profile, select_profile, validate_device_info, validate_mode_and_battery,
        validate_physical_identity, validate_serial_binding,
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
            .expect("verified 4.31 RAW conversion needs an exact signature");
        assert_eq!(raw.profile_code, 0xff17_9502);
        assert_eq!(raw.header_padding, 0x1ee);
        assert_eq!(raw.fields.len(), 29);
        assert_eq!(raw.fields.first(), Some(&"head_0"));
        assert_eq!(raw.fields.last(), Some(&"tail_0"));

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
