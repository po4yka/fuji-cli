use std::collections::{BTreeMap, BTreeSet};

use crate::ast::{
    Camera, CameraCapabilities, CapabilitySet, CapabilityWireValue, Field, FujiOption,
    PreflightOperation, PreflightStatus, RawConversionSignature, SpecKind,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FirmwareCapabilityProfile {
    pub firmware: String,
    pub options: BTreeMap<String, ResolvedOptionCapability>,
    pub raw_conversion: Option<RawConversionSignature>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ResolvedOptionCapability {
    pub allowed_values: Vec<String>,
    pub wire_values: BTreeMap<String, Vec<i32>>,
}

pub fn resolve_firmware_capabilities(
    capabilities: &CameraCapabilities,
) -> anyhow::Result<Vec<FirmwareCapabilityProfile>> {
    let mut base = BTreeMap::new();
    merge_capability_set(&mut base, &capabilities.generation);
    merge_capability_set(&mut base, &capabilities.model);
    let base_raw_conversion = capabilities
        .model
        .raw_conversion
        .clone()
        .or_else(|| capabilities.generation.raw_conversion.clone());

    let profiles = capabilities
        .firmware
        .iter()
        .map(|(firmware, overrides)| {
            let mut options = base.clone();
            merge_capability_set(&mut options, overrides);

            FirmwareCapabilityProfile {
                firmware: firmware.clone(),
                options,
                raw_conversion: overrides
                    .raw_conversion
                    .clone()
                    .or_else(|| base_raw_conversion.clone()),
            }
        })
        .collect::<Vec<_>>();
    for profile in &profiles {
        validate_profile(profile)?;
    }
    Ok(profiles)
}

pub fn validate_verified_profile_coverage(
    options: &BTreeMap<String, FujiOption>,
    cameras: &BTreeMap<String, Camera>,
) -> anyhow::Result<()> {
    for camera in cameras.values() {
        let verified = camera
            .spec
            .preflight
            .iter()
            .filter(|profile| profile.status == PreflightStatus::Verified)
            .collect::<Vec<_>>();
        if verified.is_empty() {
            continue;
        }
        let capabilities = camera.spec.capabilities.as_ref().ok_or_else(|| {
            anyhow::anyhow!(
                "camera {} has verified preflight profiles without firmware capabilities",
                camera.id,
            )
        })?;
        let profiles = resolve_firmware_capabilities(capabilities)?;

        for preflight in verified {
            let profile = profiles
                .iter()
                .find(|profile| profile.firmware == preflight.firmware)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "camera {} firmware {} has verified preflight without an exact capability profile",
                        camera.id,
                        preflight.firmware,
                    )
                })?;
            let required = required_enum_options(camera, preflight.operation);
            for option in required {
                let Some(definition) = options.get(option) else {
                    anyhow::bail!("camera {} references unknown option {option}", camera.id);
                };
                if definition.spec.kind() != SpecKind::Enum {
                    continue;
                }
                let capability = profile.options.get(option).ok_or_else(|| {
                    anyhow::anyhow!(
                        "camera {} firmware {} verified {:?} profile is missing enum capability {option}",
                        camera.id,
                        preflight.firmware,
                        preflight.operation,
                    )
                })?;
                anyhow::ensure!(
                    !capability.allowed_values.is_empty(),
                    "camera {} firmware {} enum capability {option} has no allowed values",
                    camera.id,
                    preflight.firmware,
                );
            }
            if preflight.operation == PreflightOperation::RawConversion {
                anyhow::ensure!(
                    profile.raw_conversion.is_some(),
                    "camera {} firmware {} verified RAW conversion lacks an exact wire signature",
                    camera.id,
                    preflight.firmware,
                );
            }
        }
    }
    Ok(())
}

fn required_enum_options(camera: &Camera, operation: PreflightOperation) -> BTreeSet<&str> {
    let mut required = BTreeSet::new();
    let features = camera.spec.features.as_ref();
    if matches!(
        operation,
        PreflightOperation::SimulationAccess | PreflightOperation::SimulationWrite
    ) && features
        .and_then(|features| features.simulation.as_ref())
        .is_some()
    {
        required.insert("custom_setting");
        if let Some(simulation) = features.and_then(|features| features.simulation.as_ref()) {
            required.extend(
                simulation
                    .settings
                    .iter()
                    .map(|setting| setting.r#ref.as_str()),
            );
        }
    }
    if operation == PreflightOperation::RawConversion
        && let Some(render) = features.and_then(|features| features.render.as_ref())
    {
        required.extend(render.fields.iter().filter_map(|field| match field {
            Field::Ref(field) => Some(field.r#ref.as_str()),
            Field::Inline(_) => None,
        }));
    }
    required
}

fn validate_profile(profile: &FirmwareCapabilityProfile) -> anyhow::Result<()> {
    for (option, capability) in &profile.options {
        let mut wire_owners = BTreeMap::<i32, &str>::new();
        for logical in &capability.allowed_values {
            let wires = capability.wire_values.get(logical).ok_or_else(|| {
                anyhow::anyhow!(
                    "firmware {} option {option} allows {logical} without a wire value",
                    profile.firmware
                )
            })?;
            anyhow::ensure!(
                !wires.is_empty(),
                "firmware {} option {option} has an empty wire set for {logical}",
                profile.firmware
            );
            for wire in wires {
                if let Some(owner) = wire_owners.insert(*wire, logical) {
                    anyhow::bail!(
                        "firmware {} option {option} wire value {wire} is ambiguous between {owner} and {logical}",
                        profile.firmware
                    );
                }
            }
        }
    }
    Ok(())
}

fn merge_capability_set(
    target: &mut BTreeMap<String, ResolvedOptionCapability>,
    overlay: &CapabilitySet,
) {
    for overlay_capability in &overlay.option_overrides {
        let capability = target.entry(overlay_capability.r#ref.clone()).or_default();
        if let Some(allowed_values) = &overlay_capability.allowed_values {
            capability.allowed_values.clone_from(allowed_values);
        }
        for (logical_value, wire_value) in &overlay_capability.wire_values {
            let wire_values = match wire_value {
                CapabilityWireValue::Single(value) => vec![*value],
                CapabilityWireValue::Multi(values) => values.clone(),
            };
            capability
                .wire_values
                .insert(logical_value.clone(), wire_values);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_firmware_profiles_merge_generation_model_and_firmware_layers() {
        let capabilities: CameraCapabilities = serde_json::from_str(
            r#"{
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
            }"#,
        )
        .unwrap();

        let profiles = resolve_firmware_capabilities(&capabilities).unwrap();

        assert_eq!(
            profiles,
            vec![
                FirmwareCapabilityProfile {
                    firmware: "3.00".to_owned(),
                    raw_conversion: None,
                    options: BTreeMap::from([(
                        "film_simulation".to_owned(),
                        ResolvedOptionCapability {
                            allowed_values: vec!["provia".to_owned()],
                            wire_values: BTreeMap::from([
                                ("provia".to_owned(), vec![1]),
                                ("reala_ace".to_owned(), vec![20]),
                            ]),
                        },
                    )]),
                },
                FirmwareCapabilityProfile {
                    firmware: "4.00".to_owned(),
                    raw_conversion: None,
                    options: BTreeMap::from([(
                        "film_simulation".to_owned(),
                        ResolvedOptionCapability {
                            allowed_values: vec!["provia".to_owned(), "reala_ace".to_owned()],
                            wire_values: BTreeMap::from([
                                ("provia".to_owned(), vec![1]),
                                ("reala_ace".to_owned(), vec![20, 21]),
                            ]),
                        },
                    )]),
                },
            ],
        );
    }

    #[test]
    fn duplicate_allowed_wire_values_are_rejected() {
        let capabilities: CameraCapabilities = serde_json::from_str(
            r#"{
                "generation": { "option_overrides": [{
                    "ref": "film_simulation",
                    "allowed_values": ["provia", "velvia"],
                    "wire_values": { "provia": 1, "velvia": 1 }
                }] },
                "model": {},
                "firmware": { "4.31": {} }
            }"#,
        )
        .unwrap();

        let error = resolve_firmware_capabilities(&capabilities)
            .expect_err("ambiguous firmware wire values must fail code generation");

        assert!(error.to_string().contains("wire value 1"));
    }

    #[test]
    fn allowed_logical_value_without_wire_encoding_is_rejected() {
        let capabilities: CameraCapabilities = serde_json::from_str(
            r#"{
                "generation": { "option_overrides": [{
                    "ref": "film_simulation",
                    "allowed_values": ["provia"],
                    "wire_values": {}
                }] },
                "model": {},
                "firmware": { "4.31": {} }
            }"#,
        )
        .unwrap();

        let error = resolve_firmware_capabilities(&capabilities)
            .expect_err("every allowed logical value needs an exact firmware wire encoding");

        assert!(error.to_string().contains("provia"));
    }

    #[test]
    fn verified_profile_requires_every_enum_consumed_by_the_operation() {
        let custom_setting: FujiOption = serde_json::from_str(
            r#"{
                "id": "custom_setting",
                "spec": {
                    "name": "Slot", "kind": "enum",
                    "rules": { "variants": [{ "id": "c1", "name": "C1", "aliases": [] }] },
                    "encoding": { "kind": "lookup", "prop_code": 53644, "spec": { "values": { "c1": 1 } } }
                }
            }"#,
        )
        .unwrap();
        let file_type: FujiOption = serde_json::from_str(
            r#"{
                "id": "file_type",
                "spec": {
                    "name": "File type", "kind": "enum",
                    "rules": { "variants": [{ "id": "jpeg", "name": "JPEG", "aliases": [] }] },
                    "encoding": { "kind": "lookup", "prop_code": 53645, "spec": { "values": { "jpeg": 7 } } }
                }
            }"#,
        )
        .unwrap();
        let camera: Camera = serde_json::from_str(
            r#"{
                "id": "fixture",
                "spec": {
                    "name": "Fixture", "generation": "fixture",
                    "usb": { "vendor_id": 1227, "product_id": 1, "chunk_size_ceiling": 1024 },
                    "preflight": [{
                        "operation": "simulation_access", "status": "verified", "firmware": "4.31",
                        "minimum_battery_percent": 100, "allowed_usb_modes": [6],
                        "required_operations": [4097], "required_properties": []
                    }],
                    "capabilities": {
                        "generation": { "option_overrides": [{
                            "ref": "custom_setting", "allowed_values": ["c1"],
                            "wire_values": { "c1": 1 }
                        }] },
                        "model": {}, "firmware": { "4.31": {} }
                    },
                    "features": { "simulation": {
                        "slots": 1, "settings": [{ "id": "file_type", "ref": "file_type" }]
                    } }
                }
            }"#,
        )
        .unwrap();
        let error = validate_verified_profile_coverage(
            &BTreeMap::from([
                (custom_setting.id.clone(), custom_setting),
                (file_type.id.clone(), file_type),
            ]),
            &BTreeMap::from([(camera.id.clone(), camera)]),
        )
        .expect_err("verified paths must not fall back for an unprofiled enum");

        assert!(error.to_string().contains("file_type"));
    }

    #[test]
    fn raw_conversion_requires_only_render_profile_enum_options() {
        let camera: Camera = serde_json::from_str(
            r#"{
                "id": "fixture",
                "spec": {
                    "name": "Fixture", "generation": "fixture",
                    "usb": { "vendor_id": 1227, "product_id": 1, "chunk_size_ceiling": 1024 },
                    "features": {
                        "simulation": {
                            "slots": 1,
                            "settings": [{ "id": "simulation_only", "ref": "simulation_only" }]
                        },
                        "render": {
                            "profile_code": 1,
                            "header_padding": 2,
                            "fields": [{ "id": "render_format", "ref": "render_format" }]
                        }
                    }
                }
            }"#,
        )
        .unwrap();

        assert_eq!(
            required_enum_options(&camera, PreflightOperation::RawConversion),
            BTreeSet::from(["render_format"]),
        );
    }
}
