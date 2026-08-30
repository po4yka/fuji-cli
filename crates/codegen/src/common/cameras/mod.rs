use std::collections::BTreeMap;

use proc_macro2::{Literal, TokenStream};
use quote::quote;

use crate::{
    ast::{Camera, PreflightOperation, PreflightStatus},
    util::ident::{safe_upper_camel_case_ident, safe_uppercase_ident},
};

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
        let chunk_size = Literal::usize_suffixed(camera.spec.usb.chunk_size.try_into()?);
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
                PreflightOperation::SimulationWrite => {
                    quote! { CameraPreflightOperation::SimulationWrite }
                }
                PreflightOperation::RawConversion => {
                    quote! { CameraPreflightOperation::RawConversion }
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
                camera_factory: || Box::new(#struct_name),
            };

            impl crate::features::base::CameraBase for #struct_name {
                type Context = rusb::GlobalContext;

                fn camera_definition(&self) -> &'static crate::SupportedCamera {
                    &#const_name
                }

                fn chunk_size(&self) -> usize {
                    #chunk_size
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
            SimulationWrite,
            RawConversion,
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
                    "usb": { "vendor_id": 1227, "product_id": 764, "chunk_size": 1024 },
                    "ptp": { "manufacturer": "FUJIFILM", "model": "X-T5" },
                    "preflight": [{
                        "operation": "raw_conversion",
                        "status": "verified",
                        "firmware": "4.31",
                        "minimum_battery_percent": 100,
                        "allowed_usb_modes": [6],
                        "required_operations": [4097, 4116, 4117],
                        "required_properties": [{ "code": 53635, "data_type": 4, "writable": true }]
                    }]
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
}
