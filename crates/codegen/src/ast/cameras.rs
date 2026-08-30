use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::Value;

use crate::ast::{LeafEquals, LeafPresent, Predicate, Scope};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Camera {
    pub id: String,
    pub spec: CameraSpec,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CameraSpec {
    pub name: String,
    pub generation: String,
    pub usb: Usb,
    #[serde(default)]
    pub ptp: Option<PtpIdentity>,
    #[serde(default)]
    pub preflight: Vec<PreflightProfile>,
    pub capabilities: Option<CameraCapabilities>,
    pub features: Option<Features>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Usb {
    pub vendor_id: u16,
    pub product_id: u16,
    pub chunk_size_ceiling: u32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PtpIdentity {
    pub manufacturer: String,
    pub model: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreflightProfile {
    pub operation: PreflightOperation,
    pub status: PreflightStatus,
    pub firmware: String,
    pub minimum_battery_percent: u8,
    pub allowed_usb_modes: Vec<u32>,
    pub required_operations: Vec<u16>,
    pub required_properties: Vec<PreflightProperty>,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PreflightOperation {
    BackupRestore,
    SimulationAccess,
    SimulationWrite,
    RawConversion,
    RawRecoveryFetch,
    RawRecoveryCleanup,
}

#[cfg(test)]
mod preflight_operation_tests {
    use super::PreflightOperation;

    #[test]
    fn temporary_reads_have_a_distinct_preflight_operation() {
        let simulation: PreflightOperation = serde_json::from_str(r#""simulation_access""#)
            .expect("simulation access operation must parse");

        assert_eq!(simulation, PreflightOperation::SimulationAccess);
    }

    #[test]
    fn raw_recovery_fetch_and_cleanup_have_distinct_preflight_operations() {
        let fetch: PreflightOperation = serde_json::from_str(r#""raw_recovery_fetch""#)
            .expect("RAW recovery fetch operation must parse");
        let cleanup: PreflightOperation = serde_json::from_str(r#""raw_recovery_cleanup""#)
            .expect("RAW recovery cleanup operation must parse");

        assert_eq!(fetch, PreflightOperation::RawRecoveryFetch);
        assert_eq!(cleanup, PreflightOperation::RawRecoveryCleanup);
    }
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PreflightStatus {
    Verified,
    Unverified,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreflightProperty {
    pub code: u16,
    pub data_type: Option<u16>,
    pub writable: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CameraCapabilities {
    pub generation: CapabilitySet,
    pub model: CapabilitySet,
    pub firmware: BTreeMap<String, CapabilitySet>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CapabilitySet {
    pub option_overrides: Vec<OptionCapability>,
    pub raw_conversion: Option<RawConversionDescriptor>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RawConversionDescriptor {
    pub id: String,
    pub evidence: RawConversionEvidence,
    pub binding: RawConversionBinding,
    pub read: RawConversionLayout,
    pub write: Option<RawConversionLayout>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RawConversionEvidence {
    pub status: RawConversionEvidenceStatus,
    pub manifests: Vec<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RawConversionEvidenceStatus {
    Unverified,
    Observed,
    ReadVerified,
    WriteVerified,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RawConversionBinding {
    pub usb_modes: Vec<u32>,
    pub camera_state: Option<RawConversionCameraState>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RawConversionCameraState {
    Still,
    Movie,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RawConversionLayout {
    pub profile_code: String,
    pub header_padding: u32,
    pub declared_field_count: u16,
    pub total_length: u32,
    pub fields: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OptionCapability {
    pub r#ref: String,
    pub allowed_values: Option<Vec<String>>,
    #[serde(default)]
    pub wire_values: BTreeMap<String, CapabilityWireValue>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum CapabilityWireValue {
    Single(i32),
    Multi(Vec<i32>),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Features {
    #[serde(default)]
    pub backup: bool,
    pub simulation: Option<Simulation>,
    pub render: Option<Render>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Simulation {
    pub slots: u32,
    pub settings: Vec<Setting>,
    #[serde(default)]
    pub transformations: Vec<Transformation>,
    #[serde(default)]
    pub rules: Vec<Rule>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Render {
    pub profile_code: u32,
    pub header_padding: u32,
    pub fields: Vec<Field>,
    #[serde(default)]
    pub transformations: Vec<Transformation>,
    #[serde(default)]
    pub rules: Vec<Rule>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Setting {
    pub id: String,
    pub r#ref: String,
}

#[derive(Debug, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
pub enum Field {
    Ref(FieldRef),
    Inline(FieldInline),
}

impl Field {
    pub fn id(&self) -> &str {
        match self {
            Field::Ref(r) => &r.id,
            Field::Inline(i) => &i.id,
        }
    }

    pub fn skip_read(&self) -> bool {
        match self {
            Field::Ref(r) => r.skip_read,
            Field::Inline(i) => i.skip_read,
        }
    }

    pub fn skip_write(&self) -> bool {
        match self {
            Field::Ref(r) => r.skip_write,
            Field::Inline(i) => i.skip_write,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldRef {
    pub id: String,
    pub r#ref: String,
    #[serde(default)]
    pub skip_read: bool,
    #[serde(default)]
    pub skip_write: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldInline {
    pub id: String,
    #[serde(default)]
    pub skip_read: bool,
    #[serde(default)]
    pub skip_write: bool,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SpecKind {
    Integer,
    Float,
    String,
    Enum,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Transformation {
    pub when: Option<Predicate>,
    pub apply: Vec<Assignment>,
}

#[derive(Clone, Debug)]
pub struct Assignment {
    pub r#ref: String,
    pub effect: AssignmentEffect,
}

#[derive(Clone, Debug)]
pub enum AssignmentEffect {
    Set(Value),
    Clear,
}

impl From<&Assignment> for Predicate {
    fn from(a: &Assignment) -> Self {
        match &a.effect {
            AssignmentEffect::Set(v) => LeafEquals {
                r#ref: a.r#ref.clone(),
                scope: Scope::Current,
                equals: v.clone(),
            }
            .into(),
            AssignmentEffect::Clear => LeafPresent {
                r#ref: a.r#ref.clone(),
                scope: Scope::Current,
                present: false,
            }
            .into(),
        }
    }
}

impl<'de> Deserialize<'de> for Assignment {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Raw {
            r#ref: String,
            #[serde(default)]
            value: Option<Value>,
            #[serde(default)]
            present: Option<bool>,
        }

        let raw = Raw::deserialize(deserializer)?;
        match (raw.value, raw.present) {
            (Some(value), None) => Ok(Assignment {
                r#ref: raw.r#ref,
                effect: AssignmentEffect::Set(value),
            }),
            (None, Some(false)) => Ok(Assignment {
                r#ref: raw.r#ref,
                effect: AssignmentEffect::Clear,
            }),
            _ => unreachable!("cue should ensure exactly one of the variants is present"),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    #[serde(default)]
    pub severity: Severity,
    pub message: String,
    pub when: Predicate,
}

#[derive(Debug, Default, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    #[default]
    Error,
    Warning,
    Info,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse<T: serde::de::DeserializeOwned>(json: &str) -> T {
        serde_json::from_str(json).unwrap()
    }

    fn parse_err<T: serde::de::DeserializeOwned>(json: &str) {
        assert!(
            serde_json::from_str::<T>(json).is_err(),
            "expected parse error for: {json}"
        );
    }

    #[test]
    fn rule_severity_defaults_to_error_and_round_trips() {
        let default_: Rule = parse(r#"{ "message": "x", "when": true }"#);
        let warn: Rule = parse(r#"{ "severity": "warning", "message": "x", "when": true }"#);
        let info: Rule = parse(r#"{ "severity": "info", "message": "x", "when": true }"#);
        let err: Rule = parse(r#"{ "severity": "error", "message": "x", "when": true }"#);
        assert_eq!(default_.severity, Severity::Error);
        assert_eq!(warn.severity, Severity::Warning);
        assert_eq!(info.severity, Severity::Info);
        assert_eq!(err.severity, Severity::Error);

        parse_err::<Rule>(r#"{ "severity": "critical", "message": "x", "when": true }"#);
    }

    #[test]
    fn setting_requires_ref() {
        let s: Setting = parse(r#"{ "id": "x", "ref": "image_size" }"#);
        assert_eq!(s.id, "x");
        assert_eq!(s.r#ref, "image_size");
        parse_err::<Setting>(r#"{ "id": "head_0" }"#);
    }

    #[test]
    fn field_disambiguates_ref_vs_inline_by_presence_of_ref() {
        let r: Field = parse(r#"{ "id": "x", "ref": "image_size" }"#);
        let i: Field = parse(r#"{ "id": "head_0" }"#);
        assert!(matches!(r, Field::Ref(_)));
        assert!(matches!(i, Field::Inline(_)));
    }

    #[test]
    fn field_skip_flags_default_false_and_round_trip() {
        let plain: Field = parse(r#"{ "id": "x", "ref": "image_size" }"#);
        let Field::Ref(plain) = plain else { panic!() };
        assert!(!plain.skip_read && !plain.skip_write);

        let both: Field =
            parse(r#"{ "id": "x", "ref": "image_size", "skip_read": true, "skip_write": true }"#);
        let Field::Ref(both) = both else { panic!() };
        assert!(both.skip_read && both.skip_write);

        let inline: Field = parse(r#"{ "id": "x", "skip_read": true }"#);
        let Field::Inline(inline) = inline else {
            panic!()
        };
        assert!(inline.skip_read && !inline.skip_write);
    }

    #[test]
    fn simulation_transformations_and_rules_default_empty() {
        let sim: Simulation = parse(
            r#"{
                "slots": 4,
                "settings": [{ "id": "a", "ref": "image_size" }]
            }"#,
        );
        assert_eq!(sim.slots, 4);
        assert_eq!(sim.settings.len(), 1);
        assert!(sim.transformations.is_empty());
        assert!(sim.rules.is_empty());
    }

    #[test]
    fn transformation_when_is_optional() {
        let t: Transformation = parse(r#"{ "apply": [{ "ref": "x", "value": 0 }] }"#);
        assert!(t.when.is_none());
        assert_eq!(t.apply.len(), 1);
    }

    #[test]
    fn assignment_parses_set_form() {
        let a: Assignment = parse(r#"{ "ref": "x", "value": 5 }"#);
        assert_eq!(a.r#ref, "x");
        assert!(matches!(a.effect, AssignmentEffect::Set(_)));
    }

    #[test]
    fn assignment_parses_clear_form() {
        let a: Assignment = parse(r#"{ "ref": "x", "present": false }"#);
        assert_eq!(a.r#ref, "x");
        assert!(matches!(a.effect, AssignmentEffect::Clear));
    }

    #[test]
    fn features_backup_defaults_false_and_capabilities_optional() {
        let f: Features = parse("{}");
        assert!(!f.backup);
        assert!(f.simulation.is_none() && f.render.is_none());

        let only_render: Features =
            parse(r#"{ "render": { "profile_code": 1, "header_padding": 0, "fields": [] } }"#);
        assert!(!only_render.backup);
        assert!(only_render.simulation.is_none());
        assert!(only_render.render.is_some());
    }

    #[test]
    fn camera_spec_accepts_preflight_profiles() {
        let result = serde_json::from_str::<CameraSpec>(
            r#"{
                "name": "Demo",
                "generation": "gen_a",
                "usb": { "vendor_id": 1227, "product_id": 764, "chunk_size_ceiling": 1024 },
                "preflight": [{
                    "operation": "backup_restore",
                    "status": "verified",
                    "firmware": "4.31",
                    "minimum_battery_percent": 100,
                    "allowed_usb_modes": [6],
                    "required_operations": [4097, 4116, 4117],
                    "required_properties": [{ "code": 53614, "writable": false }]
                }]
            }"#,
        );

        assert!(result.is_ok(), "preflight profile must parse: {result:?}");
    }

    #[test]
    fn camera_spec_accepts_exact_ptp_identity() {
        let result = serde_json::from_str::<CameraSpec>(
            r#"{
                "name": "Demo",
                "generation": "gen_a",
                "usb": { "vendor_id": 1227, "product_id": 764, "chunk_size_ceiling": 1024 },
                "ptp": { "manufacturer": "FUJIFILM", "model": "X-T5" }
            }"#,
        );

        assert!(result.is_ok(), "PTP identity must parse: {result:?}");
    }

    #[test]
    fn usb_transport_contract_rejects_legacy_chunk_size() {
        let result = serde_json::from_str::<Camera>(
            r#"{
                "id": "demo",
                "spec": {
                    "name": "Demo",
                    "generation": "gen_a",
                    "usb": {
                        "vendor_id": 1227,
                        "product_id": 764,
                        "chunk_size": 1024
                    }
                }
            }"#,
        );

        assert!(result.is_err(), "legacy chunk_size must be rejected");
    }

    #[test]
    fn camera_spec_accepts_layered_firmware_capabilities() {
        let result = serde_json::from_str::<CameraSpec>(
            r#"{
                "name": "Demo",
                "generation": "gen_a",
                "usb": { "vendor_id": 1227, "product_id": 764, "chunk_size_ceiling": 1024 },
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
            }"#,
        );

        assert!(
            result.is_ok(),
            "layered capabilities must parse: {result:?}"
        );
    }

    #[test]
    fn capability_set_accepts_directional_raw_conversion_descriptor() {
        let result = serde_json::from_str::<CapabilitySet>(
            r#"{
                "raw_conversion": {
                    "id": "x-t5-4.31-still",
                    "evidence": { "status": "write_verified", "manifests": ["capture.json"] },
                    "binding": { "usb_modes": [6], "camera_state": "still" },
                    "read": {
                        "profile_code": "ff179502", "header_padding": 494,
                        "declared_field_count": 3, "total_length": 527,
                        "fields": ["head_0", "film_simulation"]
                    },
                    "write": {
                        "profile_code": "ff179502", "header_padding": 494,
                        "declared_field_count": 3, "total_length": 531,
                        "fields": ["head_0", "film_simulation", "tail_0"]
                    }
                }
            }"#,
        );

        assert!(result.is_ok(), "RAW descriptor must parse: {result:?}");
    }
}
