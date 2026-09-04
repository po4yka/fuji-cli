//! Consistency checks for `features.simulation.settings` declarations.
//!
//! CUE indexes `options[setting.ref]` to derive each setting's grammar
//! bucket, so a `ref` naming a nonexistent option already fails the CUE
//! export. What CUE does not check is that the referenced option is a PTP
//! device property: an option with no `prop_code` compiles fine as FML but
//! only fails once the generated
//! `<Type as crate::ptp::option::SimulationSetting>::prop_code()` call does
//! not exist, which surfaces as an opaque Rust compile error in the
//! generated crate instead of a codegen error naming the camera, setting id,
//! and ref. This pass closes that gap.
//!
//! CUE's `_validation.ids` already forces setting ids to be unique within one
//! camera (`fml/camera.cue` `#Simulation._validation`), so this pass does not
//! repeat that check. CUE does not constrain `ref` uniqueness, so two
//! settings could name the same option; this pass rejects that too, since a
//! duplicate ref would give one PTP property two independent generated
//! setting identities.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{bail, ensure};

use crate::ast::{Camera, FujiOption};

/// Every simulation setting's `ref` must name an option that carries a PTP
/// `prop_code`, and no two settings within one camera may share a `ref`.
pub fn validate_simulation_setting_refs(
    options: &BTreeMap<String, FujiOption>,
    cameras: &BTreeMap<String, Camera>,
) -> anyhow::Result<()> {
    for camera in cameras.values() {
        let Some(simulation) = camera
            .spec
            .features
            .as_ref()
            .and_then(|features| features.simulation.as_ref())
        else {
            continue;
        };

        let mut seen_refs = BTreeSet::new();
        for setting in &simulation.settings {
            let Some(option) = options.get(&setting.r#ref) else {
                bail!(
                    "camera `{}` simulation setting `{}` refs unknown option `{}`",
                    camera.id,
                    setting.id,
                    setting.r#ref
                );
            };
            ensure!(
                option.spec.prop_code().is_some(),
                "camera `{}` simulation setting `{}` refs option `{}`, which has no prop_code",
                camera.id,
                setting.id,
                setting.r#ref
            );
            ensure!(
                seen_refs.insert(setting.r#ref.clone()),
                "camera `{}` simulation setting `{}` refs option `{}`, which another setting already refs",
                camera.id,
                setting.id,
                setting.r#ref
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::validate_simulation_setting_refs;
    use crate::ast::{Camera, FujiOption};

    fn option(json: &str) -> (String, FujiOption) {
        let option: FujiOption = serde_json::from_str(json).expect("option must parse");
        (option.id.clone(), option)
    }

    fn camera(settings: &str) -> BTreeMap<String, Camera> {
        let camera: Camera = serde_json::from_str(&format!(
            r#"{{
                "id": "demo",
                "spec": {{
                    "name": "Demo",
                    "generation": "gen_a",
                    "usb": {{ "vendor_id": 1227, "product_id": 764, "chunk_size_ceiling": 1024 }},
                    "features": {{
                        "simulation": {{
                            "slots": 1,
                            "settings": {settings}
                        }}
                    }}
                }}
            }}"#
        ))
        .expect("camera must parse");
        BTreeMap::from([(camera.id.clone(), camera)])
    }

    const WITH_PROP_CODE: &str = r#"{ "id": "color", "spec": { "name": "Color", "kind": "integer",
        "rules": { "min": -4, "max": 4, "step": 1 },
        "encoding": { "kind": "scale", "prop_code": 53663, "data_type": 3, "spec": { "scale": 10 } } } }"#;

    const WITHOUT_PROP_CODE: &str = r#"{ "id": "slot_only", "spec": { "name": "Slot", "kind": "integer",
        "rules": { "min": 0, "max": 4, "step": 1 },
        "encoding": { "kind": "raw" } } }"#;

    #[test]
    fn a_setting_ref_to_an_option_with_a_prop_code_passes() {
        let options = BTreeMap::from([option(WITH_PROP_CODE)]);
        let cameras = camera(r#"[{ "id": "color", "ref": "color" }]"#);

        validate_simulation_setting_refs(&options, &cameras)
            .expect("a ref to a PTP-backed option must pass");
    }

    #[test]
    fn a_setting_ref_to_an_unknown_option_fails() {
        let options = BTreeMap::new();
        let cameras = camera(r#"[{ "id": "color", "ref": "color" }]"#);

        let error = validate_simulation_setting_refs(&options, &cameras)
            .expect_err("a ref to a missing option must fail the build");
        let message = format!("{error:#}");
        assert!(message.contains("demo"), "{message}");
        assert!(message.contains("color"), "{message}");
    }

    #[test]
    fn a_setting_ref_to_an_option_without_a_prop_code_fails() {
        let options = BTreeMap::from([option(WITHOUT_PROP_CODE)]);
        let cameras = camera(r#"[{ "id": "slot", "ref": "slot_only" }]"#);

        let error = validate_simulation_setting_refs(&options, &cameras)
            .expect_err("a ref to a non-PTP option must fail the build");
        let message = format!("{error:#}");
        assert!(message.contains("demo"), "{message}");
        assert!(message.contains("slot"), "{message}");
        assert!(message.contains("slot_only"), "{message}");
    }

    #[test]
    fn two_settings_sharing_a_ref_within_one_camera_fail() {
        let options = BTreeMap::from([option(WITH_PROP_CODE)]);
        let cameras =
            camera(r#"[{ "id": "color_a", "ref": "color" }, { "id": "color_b", "ref": "color" }]"#);

        let error = validate_simulation_setting_refs(&options, &cameras)
            .expect_err("two settings refing the same option must fail the build");
        let message = format!("{error:#}");
        assert!(message.contains("demo"), "{message}");
        assert!(message.contains("color_b"), "{message}");
    }

    #[test]
    fn a_camera_without_simulation_needs_no_checks() {
        let options = BTreeMap::new();
        let camera: Camera = serde_json::from_str(
            r#"{
                "id": "no_sim",
                "spec": {
                    "name": "NoSim",
                    "generation": "gen_a",
                    "usb": { "vendor_id": 1227, "product_id": 764, "chunk_size_ceiling": 1024 }
                }
            }"#,
        )
        .expect("camera must parse");
        let cameras = BTreeMap::from([(camera.id.clone(), camera)]);

        validate_simulation_setting_refs(&options, &cameras)
            .expect("a camera with no simulation feature has nothing to check");
    }
}
