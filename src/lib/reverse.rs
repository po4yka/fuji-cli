use serde::Serialize;

use crate::ptp::DevicePropCode;

const MAX_RAW_PROFILE_DISCOVERY_BYTES: usize = 1024 * 1024;

/// The RAW conversion profile property this discovery captures.
const RAW_PROFILE_PROPERTY: u16 = DevicePropCode::FujiRawConversionProfile as u16;

/// Battery property code, re-exported for the development tools so the
/// unpublished adapter never repeats a code the library already owns.
pub const FUJI_BATTERY_INFO2_PROPERTY: u16 = DevicePropCode::FujiBatteryInfo2 as u16;

/// One advertised device property as the camera answers for it right now.
///
/// The survey never records payload bytes. Every value that was read
/// contributes its length and its [`classify_value_shape`] shape; a
/// SHA-256 digest is recorded only when that shape is `"string"`, because a
/// digest of a 2- or 4-byte scalar is inverted by exhaustive search in
/// seconds (an 8-byte scalar is dropped for uniformity), while a PTP
/// string's value space is too large to exhaust. A
/// property may still hold a serial number, an owner string, or GPS data as
/// a string, so a reviewer who cares about that should glance at
/// string-valued properties before sharing the artifact; scalar-valued
/// properties need no such review.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PropertyObservation {
    pub code: u16,
    pub descriptor_available: bool,
    pub descriptor_data_type: Option<&'static str>,
    pub descriptor_writable: Option<bool>,
    pub descriptor_form: Option<&'static str>,
    pub value_available: bool,
    pub value_length: Option<usize>,
    pub value_shape: Option<&'static str>,
    /// SHA-256 digest of the raw value, present only when `value_shape` is
    /// `"string"`. See [`survey_value_digest`] for the policy.
    pub value_sha256: Option<String>,
    /// PTP datatype code the FML preflight profiles pin for this property on
    /// the connected model and firmware, when they declare it at all.
    pub declared_data_type: Option<u16>,
    /// Whether the observed value has the wire shape of `declared_data_type`.
    /// `None` when nothing is declared or no value could be read.
    pub declaration_matches: Option<bool>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct PropertySurveySummary {
    pub advertised: usize,
    pub descriptors_read: usize,
    pub values_read: usize,
    pub declared: usize,
    pub declaration_mismatches: usize,
}

/// The complete read-only PTP surface of the connected camera in one artifact:
/// what `GetDeviceInfo` advertises, what each advertised property answers, and
/// how that compares with the FML declarations for this exact model. See
/// [`PropertyObservation`] for exactly what each property records.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PropertySurvey {
    pub schema_version: u8,
    pub manufacturer: String,
    pub model: String,
    pub firmware: String,
    pub usb_mode: Option<u32>,
    /// Name of the matching entry in the generated camera registry, if the
    /// PTP identity matches one. `None` means nothing was cross-checked.
    pub declared_camera: Option<&'static str>,
    pub operations_supported: Vec<u16>,
    pub events_supported: Vec<u16>,
    pub capture_formats: Vec<u16>,
    pub image_formats: Vec<u16>,
    pub properties: Vec<PropertyObservation>,
    pub summary: PropertySurveySummary,
}

/// Wire shape of a raw property payload, by the PTP datatypes this project
/// uses. Reported alongside the length so an unexpected width is visible even
/// when the payload itself is never recorded.
pub(crate) fn classify_value_shape(bytes: &[u8]) -> &'static str {
    match bytes {
        [] => "empty",
        [_] => "uint8",
        [_, _] => "uint16",
        [_, _, _, _] => "uint32",
        [_, _, _, _, _, _, _, _] => "uint64",
        [count, rest @ ..]
            if usize::from(*count) * 2 == rest.len() && rest.last_chunk::<2>() == Some(&[0, 0]) =>
        {
            "string"
        }
        _ => "bytes",
    }
}

/// The digest a survey should record for a raw property value, if any.
///
/// A SHA-256 digest of a 2- or 4-byte scalar is inverted by exhaustive
/// search in seconds, so the survey must never record one for a scalar
/// shape (an 8-byte scalar is dropped for uniformity): it would make the
/// artifact as sensitive as the raw bytes it is meant to replace. A PTP
/// string's value space is too large to exhaust, so
/// its digest stays one-way; recording it lets someone who already holds a
/// candidate string confirm it without recovering strings that were never
/// guessed. `"empty"` and `"bytes"` payloads get no digest either: an empty
/// payload has nothing to hide, and `"bytes"` is reserved for a value this
/// project's shapes do not otherwise classify.
pub(crate) fn survey_value_digest(bytes: &[u8]) -> Option<String> {
    (classify_value_shape(bytes) == "string").then(|| crate::features::backup::sha256_hex(bytes))
}

/// Whether a raw payload has the wire shape of `data_type`. Used to check a
/// firmware-derived FML pin against what the camera actually answers.
pub(crate) fn value_matches_data_type(bytes: &[u8], data_type: u16) -> bool {
    match data_type {
        0x0001 | 0x0002 => bytes.len() == 1,
        0x0003 | 0x0004 => bytes.len() == 2,
        0x0005 | 0x0006 => bytes.len() == 4,
        0x0007 | 0x0008 => bytes.len() == 8,
        0x0009 | 0x000a => bytes.len() == 16,
        0xffff => classify_value_shape(bytes) == "string",
        _ => false,
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RawProfileEnvelope {
    pub declared_field_count: Option<i16>,
    pub profile_code: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RawProfileDiscovery {
    pub schema_version: u8,
    pub manufacturer: String,
    pub model: String,
    pub firmware: String,
    pub usb_mode: Option<u32>,
    pub camera_state: &'static str,
    pub property_code: u16,
    pub descriptor_available: bool,
    pub descriptor_length: Option<usize>,
    pub descriptor_sha256: Option<String>,
    pub descriptor_hex: Option<String>,
    pub property_data_type: Option<String>,
    pub property_writable: Option<bool>,
    pub property_form: Option<String>,
    pub payload_length: usize,
    pub payload_sha256: String,
    pub payload_hex: String,
    pub envelope: RawProfileEnvelope,
}

impl RawProfileDiscovery {
    pub(crate) fn from_observation(
        manufacturer: String,
        model: String,
        firmware: String,
        usb_mode: Option<u32>,
        descriptor: Option<&[u8]>,
        descriptor_summary: Option<(String, bool, String)>,
        payload: &[u8],
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(
            payload.len() <= MAX_RAW_PROFILE_DISCOVERY_BYTES,
            "RAW conversion profile exceeds the {MAX_RAW_PROFILE_DISCOVERY_BYTES}-byte discovery limit",
        );
        if let Some(descriptor) = descriptor {
            anyhow::ensure!(
                descriptor.len() <= MAX_RAW_PROFILE_DISCOVERY_BYTES,
                "RAW conversion descriptor exceeds the {MAX_RAW_PROFILE_DISCOVERY_BYTES}-byte discovery limit",
            );
        }
        let (property_data_type, property_writable, property_form) = descriptor_summary
            .map_or((None, None, None), |(data_type, writable, form)| {
                (Some(data_type), Some(writable), Some(form))
            });
        Ok(Self {
            schema_version: 1,
            manufacturer,
            model,
            firmware,
            usb_mode,
            camera_state: "unknown",
            property_code: RAW_PROFILE_PROPERTY,
            descriptor_available: descriptor.is_some(),
            descriptor_length: descriptor.map(<[u8]>::len),
            descriptor_sha256: descriptor.map(crate::features::backup::sha256_hex),
            descriptor_hex: descriptor.map(encode_hex),
            property_data_type,
            property_writable,
            property_form,
            payload_length: payload.len(),
            payload_sha256: crate::features::backup::sha256_hex(payload),
            payload_hex: encode_hex(payload),
            envelope: inspect_envelope(payload),
        })
    }
}

fn inspect_envelope(payload: &[u8]) -> RawProfileEnvelope {
    let declared_field_count = payload
        .get(..2)
        .map(|bytes| i16::from_le_bytes([bytes[0], bytes[1]]));
    let profile_code = payload.get(2).and_then(|length| {
        let byte_length = usize::from(*length).checked_mul(2)?;
        let encoded = payload.get(3..3_usize.checked_add(byte_length)?)?;
        let units = encoded
            .as_chunks::<2>()
            .0
            .iter()
            .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
            .collect::<Vec<_>>();
        String::from_utf16(&units).ok()
    });
    RawProfileEnvelope {
        declared_field_count,
        profile_code,
    }
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::{
        RawProfileDiscovery, classify_value_shape, survey_value_digest, value_matches_data_type,
    };

    #[test]
    fn value_shapes_cover_the_scalars_and_ptp_strings_this_project_reads() {
        assert_eq!(classify_value_shape(&[]), "empty");
        assert_eq!(classify_value_shape(&[6]), "uint8");
        assert_eq!(classify_value_shape(&[6, 0]), "uint16");
        assert_eq!(classify_value_shape(&[6, 0, 0, 0]), "uint32");
        assert_eq!(classify_value_shape(&[0; 8]), "uint64");
        // A three-unit PTP string: length byte, two characters, terminator.
        assert_eq!(
            classify_value_shape(&[0x03, b'6', 0, b'5', 0, 0, 0]),
            "string"
        );
        // One unit that is the terminator: a well-formed empty PTP string.
        // The X-T5 audit saw exactly these bytes from D20B.
        assert_eq!(classify_value_shape(&[0x01, 0, 0]), "string");
        assert_eq!(classify_value_shape(&[0x01, 0, 5]), "bytes");
    }

    #[test]
    fn survey_value_digest_is_none_for_a_scalar_payload() {
        // A uint16 payload is invertible by exhaustive search, so the survey
        // must not record a digest for it.
        assert_eq!(survey_value_digest(&[6, 0]), None);
    }

    #[test]
    fn survey_value_digest_is_some_for_a_string_payload() {
        // A three-unit PTP string: length byte, two characters, terminator.
        let digest = survey_value_digest(&[0x03, b'6', 0, b'5', 0, 0, 0]);
        assert_eq!(digest.as_deref().map(str::len), Some(64));
    }

    #[test]
    fn survey_value_digest_is_none_for_an_empty_payload() {
        assert_eq!(survey_value_digest(&[]), None);
    }

    #[test]
    fn a_four_byte_payload_is_reported_as_a_scalar_not_a_string() {
        // 0x01 would frame a one-unit string, but the terminator is missing,
        // so the survey must not claim a string shape for a uint32.
        assert_eq!(classify_value_shape(&[0x01, 0x00, 0x0b, 0x00]), "uint32");
    }

    #[test]
    fn declared_datatypes_are_checked_against_the_observed_wire_shape() {
        assert!(value_matches_data_type(&[0, 0], 0x0004));
        assert!(value_matches_data_type(&[0, 0], 0x0003));
        assert!(!value_matches_data_type(&[0, 0], 0x0006));
        assert!(value_matches_data_type(&[0, 0, 0, 0], 0x0006));
        assert!(value_matches_data_type(&[0x01, 0, 0], 0xffff));
        assert!(!value_matches_data_type(&[0, 0], 0xffff));
        // Array datatypes are never claimed to match: the survey records
        // shapes, and an array's element count is not verifiable from length.
        assert!(!value_matches_data_type(&[1, 0, 0, 0, 6, 0], 0x4004));
    }

    #[test]
    fn discovery_preserves_payload_and_reports_only_unambiguous_envelope_fields() {
        let mut payload = 29_i16.to_le_bytes().to_vec();
        payload.push(8);
        for unit in "ff179502".encode_utf16() {
            payload.extend_from_slice(&unit.to_le_bytes());
        }
        payload.extend_from_slice(&[0xaa, 0xbb, 0xcc]);

        let discovery = RawProfileDiscovery::from_observation(
            "FUJIFILM".to_owned(),
            "X-T5".to_owned(),
            "4.31".to_owned(),
            Some(6),
            Some(&[0x85, 0xd1, 0x06, 0x00, 0x01]),
            Some(("undefined".to_owned(), true, "none".to_owned())),
            &payload,
        )
        .expect("small discovery payload must be accepted");

        assert_eq!(discovery.schema_version, 1);
        assert_eq!(discovery.property_code, 0xD185);
        assert!(discovery.descriptor_available);
        assert_eq!(
            discovery.descriptor_sha256.as_deref().map(str::len),
            Some(64)
        );
        assert_eq!(discovery.payload_length, payload.len());
        assert_eq!(discovery.envelope.declared_field_count, Some(29));
        assert_eq!(discovery.envelope.profile_code.as_deref(), Some("ff179502"));
        assert_eq!(discovery.payload_hex.len(), payload.len() * 2);
        assert_eq!(discovery.payload_sha256.len(), 64);
    }

    #[test]
    fn discovery_rejects_payload_above_the_artifact_budget() {
        let payload = vec![0; super::MAX_RAW_PROFILE_DISCOVERY_BYTES + 1];

        let error = RawProfileDiscovery::from_observation(
            "FUJIFILM".to_owned(),
            "Unknown".to_owned(),
            "Unknown".to_owned(),
            None,
            None,
            None,
            &payload,
        )
        .expect_err("oversized discovery artifacts must be rejected before hex expansion");

        assert!(error.to_string().contains("discovery limit"));
    }

    #[test]
    fn discovery_preserves_payload_when_the_descriptor_cannot_be_decoded() {
        let payload = [1, 0, 1, b'1', 0, 0, 0, 0, 0, 0, 0];
        let unknown_descriptor = [0x85, 0xd1, 0xff, 0x9f, 0];

        let discovery = RawProfileDiscovery::from_observation(
            "FUJIFILM".to_owned(),
            "Unknown".to_owned(),
            "Unknown".to_owned(),
            None,
            Some(&unknown_descriptor),
            None,
            &payload,
        )
        .expect("unknown descriptor syntax must not discard the captured payload");

        assert!(discovery.descriptor_available);
        assert!(discovery.property_data_type.is_none());
        assert_eq!(discovery.payload_hex.len(), payload.len() * 2);
    }
}
