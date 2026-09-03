//! Consistency checks for preflight `static_descriptor` declarations.
//!
//! CUE already forces a static descriptor to pin `data_type`, to be
//! `writable`, and to use `form: none` for strings. This pass adds the check
//! CUE cannot express: when the property code belongs to an option, the
//! pinned datatype must be the one the option's `SimulationSetting` codec
//! actually puts on the wire, so a drift between the two layers fails the
//! build instead of reaching a camera.

use std::collections::BTreeMap;

use anyhow::{Context, bail, ensure};

use crate::{
    ast::{Camera, FujiOption, LookupValue, NumericEncoding, OptionSpec, StaticForm},
    common::options::common::{resolve_enum_repr_signed, resolve_numeric_repr_signed},
};

const PTP_INT16: u16 = 0x0003;
const PTP_UINT16: u16 = 0x0004;
const PTP_STRING: u16 = 0xFFFF;

pub fn validate_static_descriptors(
    options: &BTreeMap<String, FujiOption>,
    cameras: &BTreeMap<String, Camera>,
) -> anyhow::Result<()> {
    let options_by_code: BTreeMap<u16, &FujiOption> = options
        .values()
        .filter_map(|option| option.spec.prop_code().map(|code| (code, option)))
        .collect();

    for camera in cameras.values() {
        for profile in &camera.spec.preflight {
            for property in &profile.required_properties {
                let Some(descriptor) = &property.static_descriptor else {
                    continue;
                };
                let context = || {
                    format!(
                        "camera {} firmware {} {:?} static descriptor for 0x{:04x}",
                        camera.id, profile.firmware, profile.operation, property.code
                    )
                };
                let Some(data_type) = property.data_type else {
                    bail!("{} must pin data_type", context());
                };
                ensure!(property.writable, "{} must be writable", context());
                validate_form(data_type, &descriptor.form).with_context(context)?;
                if let Some(option) = options_by_code.get(&property.code) {
                    let wire = option_wire_data_type(option)
                        .with_context(|| format!("option `{}`", option.id))
                        .with_context(context)?;
                    ensure!(
                        wire == data_type,
                        "{} pins datatype 0x{data_type:04x} but option `{}` writes datatype 0x{wire:04x}",
                        context(),
                        option.id
                    );
                }
            }
        }
    }
    Ok(())
}

fn validate_form(data_type: u16, form: &StaticForm) -> anyhow::Result<()> {
    match form {
        StaticForm::None => Ok(()),
        StaticForm::Enumeration { values } => {
            ensure!(data_type != PTP_STRING, "strings cannot carry a form");
            ensure!(!values.is_empty(), "enumeration form has no values");
            Ok(())
        }
        StaticForm::Range {
            minimum,
            maximum,
            step,
        } => {
            ensure!(data_type != PTP_STRING, "strings cannot carry a form");
            ensure!(*step > 0, "range step must be positive");
            ensure!(minimum <= maximum, "range minimum exceeds its maximum");
            Ok(())
        }
    }
}

/// The PTP datatype the option's generated `SimulationSetting` codec writes:
/// a PTP string for string options, otherwise the 16-bit scalar whose
/// signedness the option emitters derive from the wire values.
fn option_wire_data_type(option: &FujiOption) -> anyhow::Result<u16> {
    let signed = match &option.spec {
        OptionSpec::String { .. } => return Ok(PTP_STRING),
        OptionSpec::Enum { encoding, .. } => {
            let crate::ast::EnumEncoding::Lookup { spec, .. } = encoding;
            resolve_enum_repr_signed(&lookup_wire_values(&spec.values))?
        }
        OptionSpec::Integer {
            rules, encoding, ..
        } => match encoding {
            NumericEncoding::Lookup { spec, .. } => {
                resolve_enum_repr_signed(&lookup_wire_values(&spec.values))?
            }
            NumericEncoding::Raw { .. } | NumericEncoding::Scale { .. } => {
                let rules = rules.as_ref();
                let min = rules.and_then(|rules| rules.min).unwrap_or(i32::MIN);
                let max = rules.and_then(|rules| rules.max).unwrap_or(i32::MAX);
                let scale = match encoding {
                    NumericEncoding::Scale { spec, .. } => spec.scale,
                    _ => 1,
                };
                resolve_numeric_repr_signed(min.saturating_mul(scale), max.saturating_mul(scale))?
            }
        },
        OptionSpec::Float {
            rules, encoding, ..
        } => match encoding {
            NumericEncoding::Lookup { spec, .. } => {
                resolve_enum_repr_signed(&lookup_wire_values(&spec.values))?
            }
            NumericEncoding::Raw { .. } | NumericEncoding::Scale { .. } => {
                let rules = rules.as_ref();
                let min = rules.and_then(|rules| rules.min).unwrap_or(f32::MIN);
                let max = rules.and_then(|rules| rules.max).unwrap_or(f32::MAX);
                let scale = match encoding {
                    NumericEncoding::Scale { spec, .. } => spec.scale,
                    _ => 1,
                } as f32;
                resolve_numeric_repr_signed(
                    (min * scale).round() as i32,
                    (max * scale).round() as i32,
                )?
            }
        },
    };
    Ok(if signed { PTP_INT16 } else { PTP_UINT16 })
}

fn lookup_wire_values(values: &BTreeMap<String, LookupValue>) -> Vec<i32> {
    values
        .values()
        .flat_map(|value| match value {
            LookupValue::Single(value) => vec![*value],
            LookupValue::Multi(values) => values.clone(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::validate_static_descriptors;
    use crate::ast::{Camera, FujiOption};

    fn option(json: &str) -> (String, FujiOption) {
        let option: FujiOption = serde_json::from_str(json).expect("option must parse");
        (option.id.clone(), option)
    }

    fn camera(property: &str) -> BTreeMap<String, Camera> {
        let camera: Camera = serde_json::from_str(&format!(
            r#"{{
                "id": "demo",
                "spec": {{
                    "name": "Demo",
                    "generation": "gen_a",
                    "usb": {{ "vendor_id": 1227, "product_id": 764, "chunk_size_ceiling": 1024 }},
                    "preflight": [{{
                        "operation": "simulation_write",
                        "status": "unverified",
                        "firmware": "4.31",
                        "minimum_battery_percent": 100,
                        "allowed_usb_modes": [6],
                        "required_operations": [4097],
                        "required_properties": [{property}]
                    }}]
                }}
            }}"#
        ))
        .expect("camera must parse");
        BTreeMap::from([(camera.id.clone(), camera)])
    }

    const SIGNED_SCALE: &str = r#"{ "id": "color", "spec": { "name": "Color", "kind": "integer",
        "rules": { "min": -4, "max": 4, "step": 1 },
        "encoding": { "kind": "scale", "prop_code": 53663, "spec": { "scale": 10 } } } }"#;

    #[test]
    fn static_descriptor_matching_the_option_wire_type_passes() {
        let options = BTreeMap::from([option(SIGNED_SCALE)]);
        let cameras = camera(
            r#"{ "code": 53663, "data_type": 3, "writable": true,
                 "static_descriptor": { "evidence": "test", "form": { "kind": "none" } } }"#,
        );

        validate_static_descriptors(&options, &cameras).expect("INT16 matches a signed scale");
    }

    #[test]
    fn static_descriptor_with_the_wrong_signedness_fails_the_build() {
        let options = BTreeMap::from([option(SIGNED_SCALE)]);
        let cameras = camera(
            r#"{ "code": 53663, "data_type": 4, "writable": true,
                 "static_descriptor": { "evidence": "test", "form": { "kind": "none" } } }"#,
        );

        let error = validate_static_descriptors(&options, &cameras)
            .expect_err("UINT16 must be rejected for a signed scale option");

        assert!(format!("{error:#}").contains("0x0003"), "{error:#}");
    }

    #[test]
    fn static_descriptor_without_a_datatype_or_writability_is_rejected() {
        let options = BTreeMap::new();
        let untyped = camera(
            r#"{ "code": 53644, "writable": true,
                 "static_descriptor": { "evidence": "test", "form": { "kind": "none" } } }"#,
        );
        let read_only = camera(
            r#"{ "code": 53644, "data_type": 4, "writable": false,
                 "static_descriptor": { "evidence": "test", "form": { "kind": "none" } } }"#,
        );

        assert!(validate_static_descriptors(&options, &untyped).is_err());
        assert!(validate_static_descriptors(&options, &read_only).is_err());
    }

    #[test]
    fn string_static_descriptor_cannot_carry_an_enumeration() {
        let options = BTreeMap::new();
        let cameras = camera(
            r#"{ "code": 53645, "data_type": 65535, "writable": true,
                 "static_descriptor": { "evidence": "test",
                     "form": { "kind": "enumeration", "values": [1] } } }"#,
        );

        assert!(validate_static_descriptors(&options, &cameras).is_err());
    }

    #[test]
    fn enumeration_and_range_forms_are_validated() {
        let options = BTreeMap::new();
        let empty_enum = camera(
            r#"{ "code": 53644, "data_type": 4, "writable": true,
                 "static_descriptor": { "evidence": "test",
                     "form": { "kind": "enumeration", "values": [] } } }"#,
        );
        let bad_range = camera(
            r#"{ "code": 53644, "data_type": 4, "writable": true,
                 "static_descriptor": { "evidence": "test",
                     "form": { "kind": "range", "minimum": 5, "maximum": 1, "step": 1 } } }"#,
        );
        let good = camera(
            r#"{ "code": 53644, "data_type": 4, "writable": true,
                 "static_descriptor": { "evidence": "test",
                     "form": { "kind": "enumeration", "values": [1, 2, 3, 4, 5, 6, 7] } } }"#,
        );

        assert!(validate_static_descriptors(&options, &empty_enum).is_err());
        assert!(validate_static_descriptors(&options, &bad_range).is_err());
        validate_static_descriptors(&options, &good).expect("a non-empty enumeration passes");
    }
}
