use std::collections::{BTreeMap, BTreeSet};

use crate::ast::{
    Camera, CameraCapabilities, CapabilitySet, CapabilityWireValue, Field, FujiOption,
    PreflightOperation, PreflightStatus, RawConversionDescriptor, RawConversionEvidenceStatus,
    RawConversionLayout, Render, SpecKind,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FirmwareCapabilityProfile {
    pub firmware: String,
    pub options: BTreeMap<String, ResolvedOptionCapability>,
    pub raw_conversion: Option<RawConversionDescriptor>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ResolvedOptionCapability {
    pub allowed_values: Vec<String>,
    pub wire_values: BTreeMap<String, Vec<i32>>,
}

/// `render` is the camera's render feature declaration, which every RAW
/// conversion descriptor is cross-checked against.
pub fn resolve_firmware_capabilities(
    capabilities: &CameraCapabilities,
    render: Option<&Render>,
) -> anyhow::Result<Vec<FirmwareCapabilityProfile>> {
    let mut base = BTreeMap::new();
    merge_capability_set(&mut base, &capabilities.generation);
    merge_capability_set(&mut base, &capabilities.model);
    let profiles = capabilities
        .firmware
        .iter()
        .map(|(firmware, overrides)| {
            let mut options = base.clone();
            merge_capability_set(&mut options, overrides);

            FirmwareCapabilityProfile {
                firmware: firmware.clone(),
                options,
                raw_conversion: overrides.raw_conversion.clone(),
            }
        })
        .collect::<Vec<_>>();
    for profile in &profiles {
        validate_profile(profile, render)?;
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
        let render = camera
            .spec
            .features
            .as_ref()
            .and_then(|features| features.render.as_ref());
        let profiles = resolve_firmware_capabilities(capabilities, render)?;

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
                let descriptor = profile.raw_conversion.as_ref().ok_or_else(|| {
                    anyhow::anyhow!(
                        "camera {} firmware {} verified RAW conversion lacks an exact wire descriptor",
                        camera.id,
                        preflight.firmware,
                    )
                })?;
                anyhow::ensure!(
                    descriptor.evidence.status == RawConversionEvidenceStatus::WriteVerified,
                    "camera {} firmware {} verified RAW conversion descriptor {} lacks write-verified evidence",
                    camera.id,
                    preflight.firmware,
                    descriptor.id,
                );
                anyhow::ensure!(
                    descriptor.binding.usb_modes == preflight.allowed_usb_modes,
                    "camera {} firmware {} RAW descriptor {} USB modes do not match preflight",
                    camera.id,
                    preflight.firmware,
                    descriptor.id,
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

fn validate_profile(
    profile: &FirmwareCapabilityProfile,
    render: Option<&Render>,
) -> anyhow::Result<()> {
    if let Some(descriptor) = &profile.raw_conversion {
        validate_raw_conversion_descriptor(&profile.firmware, descriptor, render)?;
    }
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

fn validate_raw_conversion_descriptor(
    firmware: &str,
    descriptor: &RawConversionDescriptor,
    render: Option<&Render>,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        !descriptor.id.trim().is_empty(),
        "firmware {firmware} RAW conversion descriptor id is empty"
    );
    anyhow::ensure!(
        !descriptor.binding.usb_modes.is_empty(),
        "firmware {firmware} RAW conversion descriptor {} has no USB mode binding",
        descriptor.id,
    );
    validate_raw_conversion_layout(firmware, &descriptor.id, "read", &descriptor.read)?;
    if let Some(write) = &descriptor.write {
        validate_raw_conversion_layout(firmware, &descriptor.id, "write", write)?;
    }
    let render = render.ok_or_else(|| {
        anyhow::anyhow!(
            "firmware {firmware} RAW conversion descriptor {} needs the camera's render feature declaration to check against",
            descriptor.id,
        )
    })?;
    validate_raw_conversion_layout_against_render(
        firmware,
        &descriptor.id,
        "read",
        &descriptor.read,
        render,
        |field| !field.skip_read(),
    )?;
    if let Some(write) = &descriptor.write {
        validate_raw_conversion_layout_against_render(
            firmware,
            &descriptor.id,
            "write",
            write,
            render,
            |field| !field.skip_write(),
        )?;
    }
    if descriptor.evidence.status == RawConversionEvidenceStatus::WriteVerified {
        anyhow::bail!(
            "firmware {firmware} RAW conversion descriptor {} requests write_verified, but RAW writes remain unavailable until evidence manifests, camera state, and lossless wire preservation are machine-checked",
            descriptor.id,
        );
    }
    Ok(())
}

/// Cross-checks a capability layout against the camera's render feature, the
/// other place the same wire facts are declared. The generated codec derives
/// its profile code, header padding, and slot order from the render feature,
/// while preflight validates live bytes against this descriptor; if the two
/// disagree the read fingerprint can never pass. The wire field count
/// (`declared_field_count`, PTP `n_props`) counts every render slot in both
/// directions; the per-direction field list omits the slots that are
/// `skip_read` or `skip_write`.
fn validate_raw_conversion_layout_against_render(
    firmware: &str,
    descriptor_id: &str,
    direction: &str,
    layout: &RawConversionLayout,
    render: &Render,
    in_direction: impl Fn(&Field) -> bool,
) -> anyhow::Result<()> {
    let render_profile_code = format!("{:x}", render.profile_code);
    anyhow::ensure!(
        layout.profile_code == render_profile_code,
        "firmware {firmware} RAW conversion descriptor {descriptor_id} {direction} profile code {} differs from the render feature's {render_profile_code}",
        layout.profile_code,
    );
    anyhow::ensure!(
        layout.header_padding == render.header_padding,
        "firmware {firmware} RAW conversion descriptor {descriptor_id} {direction} header padding {:#x} differs from the render feature's {:#x}",
        layout.header_padding,
        render.header_padding,
    );
    anyhow::ensure!(
        usize::from(layout.declared_field_count) == render.fields.len(),
        "firmware {firmware} RAW conversion descriptor {descriptor_id} {direction} declares {} wire fields, but the render feature has {} slots",
        layout.declared_field_count,
        render.fields.len(),
    );
    let expected_fields = render
        .fields
        .iter()
        .filter(|field| in_direction(field))
        .map(|field| field.id().to_owned())
        .collect::<Vec<_>>();
    anyhow::ensure!(
        layout.fields == expected_fields,
        "firmware {firmware} RAW conversion descriptor {descriptor_id} {direction} field list {:?} differs from the render feature's {direction} slots {expected_fields:?}",
        layout.fields,
    );
    Ok(())
}

/// Self-consistency only: every number here comes from the same FML layout,
/// so this proves the declaration adds up, not that the camera agrees.
/// Device agreement is the evidence status, and
/// [`validate_raw_conversion_layout_against_render`] ties the layout to the
/// render feature the generated codec is built from.
fn validate_raw_conversion_layout(
    firmware: &str,
    descriptor_id: &str,
    direction: &str,
    layout: &RawConversionLayout,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        !layout.profile_code.is_empty()
            && layout.profile_code.len() <= 8
            && layout
                .profile_code
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        "firmware {firmware} RAW conversion descriptor {descriptor_id} {direction} profile code is not canonical lowercase hexadecimal text",
    );
    u32::from_str_radix(&layout.profile_code, 16).map_err(|error| {
        anyhow::anyhow!(
            "firmware {firmware} RAW conversion descriptor {descriptor_id} {direction} profile code cannot be represented by the generated u32 codec: {error}"
        )
    })?;
    anyhow::ensure!(
        layout.total_length > layout.header_padding,
        "firmware {firmware} RAW conversion descriptor {descriptor_id} {direction} total length is invalid",
    );
    let unique = layout.fields.iter().collect::<BTreeSet<_>>();
    anyhow::ensure!(
        unique.len() == layout.fields.len(),
        "firmware {firmware} RAW conversion descriptor {descriptor_id} {direction} has duplicate fields",
    );
    let expected_total_length = 2_u64
        + 1
        + u64::try_from(layout.profile_code.len())? * 2
        + u64::from(layout.header_padding)
        + u64::try_from(layout.fields.len())? * 4;
    anyhow::ensure!(
        u64::from(layout.total_length) == expected_total_length,
        "firmware {firmware} RAW conversion descriptor {descriptor_id} {direction} total length {} does not match its exact layout length {expected_total_length}",
        layout.total_length,
    );
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

        let profiles = resolve_firmware_capabilities(&capabilities, None).unwrap();

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

        let error = resolve_firmware_capabilities(&capabilities, None)
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

        let error = resolve_firmware_capabilities(&capabilities, None)
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

    #[test]
    fn verified_raw_conversion_rejects_self_declared_signature_without_write_evidence() {
        let camera: Camera = serde_json::from_str(
            r#"{
                "id": "fixture",
                "spec": {
                    "name": "Fixture", "generation": "fixture",
                    "usb": { "vendor_id": 1227, "product_id": 1, "chunk_size_ceiling": 1024 },
                    "preflight": [{
                        "operation": "raw_conversion", "status": "verified", "firmware": "4.31",
                        "minimum_battery_percent": 100, "allowed_usb_modes": [6],
                        "required_operations": [4097], "required_properties": []
                    }],
                    "capabilities": {
                        "generation": {}, "model": {},
                        "firmware": { "4.31": { "raw_conversion": {
                            "id": "self-declared",
                            "evidence": { "status": "unverified", "manifests": [] },
                            "binding": { "usb_modes": [6] },
                            "read": {
                                "profile_code": "1", "header_padding": 2,
                                "declared_field_count": 1, "total_length": 11,
                                "fields": ["opaque"]
                            },
                            "write": {
                                "profile_code": "1", "header_padding": 2,
                                "declared_field_count": 1, "total_length": 11,
                                "fields": ["opaque"]
                            }
                        } } }
                    },
                    "features": { "render": {
                        "profile_code": 1, "header_padding": 2, "fields": [{ "id": "opaque" }]
                    } }
                }
            }"#,
        )
        .unwrap();

        let validation = validate_verified_profile_coverage(
            &BTreeMap::new(),
            &BTreeMap::from([(camera.id.clone(), camera)]),
        );

        assert!(
            validation.is_err(),
            "a self-declared RAW signature without independent write evidence must not authorize a verified profile"
        );
    }

    #[test]
    fn write_verified_raw_descriptor_is_reserved_until_evidence_is_machine_checked() {
        let descriptor: RawConversionDescriptor = serde_json::from_str(
            r#"{
                "id": "self-declared",
                "evidence": { "status": "write_verified", "manifests": ["fixture.json"] },
                "binding": { "usb_modes": [6], "camera_state": "still" },
                "read": {
                    "profile_code": "1", "header_padding": 2,
                    "declared_field_count": 1, "total_length": 11,
                    "fields": ["opaque"]
                },
                "write": {
                    "profile_code": "1", "header_padding": 2,
                    "declared_field_count": 1, "total_length": 11,
                    "fields": ["opaque"]
                }
            }"#,
        )
        .unwrap();

        let render: Render = serde_json::from_str(
            r#"{ "profile_code": 1, "header_padding": 2, "fields": [{ "id": "opaque" }] }"#,
        )
        .unwrap();

        let error = validate_raw_conversion_descriptor("4.31", &descriptor, Some(&render))
            .expect_err("a manifest path alone must never authorize RAW writes");

        assert!(error.to_string().contains("machine-checked"));
    }

    #[test]
    fn raw_descriptor_must_agree_with_the_render_feature_it_is_checked_against() {
        let descriptor: RawConversionDescriptor = serde_json::from_str(
            r#"{
                "id": "layout",
                "evidence": { "status": "unverified", "manifests": [] },
                "binding": { "usb_modes": [6], "camera_state": "still" },
                "read": {
                    "profile_code": "ff179502", "header_padding": 494,
                    "declared_field_count": 2, "total_length": 517,
                    "fields": ["head_0"]
                },
                "write": {
                    "profile_code": "ff179502", "header_padding": 494,
                    "declared_field_count": 2, "total_length": 521,
                    "fields": ["head_0", "tail_0"]
                }
            }"#,
        )
        .unwrap();
        let render = |profile_code: u32| -> Render {
            serde_json::from_str(&format!(
                r#"{{ "profile_code": {profile_code}, "header_padding": 494,
                     "fields": [{{ "id": "head_0" }}, {{ "id": "tail_0", "skip_read": true }}] }}"#
            ))
            .unwrap()
        };

        validate_raw_conversion_descriptor("4.31", &descriptor, Some(&render(0xff17_9502)))
            .expect("a descriptor that mirrors the render feature is consistent");

        let error =
            validate_raw_conversion_descriptor("4.31", &descriptor, Some(&render(0xff12_9504)))
                .expect_err("the descriptor's profile code must match the render feature's");
        assert!(error.to_string().contains("ff129504"), "{error}");

        let error = validate_raw_conversion_descriptor("4.31", &descriptor, None)
            .expect_err("a descriptor without a render feature has nothing to check against");
        assert!(error.to_string().contains("render feature"), "{error}");
    }
}
