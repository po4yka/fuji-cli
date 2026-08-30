use std::collections::BTreeMap;

use proc_macro2::{Literal, TokenStream};
use quote::quote;

use crate::{
    ast::{
        Camera, PreflightOperation, PreflightStatus, RawConversionCameraState,
        RawConversionEvidenceStatus, RawConversionLayout,
    },
    schema::capabilities::resolve_firmware_capabilities,
    util::ident::{safe_upper_camel_case_ident, safe_uppercase_ident},
};

fn generate_raw_conversion_layout(layout: &RawConversionLayout) -> TokenStream {
    let profile_code = &layout.profile_code;
    let header_padding = usize::try_from(layout.header_padding)
        .map(Literal::usize_suffixed)
        .expect("u32 header padding must fit usize");
    let declared_field_count = Literal::u16_suffixed(layout.declared_field_count);
    let total_length = usize::try_from(layout.total_length)
        .map(Literal::usize_suffixed)
        .expect("u32 RAW profile length must fit usize");
    let fields = &layout.fields;

    quote! {
        CameraRawConversionLayout {
            profile_code: #profile_code,
            header_padding: #header_padding,
            declared_field_count: #declared_field_count,
            total_length: #total_length,
            fields: &[#(#fields),*],
        }
    }
}

pub fn generate(cameras: &BTreeMap<String, Camera>) -> anyhow::Result<TokenStream> {
    let mut sorted: Vec<&Camera> = cameras.values().collect();
    sorted.sort_by(|a, b| {
        (a.spec.generation.as_str(), a.id.as_str())
            .cmp(&(b.spec.generation.as_str(), b.id.as_str()))
    });

    let mut defs = Vec::new();
    let mut supported_entries = Vec::new();

    for camera in sorted {
        let struct_name = safe_upper_camel_case_ident(&camera.id);
        let const_name = safe_uppercase_ident(&format!("C_{}", camera.id));
        let name_str = &camera.spec.name;
        let vendor = Literal::u16_suffixed(camera.spec.usb.vendor_id);
        let product = Literal::u16_suffixed(camera.spec.usb.product_id);
        let chunk_size_ceiling =
            Literal::usize_suffixed(camera.spec.usb.chunk_size_ceiling.try_into()?);
        let ptp_identity = camera.spec.ptp.as_ref().map_or_else(
            || quote! { None },
            |identity| {
                let manufacturer = &identity.manufacturer;
                let model = &identity.model;

                quote! {
                    Some(CameraPtpIdentity {
                        manufacturer: #manufacturer,
                        model: #model,
                    })
                }
            },
        );
        let preflight_profiles = camera.spec.preflight.iter().map(|profile| {
            let operation = match profile.operation {
                PreflightOperation::BackupRestore => {
                    quote! { CameraPreflightOperation::BackupRestore }
                }
                PreflightOperation::SimulationAccess => {
                    quote! { CameraPreflightOperation::SimulationAccess }
                }
                PreflightOperation::SimulationWrite => {
                    quote! { CameraPreflightOperation::SimulationWrite }
                }
                PreflightOperation::RawConversion => {
                    quote! { CameraPreflightOperation::RawConversion }
                }
                PreflightOperation::RawRecoveryFetch => {
                    quote! { CameraPreflightOperation::RawRecoveryFetch }
                }
                PreflightOperation::RawRecoveryCleanup => {
                    quote! { CameraPreflightOperation::RawRecoveryCleanup }
                }
            };
            let status = match profile.status {
                PreflightStatus::Verified => quote! { CameraPreflightProfileStatus::Verified },
                PreflightStatus::Unverified => quote! { CameraPreflightProfileStatus::Unverified },
            };
            let firmware = &profile.firmware;
            let minimum_battery_percent = Literal::u8_suffixed(profile.minimum_battery_percent);
            let allowed_usb_modes = profile
                .allowed_usb_modes
                .iter()
                .copied()
                .map(Literal::u32_suffixed);
            let required_operations = profile
                .required_operations
                .iter()
                .copied()
                .map(Literal::u16_suffixed);
            let required_properties = profile.required_properties.iter().map(|property| {
                let code = Literal::u16_suffixed(property.code);
                let data_type = property.data_type.map_or_else(
                    || quote! { CameraPreflightDataType::Any },
                    |data_type| {
                        let data_type = Literal::u16_suffixed(data_type);
                        quote! { CameraPreflightDataType::Exact(#data_type) }
                    },
                );
                let writable = property.writable;

                quote! {
                    CameraPreflightProperty {
                        code: #code,
                        data_type: #data_type,
                        writable: #writable,
                    }
                }
            });

            quote! {
                CameraPreflightProfile {
                    operation: #operation,
                    status: #status,
                    firmware: #firmware,
                    minimum_battery_percent: #minimum_battery_percent,
                    allowed_usb_modes: &[#(#allowed_usb_modes),*],
                    required_operations: &[#(#required_operations),*],
                    required_properties: &[#(#required_properties),*],
                }
            }
        });
        let firmware_capability_profiles = match camera.spec.capabilities.as_ref() {
            Some(capabilities) => resolve_firmware_capabilities(capabilities)?,
            None => Vec::new(),
        }
        .into_iter()
        .map(|profile| {
            let firmware = profile.firmware;
            let raw_conversion = profile.raw_conversion.map_or_else(
                || quote! { None },
                |descriptor| {
                    let id = descriptor.id;
                    let evidence_status = match descriptor.evidence.status {
                        RawConversionEvidenceStatus::Unverified => {
                            quote! { CameraRawConversionEvidenceStatus::Unverified }
                        }
                        RawConversionEvidenceStatus::Observed => {
                            quote! { CameraRawConversionEvidenceStatus::Observed }
                        }
                        RawConversionEvidenceStatus::ReadVerified => {
                            quote! { CameraRawConversionEvidenceStatus::ReadVerified }
                        }
                        RawConversionEvidenceStatus::WriteVerified => {
                            quote! { CameraRawConversionEvidenceStatus::WriteVerified }
                        }
                    };
                    let manifests = descriptor.evidence.manifests;
                    let usb_modes = descriptor
                        .binding
                        .usb_modes
                        .into_iter()
                        .map(Literal::u32_suffixed);
                    let camera_state = descriptor.binding.camera_state.map_or_else(
                        || quote! { None },
                        |state| match state {
                            RawConversionCameraState::Still => {
                                quote! { Some(CameraRawConversionState::Still) }
                            }
                            RawConversionCameraState::Movie => {
                                quote! { Some(CameraRawConversionState::Movie) }
                            }
                        },
                    );
                    let read = generate_raw_conversion_layout(&descriptor.read);
                    let write = descriptor.write.as_ref().map_or_else(
                        || quote! { None },
                        |layout| {
                            let layout = generate_raw_conversion_layout(layout);
                            quote! { Some(#layout) }
                        },
                    );
                    quote! {
                        Some(CameraRawConversionDescriptor {
                            id: #id,
                            evidence_status: #evidence_status,
                            evidence_manifests: &[#(#manifests),*],
                            usb_modes: &[#(#usb_modes),*],
                            camera_state: #camera_state,
                            read: #read,
                            write: #write,
                        })
                    }
                },
            );
            let options = profile.options.into_iter().map(|(option, capability)| {
                let allowed_values = capability.allowed_values;
                let wire_values =
                    capability
                        .wire_values
                        .into_iter()
                        .map(|(logical_value, wire_values)| {
                            let wire_values = wire_values.into_iter().map(Literal::i32_suffixed);

                            quote! {
                                CameraOptionWireValue {
                                    logical_value: #logical_value,
                                    wire_values: &[#(#wire_values),*],
                                }
                            }
                        });

                quote! {
                    CameraOptionCapability {
                        option: #option,
                        allowed_values: &[#(#allowed_values),*],
                        wire_values: &[#(#wire_values),*],
                    }
                }
            });

            quote! {
                    CameraFirmwareCapabilityProfile {
                        firmware: #firmware,
                        options: &[#(#options),*],
                        raw_conversion: #raw_conversion,
                    }
            }
        });

        let features = camera.spec.features.as_ref();
        let backup_override = features.is_some_and(|f| f.backup).then(|| {
            quote! {
                fn as_backup_manager(
                    &self,
                ) -> Option<&dyn crate::features::backup::CameraBackupManager<Context = Self::Context>> {
                    Some(self)
                }
            }
        });
        let simulation_override = features.and_then(|f| f.simulation.as_ref()).map(|_| {
            quote! {
                fn as_simulation_parser(
                    &self,
                ) -> Option<&dyn crate::features::simulation::CameraSimulationParser> {
                    Some(self)
                }

                fn as_simulation_manager(
                    &self,
                ) -> Option<&dyn crate::features::simulation::CameraSimulationManager<Context = Self::Context>> {
                    Some(self)
                }
            }
        });
        let render_override = features.and_then(|f| f.render.as_ref()).map(|_| {
            quote! {
                fn as_render_manager(
                    &self,
                ) -> Option<&dyn crate::features::render::CameraRenderManager<Context = Self::Context>> {
                    Some(self)
                }
            }
        });

        defs.push(quote! {
            pub struct #struct_name;

            pub const #const_name: crate::SupportedCamera = crate::SupportedCamera {
                name: #name_str,
                vendor: #vendor,
                product: #product,
                ptp_identity: #ptp_identity,
                preflight_profiles: &[#(#preflight_profiles),*],
                firmware_capability_profiles: &[#(#firmware_capability_profiles),*],
                camera_factory: || Box::new(#struct_name),
            };

            impl crate::features::base::CameraBase for #struct_name {
                type Context = rusb::GlobalContext;

                fn camera_definition(&self) -> &'static crate::SupportedCamera {
                    &#const_name
                }

                fn chunk_size_ceiling(&self) -> usize {
                    #chunk_size_ceiling
                }

                #backup_override
                #simulation_override
                #render_override
            }
        });
        supported_entries.push(quote! { #const_name });
    }

    Ok(quote! {
        //! Generated camera definitions and supported device registry. Do not edit.

        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum CameraPreflightOperation {
            BackupRestore,
            SimulationAccess,
            SimulationWrite,
            RawConversion,
            RawRecoveryFetch,
            RawRecoveryCleanup,
        }

        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum CameraPreflightProfileStatus {
            Verified,
            Unverified,
        }

        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub struct CameraPtpIdentity {
            pub manufacturer: &'static str,
            pub model: &'static str,
        }

        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum CameraPreflightDataType {
            Any,
            Exact(u16),
        }

        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub struct CameraPreflightProperty {
            pub code: u16,
            pub data_type: CameraPreflightDataType,
            pub writable: bool,
        }

        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub struct CameraPreflightProfile {
            pub operation: CameraPreflightOperation,
            pub status: CameraPreflightProfileStatus,
            pub firmware: &'static str,
            pub minimum_battery_percent: u8,
            pub allowed_usb_modes: &'static [u32],
            pub required_operations: &'static [u16],
            pub required_properties: &'static [CameraPreflightProperty],
        }

        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub struct CameraFirmwareCapabilityProfile {
            pub firmware: &'static str,
            pub options: &'static [CameraOptionCapability],
            pub raw_conversion: Option<CameraRawConversionDescriptor>,
        }

        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum CameraRawConversionEvidenceStatus {
            Unverified,
            Observed,
            ReadVerified,
            WriteVerified,
        }

        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum CameraRawConversionState {
            Still,
            Movie,
        }

        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub struct CameraRawConversionLayout {
            pub profile_code: &'static str,
            pub header_padding: usize,
            pub declared_field_count: u16,
            pub total_length: usize,
            pub fields: &'static [&'static str],
        }

        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub struct CameraRawConversionDescriptor {
            pub id: &'static str,
            pub evidence_status: CameraRawConversionEvidenceStatus,
            pub evidence_manifests: &'static [&'static str],
            pub usb_modes: &'static [u32],
            pub camera_state: Option<CameraRawConversionState>,
            pub read: CameraRawConversionLayout,
            pub write: Option<CameraRawConversionLayout>,
        }

        impl CameraFirmwareCapabilityProfile {
            pub fn find_exact<'a>(
                profiles: &'a [Self],
                firmware: &str,
            ) -> Option<&'a Self> {
                profiles
                    .iter()
                    .find(|profile| profile.firmware == firmware)
            }

            pub fn option(&self, name: &str) -> Option<&'static CameraOptionCapability> {
                self.options
                    .iter()
                    .find(|capability| capability.option == name)
            }

            pub fn validate_option_value(
                &self,
                option: &str,
                logical: &str,
            ) -> anyhow::Result<()> {
                let capability = self.option(option).ok_or_else(|| {
                    anyhow::anyhow!(
                        "firmware {} has no capability profile for option {option}",
                        self.firmware,
                    )
                })?;
                if !capability.allowed_values.contains(&logical) {
                    anyhow::bail!(
                        "firmware {} does not allow {option}={logical}",
                        self.firmware,
                    );
                }

                Ok(())
            }

            pub fn write_wire_value(
                &self,
                option: &str,
                logical: &str,
            ) -> anyhow::Result<i32> {
                self.validate_option_value(option, logical)?;
                let capability = self.option(option).ok_or_else(|| {
                    anyhow::anyhow!(
                        "firmware {} has no capability profile for option {option}",
                        self.firmware,
                    )
                })?;
                capability
                    .wire_values
                    .iter()
                    .find(|value| value.logical_value == logical)
                    .and_then(|value| value.wire_values.first())
                    .copied()
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "firmware {} has no wire value for {option}={logical}",
                            self.firmware,
                        )
                    })
            }

            pub fn read_logical_value(
                &self,
                option: &str,
                wire: i32,
            ) -> anyhow::Result<&'static str> {
                let capability = self.option(option).ok_or_else(|| {
                    anyhow::anyhow!(
                        "firmware {} has no capability profile for option {option}",
                        self.firmware,
                    )
                })?;
                capability
                    .wire_values
                    .iter()
                    .find(|value| {
                        capability.allowed_values.contains(&value.logical_value)
                            && value.wire_values.contains(&wire)
                    })
                    .map(|value| value.logical_value)
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "firmware {} has no allowed logical value for {option} wire value {wire}",
                            self.firmware,
                        )
                    })
            }

            pub fn validate_raw_conversion_signature(
                &self,
                profile_code: u32,
                header_padding: usize,
                fields: &[&str],
                total_length: usize,
            ) -> anyhow::Result<()> {
                let descriptor = self.raw_conversion.as_ref().ok_or_else(|| {
                    anyhow::anyhow!(
                        "firmware {} has no RAW conversion descriptor",
                        self.firmware,
                    )
                })?;
                anyhow::ensure!(
                    descriptor.evidence_status == CameraRawConversionEvidenceStatus::WriteVerified
                        && !descriptor.evidence_manifests.is_empty(),
                    "firmware {} RAW conversion descriptor {} is not write-verified",
                    self.firmware,
                    descriptor.id,
                );
                anyhow::ensure!(
                    descriptor.camera_state.is_none(),
                    "firmware {} RAW conversion descriptor {} requires camera-state validation that this runtime cannot yet establish",
                    self.firmware,
                    descriptor.id,
                );
                let layout = descriptor.write.as_ref().ok_or_else(|| {
                    anyhow::anyhow!(
                        "firmware {} RAW conversion descriptor {} has no write layout",
                        self.firmware,
                        descriptor.id,
                    )
                })?;
                let profile_code_text = format!("{profile_code:x}");
                anyhow::ensure!(
                    layout.profile_code == profile_code_text
                        && layout.header_padding == header_padding
                        && usize::from(layout.declared_field_count) == fields.len()
                        && layout.fields == fields
                        && layout.total_length == total_length,
                    "firmware {} RAW conversion write descriptor {} does not match the generated codec or candidate payload",
                    self.firmware,
                    descriptor.id,
                );
                Ok(())
            }

            pub fn validate_raw_conversion_read_fingerprint(
                &self,
                profile_code: u32,
                header_padding: usize,
                declared_field_count: u16,
                fields: &[&str],
                total_length: usize,
            ) -> anyhow::Result<()> {
                let descriptor = self.raw_conversion.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("firmware {} has no RAW conversion descriptor", self.firmware)
                })?;
                anyhow::ensure!(
                    descriptor.evidence_status == CameraRawConversionEvidenceStatus::WriteVerified
                        && !descriptor.evidence_manifests.is_empty(),
                    "firmware {} RAW conversion descriptor {} is not write-verified",
                    self.firmware,
                    descriptor.id,
                );
                let profile_code_text = format!("{profile_code:x}");
                let layout = &descriptor.read;
                anyhow::ensure!(
                    layout.profile_code == profile_code_text
                        && layout.header_padding == header_padding
                        && layout.declared_field_count == declared_field_count
                        && layout.fields == fields
                        && layout.total_length == total_length,
                    "firmware {} RAW conversion read fingerprint does not match descriptor {}",
                    self.firmware,
                    descriptor.id,
                );
                Ok(())
            }
        }

        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub struct CameraOptionCapability {
            pub option: &'static str,
            pub allowed_values: &'static [&'static str],
            pub wire_values: &'static [CameraOptionWireValue],
        }

        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub struct CameraOptionWireValue {
            pub logical_value: &'static str,
            pub wire_values: &'static [i32],
        }

        #(#defs)*

        pub const SUPPORTED: &[crate::SupportedCamera] = &[
            #(#supported_entries,)*
        ];
    })
}

pub fn path() -> TokenStream {
    quote! { crate::generated::cameras }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn camera_with_preflight() -> Camera {
        serde_json::from_str(
            r#"{
                "id": "x_t5",
                "spec": {
                    "name": "FUJIFILM X-T5",
                    "generation": "x_trans_v",
                    "usb": { "vendor_id": 1227, "product_id": 764, "chunk_size_ceiling": 1024 },
                    "ptp": { "manufacturer": "FUJIFILM", "model": "X-T5" },
                    "preflight": [{
                        "operation": "raw_conversion",
                        "status": "verified",
                        "firmware": "4.31",
                        "minimum_battery_percent": 100,
                        "allowed_usb_modes": [6],
                        "required_operations": [4097, 4116, 4117],
                        "required_properties": [{ "code": 53635, "data_type": 4, "writable": true }]
                    }],
                    "capabilities": {
                        "generation": { "option_overrides": [{
                            "ref": "film_simulation",
                                "allowed_values": ["provia"],
                                "wire_values": { "provia": 1 }
                        }] },
                        "model": { "option_overrides": [{
                            "ref": "film_simulation",
                            "wire_values": { "reala_ace": 20 }
                        }] },
                        "firmware": {
                            "3.00": {},
                            "4.00": { "option_overrides": [{
                                    "ref": "film_simulation",
                                    "allowed_values": ["provia", "reala_ace"],
                                    "wire_values": { "reala_ace": [20, 21] }
                            }] }
                        }
                    }
                }
            }"#,
        )
        .unwrap()
    }

    #[test]
    fn camera_registry_defines_preflight_operation_kind() {
        let generated = generate(&BTreeMap::new()).unwrap().to_string();

        assert!(generated.contains("pub enum CameraPreflightOperation"));
    }

    #[test]
    fn camera_registry_defines_preflight_profile_contract() {
        let generated = generate(&BTreeMap::new()).unwrap().to_string();

        assert!(generated.contains("pub struct CameraPreflightProfile"));
    }

    #[test]
    fn camera_registry_emits_preflight_profiles() {
        let camera = camera_with_preflight();
        let generated = generate(&BTreeMap::from([(camera.id.clone(), camera)]))
            .unwrap()
            .to_string();

        assert!(generated.contains("preflight_profiles : & [CameraPreflightProfile"));
    }

    #[test]
    fn camera_registry_emits_exact_ptp_identity() {
        let camera = camera_with_preflight();
        let generated = generate(&BTreeMap::from([(camera.id.clone(), camera)]))
            .unwrap()
            .to_string();

        assert!(generated.contains("ptp_identity : Some (CameraPtpIdentity"));
    }

    #[test]
    fn camera_registry_emits_chunk_size_ceiling() {
        let camera = camera_with_preflight();
        let generated = generate(&BTreeMap::from([(camera.id.clone(), camera)]))
            .unwrap()
            .to_string();

        assert!(generated.contains("fn chunk_size_ceiling (& self) -> usize"));
        assert!(generated.contains("1024usize"));
    }

    #[test]
    fn camera_registry_defines_exact_firmware_capability_contract() {
        let generated = generate(&BTreeMap::new()).unwrap().to_string();

        assert!(generated.contains("pub struct CameraFirmwareCapabilityProfile"));
    }

    #[test]
    fn camera_registry_defines_raw_conversion_descriptor_contract() {
        let generated = generate(&BTreeMap::new()).unwrap().to_string();

        assert!(generated.contains("pub struct CameraRawConversionDescriptor"));
        assert!(generated.contains("pub struct CameraRawConversionLayout"));
        assert!(generated.contains("CameraRawConversionEvidenceStatus"));
    }

    #[test]
    fn firmware_capability_profile_owns_raw_conversion_descriptor() {
        let generated = generate(&BTreeMap::new()).unwrap().to_string();

        assert!(
            generated.contains("pub raw_conversion : Option < CameraRawConversionDescriptor >")
        );
    }

    #[test]
    fn firmware_profile_validates_raw_conversion_descriptor_exactly() {
        let generated = generate(&BTreeMap::new()).unwrap().to_string();

        assert!(generated.contains("validate_raw_conversion_signature"));
        assert!(generated.contains("validate_raw_conversion_read_fingerprint"));
        assert!(generated.contains("WriteVerified"));
    }

    #[test]
    fn camera_registry_emits_only_exact_resolved_firmware_profiles() {
        let camera = camera_with_preflight();
        let generated = generate(&BTreeMap::from([(camera.id.clone(), camera)]))
            .unwrap()
            .to_string();

        assert!(
            generated.contains("firmware_capability_profiles : & [CameraFirmwareCapabilityProfile")
        );
    }

    #[test]
    fn firmware_capability_lookup_is_exact() {
        let generated = generate(&BTreeMap::new()).unwrap().to_string();

        assert!(generated.contains("profile . firmware == firmware"));
    }

    #[test]
    fn firmware_capability_profile_exposes_fail_closed_option_codec() {
        let generated = generate(&BTreeMap::new()).unwrap().to_string();

        assert!(generated.contains("pub fn option"));
        assert!(generated.contains("pub fn validate_option_value"));
        assert!(generated.contains("pub fn write_wire_value"));
        assert!(generated.contains("pub fn read_logical_value"));
    }
}
