use serde::Serialize;

const MAX_RAW_PROFILE_DISCOVERY_BYTES: usize = 1024 * 1024;

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
            property_code: 0xD185,
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
    use super::RawProfileDiscovery;

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
