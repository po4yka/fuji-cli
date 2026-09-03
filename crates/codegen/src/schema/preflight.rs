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

/// Every option that is a PTP device property must pin `data_type`, and the
/// pin must be the datatype its own wire codec emits. CUE requires the field
/// whenever `prop_code` is set; this check closes the gap between the declared
/// code and the representation the emitters derive from the wire range.
pub fn validate_option_data_types(options: &BTreeMap<String, FujiOption>) -> anyhow::Result<()> {
    for option in options.values() {
        let Some(prop_code) = option.spec.prop_code() else {
            continue;
        };
        let Some(declared) = option.spec.data_type() else {
            bail!(
                "option `{}` is PTP property 0x{prop_code:04x} but pins no data_type",
                option.id
            );
        };
        let wire = option_wire_data_type(option)
            .with_context(|| format!("deriving the wire datatype of option `{}`", option.id))?;
        ensure!(
            declared == wire,
            "option `{}` pins data_type 0x{declared:04x} but its wire codec emits 0x{wire:04x}",
            option.id
        );
    }
    Ok(())
}

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
                    validate_range_against_option(option, &descriptor.form)
                        .with_context(context)?;
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

/// A range form must be the option's own wire range. Otherwise the permit
/// would accept words the option codec rejects, or reject words it produces.
/// Only scaled and raw numeric options have a dense wire range; lookups carry
/// a finite value set that the firmware capability profile owns instead.
fn validate_range_against_option(option: &FujiOption, form: &StaticForm) -> anyhow::Result<()> {
    let StaticForm::Range {
        minimum,
        maximum,
        step,
    } = form
    else {
        return Ok(());
    };
    let Some(wire) = option_wire_range(option) else {
        bail!(
            "option `{}` has no dense wire range, so it cannot carry a range form",
            option.id
        );
    };
    ensure!(
        (*minimum, *maximum, *step) == wire,
        "range form {minimum}/{maximum}/{step} does not match the wire range {}/{}/{} of option `{}`",
        wire.0,
        wire.1,
        wire.2,
        option.id
    );
    Ok(())
}

/// `(minimum, maximum, step)` in wire units for an option whose logical range
/// maps densely onto the wire, or `None` for lookups, strings, and options
/// with incomplete rules.
fn option_wire_range(option: &FujiOption) -> Option<(i64, i64, i64)> {
    match &option.spec {
        OptionSpec::Integer {
            rules, encoding, ..
        } => {
            let scale = i64::from(numeric_scale(encoding)?);
            let rules = rules.as_ref()?;
            Some((
                i64::from(rules.min?) * scale,
                i64::from(rules.max?) * scale,
                i64::from(rules.step?) * scale,
            ))
        }
        OptionSpec::Float {
            rules, encoding, ..
        } => {
            let scale = numeric_scale(encoding)? as f32;
            let rules = rules.as_ref()?;
            Some((
                (rules.min? * scale).round() as i64,
                (rules.max? * scale).round() as i64,
                (rules.step? * scale).round() as i64,
            ))
        }
        OptionSpec::String { .. } | OptionSpec::Enum { .. } => None,
    }
}

fn numeric_scale(encoding: &NumericEncoding) -> Option<i32> {
    match encoding {
        NumericEncoding::Raw { .. } => Some(1),
        NumericEncoding::Scale { spec, .. } => Some(spec.scale),
        NumericEncoding::Lookup { .. } => None,
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

    use super::{validate_option_data_types, validate_static_descriptors};
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
        "encoding": { "kind": "scale", "prop_code": 53663, "data_type": 3, "spec": { "scale": 10 } } } }"#;

    #[test]
    fn a_pinned_datatype_must_match_the_option_wire_codec() {
        let good = BTreeMap::from([option(SIGNED_SCALE)]);
        validate_option_data_types(&good).expect("INT16 is what a signed scale option emits");

        let wrong = BTreeMap::from([option(
            &SIGNED_SCALE.replace(r#""data_type": 3"#, r#""data_type": 4"#),
        )]);
        let error = validate_option_data_types(&wrong)
            .expect_err("UINT16 must be rejected for a signed scale option");
        assert!(format!("{error:#}").contains("0x0003"), "{error:#}");
    }

    #[test]
    fn a_ptp_property_option_without_a_pinned_datatype_fails_the_build() {
        let options = BTreeMap::from([option(&SIGNED_SCALE.replace(r#""data_type": 3, "#, ""))]);

        let error = validate_option_data_types(&options)
            .expect_err("an option with a prop_code must pin its datatype");

        assert!(
            format!("{error:#}").contains("pins no data_type"),
            "{error:#}"
        );
    }

    #[test]
    fn an_option_without_a_prop_code_needs_no_pin() {
        let options = BTreeMap::from([option(
            r#"{ "id": "slot_only", "spec": { "name": "Slot", "kind": "integer",
                 "rules": { "min": 0, "max": 4, "step": 1 },
                 "encoding": { "kind": "raw" } } }"#,
        )]);

        validate_option_data_types(&options).expect("render-only options carry no PTP datatype");
    }

    #[test]
    fn a_range_form_must_equal_the_option_wire_range() {
        let options = BTreeMap::from([option(SIGNED_SCALE)]);
        let matching = camera(
            r#"{ "code": 53663, "data_type": 3, "writable": true,
                 "static_descriptor": { "evidence": "test",
                     "form": { "kind": "range", "minimum": -40, "maximum": 40, "step": 10 } } }"#,
        );
        let widened = camera(
            r#"{ "code": 53663, "data_type": 3, "writable": true,
                 "static_descriptor": { "evidence": "test",
                     "form": { "kind": "range", "minimum": -100, "maximum": 100, "step": 10 } } }"#,
        );

        validate_static_descriptors(&options, &matching)
            .expect("the option's own wire range must be accepted");
        let error = validate_static_descriptors(&options, &widened)
            .expect_err("a range wider than the option codec accepts must fail the build");
        assert!(format!("{error:#}").contains("-40/40/10"), "{error:#}");
    }

    #[test]
    fn a_lookup_option_cannot_carry_a_range_form() {
        let options = BTreeMap::from([option(
            r#"{ "id": "noise", "spec": { "name": "Noise", "kind": "integer",
                 "rules": { "min": -4, "max": 4, "step": 1 },
                 "encoding": { "kind": "lookup", "prop_code": 53665, "data_type": 4,
                     "spec": { "values": { "4": 20480, "0": 0 } } } } }"#,
        )]);
        let cameras = camera(
            r#"{ "code": 53665, "data_type": 4, "writable": true,
                 "static_descriptor": { "evidence": "test",
                     "form": { "kind": "range", "minimum": 0, "maximum": 20480, "step": 4096 } } }"#,
        );

        let error = validate_static_descriptors(&options, &cameras)
            .expect_err("a lookup option has no dense wire range");

        assert!(
            format!("{error:#}").contains("no dense wire range"),
            "{error:#}"
        );
    }

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
