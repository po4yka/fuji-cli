use std::fmt;

use anyhow::{Context, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

pub const MAX_BACKUP_PAYLOAD_BYTES: usize = crate::ptp::MAX_PTP_CONTAINER_PAYLOAD_BYTES;
pub const MAX_BACKUP_ARTIFACT_BYTES: usize =
    MAX_BACKUP_PAYLOAD_BYTES + MAX_MANIFEST_BYTES + FIXED_HEADER_BYTES;
pub const BACKUP_ARTIFACT_FORMAT_VERSION: u16 = 1;

const MAGIC: &[u8; 16] = b"FUJICLI_BACKUP\0\0";
const FIXED_HEADER_BYTES: usize =
    MAGIC.len() + size_of::<u16>() + size_of::<u32>() + size_of::<u64>();
const MAX_MANIFEST_BYTES: usize = 16 * 1024;
const MAX_IDENTITY_TEXT_BYTES: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BackupPurpose {
    Portable,
    Recovery,
}

impl fmt::Display for BackupPurpose {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Portable => formatter.write_str("portable"),
            Self::Recovery => formatter.write_str("recovery"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct BackupIdentity {
    pub camera_name: String,
    pub vendor_id: u16,
    pub product_id: u16,
    pub manufacturer: String,
    pub model: String,
    pub firmware: String,
    pub serial_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct BackupManifest {
    pub purpose: BackupPurpose,
    pub source: BackupIdentity,
    pub payload_len: u64,
    pub payload_sha256: String,
}

#[derive(Debug, Clone)]
pub struct BackupArtifact {
    bytes: Vec<u8>,
    manifest: BackupManifest,
    payload_offset: usize,
}

impl BackupArtifact {
    pub fn create(
        purpose: BackupPurpose,
        source: BackupIdentity,
        payload: &[u8],
    ) -> anyhow::Result<Self> {
        ensure!(!payload.is_empty(), "backup payload is empty");
        ensure!(
            payload.len() <= MAX_BACKUP_PAYLOAD_BYTES,
            "backup payload exceeds {MAX_BACKUP_PAYLOAD_BYTES} bytes"
        );
        validate_identity(&source)?;

        let manifest = BackupManifest {
            purpose,
            source,
            payload_len: u64::try_from(payload.len())?,
            payload_sha256: sha256_hex(payload),
        };
        let manifest_bytes = serde_json::to_vec(&manifest).context("encoding backup manifest")?;
        ensure!(
            manifest_bytes.len() <= MAX_MANIFEST_BYTES,
            "backup manifest exceeds {MAX_MANIFEST_BYTES} bytes"
        );

        let artifact_len = FIXED_HEADER_BYTES
            .checked_add(manifest_bytes.len())
            .and_then(|len| len.checked_add(payload.len()))
            .context("backup artifact length overflow")?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(artifact_len)
            .context("allocating backup artifact")?;
        bytes.extend_from_slice(MAGIC);
        bytes.extend_from_slice(&BACKUP_ARTIFACT_FORMAT_VERSION.to_le_bytes());
        bytes.extend_from_slice(&u32::try_from(manifest_bytes.len())?.to_le_bytes());
        bytes.extend_from_slice(&u64::try_from(payload.len())?.to_le_bytes());
        bytes.extend_from_slice(&manifest_bytes);
        bytes.extend_from_slice(payload);

        Self::parse(bytes)
    }

    /// Parses the versioned fujicli backup envelope and validates its framing.
    ///
    /// # Errors
    ///
    /// Returns an error if the magic, version, lengths, manifest, or exact EOF
    /// framing is invalid.
    pub fn parse(bytes: Vec<u8>) -> anyhow::Result<Self> {
        ensure!(
            bytes.len() >= FIXED_HEADER_BYTES,
            "backup artifact is truncated before its fixed header"
        );
        ensure!(
            &bytes[..MAGIC.len()] == MAGIC,
            "input is not a fujicli backup artifact"
        );

        let version = read_u16(&bytes, MAGIC.len());
        ensure!(
            version == BACKUP_ARTIFACT_FORMAT_VERSION,
            "unsupported backup artifact version {version}"
        );
        let manifest_len = usize::try_from(read_u32(&bytes, MAGIC.len() + size_of::<u16>()))?;
        ensure!(
            manifest_len <= MAX_MANIFEST_BYTES,
            "backup manifest exceeds {MAX_MANIFEST_BYTES} bytes"
        );
        let payload_len_offset = MAGIC.len() + size_of::<u16>() + size_of::<u32>();
        let payload_len = usize::try_from(read_u64(&bytes, payload_len_offset))?;
        ensure!(payload_len > 0, "backup payload is empty");
        ensure!(
            payload_len <= MAX_BACKUP_PAYLOAD_BYTES,
            "backup payload exceeds {MAX_BACKUP_PAYLOAD_BYTES} bytes"
        );

        let payload_offset = FIXED_HEADER_BYTES
            .checked_add(manifest_len)
            .context("backup artifact header length overflow")?;
        let expected_len = payload_offset
            .checked_add(payload_len)
            .context("backup artifact length overflow")?;
        ensure!(
            bytes.len() == expected_len,
            "backup artifact length mismatch: header declares {expected_len} bytes, input has {}",
            bytes.len()
        );

        let manifest: BackupManifest =
            serde_json::from_slice(&bytes[FIXED_HEADER_BYTES..payload_offset])
                .context("decoding backup manifest")?;
        ensure!(
            manifest.payload_len == u64::try_from(payload_len)?,
            "backup manifest payload length does not match its envelope"
        );
        validate_identity(&manifest.source)?;
        validate_sha256(&manifest.payload_sha256, "payload SHA-256")?;
        let actual_payload_sha256 = sha256_hex(&bytes[payload_offset..]);
        ensure!(
            manifest.payload_sha256 == actual_payload_sha256,
            "backup payload SHA-256 mismatch"
        );

        Ok(Self {
            bytes,
            manifest,
            payload_offset,
        })
    }

    pub fn manifest(&self) -> &BackupManifest {
        &self.manifest
    }

    pub fn payload(&self) -> &[u8] {
        &self.bytes[self.payload_offset..]
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn fingerprint(&self) -> String {
        sha256_hex(&self.bytes)
    }

    pub fn verify_fingerprint(&self, expected: &str) -> anyhow::Result<()> {
        validate_sha256(expected, "expected backup artifact SHA-256")?;
        ensure!(
            self.fingerprint() == expected,
            "backup artifact SHA-256 does not match --expect-sha256"
        );
        Ok(())
    }

    pub fn validate_target(
        &self,
        target: &BackupIdentity,
        expected_target_serial_sha256: Option<&str>,
    ) -> anyhow::Result<()> {
        self.validate_target_compatibility(target)?;
        let source = &self.manifest.source;
        let required_serial = match self.manifest.purpose {
            BackupPurpose::Recovery => source.serial_sha256.as_str(),
            BackupPurpose::Portable => {
                if let Some(expected) = expected_target_serial_sha256 {
                    validate_sha256(expected, "expected target serial SHA-256")?;
                    expected
                } else {
                    source.serial_sha256.as_str()
                }
            }
        };
        ensure!(
            target.serial_sha256 == required_serial,
            "backup target serial fingerprint mismatch"
        );
        Ok(())
    }

    pub fn validate_target_compatibility(&self, target: &BackupIdentity) -> anyhow::Result<()> {
        validate_identity(target)?;
        let source = &self.manifest.source;
        ensure!(
            source.camera_name == target.camera_name,
            "backup target camera definition mismatch: artifact is for {}, connected camera is {}",
            source.camera_name,
            target.camera_name
        );
        ensure!(
            source.vendor_id == target.vendor_id && source.product_id == target.product_id,
            "backup target USB model mismatch"
        );
        ensure!(
            source.manufacturer == target.manufacturer,
            "backup target manufacturer mismatch"
        );
        ensure!(
            source.model == target.model,
            "backup target model mismatch: artifact reports {}, connected camera reports {}",
            source.model,
            target.model
        );
        ensure!(
            source.firmware == target.firmware,
            "backup target firmware mismatch: artifact reports {}, connected camera reports {}",
            source.firmware,
            target.firmware
        );

        Ok(())
    }
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";

    let digest = Sha256::digest(bytes);
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

fn validate_identity(identity: &BackupIdentity) -> anyhow::Result<()> {
    validate_identity_text(&identity.camera_name, "backup camera name")?;
    validate_identity_text(&identity.manufacturer, "backup manufacturer")?;
    validate_identity_text(&identity.model, "backup camera model")?;
    validate_identity_text(&identity.firmware, "backup firmware")?;
    validate_sha256(&identity.serial_sha256, "source serial SHA-256")
}

fn validate_identity_text(value: &str, description: &str) -> anyhow::Result<()> {
    ensure!(!value.trim().is_empty(), "{description} is empty");
    ensure!(
        value.len() <= MAX_IDENTITY_TEXT_BYTES,
        "{description} exceeds {MAX_IDENTITY_TEXT_BYTES} bytes"
    );
    ensure!(
        !value.chars().any(char::is_control),
        "{description} contains control characters"
    );
    Ok(())
}

fn validate_sha256(value: &str, description: &str) -> anyhow::Result<()> {
    ensure!(
        value.len() == 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        "{description} must contain exactly 64 lowercase hexadecimal characters"
    );
    Ok(())
}

fn read_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

fn read_u64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
        bytes[offset + 4],
        bytes[offset + 5],
        bytes[offset + 6],
        bytes[offset + 7],
    ])
}

#[cfg(test)]
mod tests {
    use super::{
        BACKUP_ARTIFACT_FORMAT_VERSION, BackupArtifact, BackupIdentity, BackupManifest,
        BackupPurpose, MAGIC,
    };

    fn sample_identity(serial: &[u8]) -> BackupIdentity {
        BackupIdentity {
            camera_name: "FUJIFILM X-T5".to_owned(),
            vendor_id: 0x04cb,
            product_id: 0x02fc,
            manufacturer: "FUJIFILM".to_owned(),
            model: "X-T5".to_owned(),
            firmware: "4.31".to_owned(),
            serial_sha256: super::sha256_hex(serial),
        }
    }

    #[test]
    fn payload_digest_mismatch_is_rejected() -> anyhow::Result<()> {
        let payload = b"backup";
        let manifest = BackupManifest {
            purpose: BackupPurpose::Portable,
            source: sample_identity(b"serial"),
            payload_len: u64::try_from(payload.len())?,
            payload_sha256: "00".repeat(32),
        };
        let manifest = serde_json::to_vec(&manifest)?;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(MAGIC);
        bytes.extend_from_slice(&BACKUP_ARTIFACT_FORMAT_VERSION.to_le_bytes());
        bytes.extend_from_slice(&u32::try_from(manifest.len())?.to_le_bytes());
        bytes.extend_from_slice(&u64::try_from(payload.len())?.to_le_bytes());
        bytes.extend_from_slice(&manifest);
        bytes.extend_from_slice(payload);

        let error = BackupArtifact::parse(bytes)
            .expect_err("a payload that does not match its SHA-256 must be rejected");

        assert!(error.to_string().contains("payload SHA-256 mismatch"));
        Ok(())
    }

    #[test]
    fn created_artifact_round_trips_manifest_and_payload() -> anyhow::Result<()> {
        let source = sample_identity(b"serial");

        let artifact = BackupArtifact::create(BackupPurpose::Portable, source.clone(), b"backup")?;
        let parsed = BackupArtifact::parse(artifact.as_bytes().to_vec())?;

        assert_eq!(parsed.manifest().purpose, BackupPurpose::Portable);
        assert_eq!(parsed.manifest().source, source);
        assert_eq!(parsed.payload(), b"backup");
        Ok(())
    }

    #[test]
    fn identity_control_characters_are_rejected_before_inspection() {
        let mut source = sample_identity(b"serial");
        source.model = "X-T5\u{1b}[2J".to_owned();

        let error = BackupArtifact::create(BackupPurpose::Portable, source, b"backup")
            .expect_err("untrusted identity text must not inject terminal controls");

        assert!(error.to_string().contains("control characters"));
    }

    #[test]
    fn target_model_mismatch_is_rejected_before_restore() -> anyhow::Result<()> {
        let source = sample_identity(b"serial");
        let artifact =
            BackupArtifact::create(BackupPurpose::Portable, source.clone(), b"camera backup")?;
        let mut target = source;
        target.model = "X-S20".to_owned();

        let error = artifact
            .validate_target(&target, None)
            .expect_err("another camera model must be rejected");

        assert!(error.to_string().contains("model mismatch"));
        Ok(())
    }

    #[test]
    fn truncated_payload_is_rejected() -> anyhow::Result<()> {
        let artifact = BackupArtifact::create(
            BackupPurpose::Portable,
            sample_identity(b"serial"),
            b"backup",
        )?;
        let mut bytes = artifact.as_bytes().to_vec();
        assert_eq!(bytes.pop(), Some(b'p'));

        let error = BackupArtifact::parse(bytes).expect_err("truncated payload must fail");

        assert!(error.to_string().contains("length mismatch"));
        Ok(())
    }

    #[test]
    fn trailing_bytes_are_rejected() -> anyhow::Result<()> {
        let artifact = BackupArtifact::create(
            BackupPurpose::Portable,
            sample_identity(b"serial"),
            b"backup",
        )?;
        let mut bytes = artifact.as_bytes().to_vec();
        bytes.push(0);

        let error = BackupArtifact::parse(bytes).expect_err("trailing data must fail exact EOF");

        assert!(error.to_string().contains("length mismatch"));
        Ok(())
    }

    #[test]
    fn bad_magic_is_rejected() -> anyhow::Result<()> {
        let artifact = BackupArtifact::create(
            BackupPurpose::Portable,
            sample_identity(b"serial"),
            b"backup",
        )?;
        let mut bytes = artifact.as_bytes().to_vec();
        bytes[0] ^= 0xff;

        let error = BackupArtifact::parse(bytes).expect_err("foreign input must fail magic");

        assert!(error.to_string().contains("not a fujicli"));
        Ok(())
    }

    #[test]
    fn target_firmware_mismatch_is_rejected() -> anyhow::Result<()> {
        let source = sample_identity(b"serial");
        let artifact =
            BackupArtifact::create(BackupPurpose::Portable, source.clone(), b"camera backup")?;
        let mut target = source;
        target.firmware = "5.00".to_owned();

        let error = artifact
            .validate_target(&target, None)
            .expect_err("cross-firmware restore must fail closed");

        assert!(error.to_string().contains("firmware mismatch"));
        Ok(())
    }

    #[test]
    fn portable_backup_requires_explicit_fingerprint_for_another_serial() -> anyhow::Result<()> {
        let artifact = BackupArtifact::create(
            BackupPurpose::Portable,
            sample_identity(b"source serial"),
            b"camera backup",
        )?;
        let target = sample_identity(b"target serial");

        let error = artifact
            .validate_target(&target, None)
            .expect_err("another body must require an explicit target fingerprint");

        assert!(error.to_string().contains("serial fingerprint mismatch"));
        Ok(())
    }

    #[test]
    fn portable_backup_accepts_explicitly_bound_same_model_target() -> anyhow::Result<()> {
        let artifact = BackupArtifact::create(
            BackupPurpose::Portable,
            sample_identity(b"source serial"),
            b"camera backup",
        )?;
        let target = sample_identity(b"target serial");

        artifact.validate_target(&target, Some(&target.serial_sha256))?;
        Ok(())
    }

    #[test]
    fn recovery_backup_cannot_be_redirected_to_another_serial() -> anyhow::Result<()> {
        let artifact = BackupArtifact::create(
            BackupPurpose::Recovery,
            sample_identity(b"source serial"),
            b"camera backup",
        )?;
        let target = sample_identity(b"target serial");

        let error = artifact
            .validate_target(&target, Some(&target.serial_sha256))
            .expect_err("recovery artifacts must stay bound to their source body");

        assert!(error.to_string().contains("serial fingerprint mismatch"));
        Ok(())
    }

    #[test]
    fn expected_artifact_fingerprint_mismatch_is_rejected() -> anyhow::Result<()> {
        let artifact = BackupArtifact::create(
            BackupPurpose::Portable,
            sample_identity(b"serial"),
            b"camera backup",
        )?;

        let error = artifact
            .verify_fingerprint(&"00".repeat(32))
            .expect_err("external fingerprint pin must match the complete artifact");

        assert!(error.to_string().contains("--expect-sha256"));
        Ok(())
    }
}
