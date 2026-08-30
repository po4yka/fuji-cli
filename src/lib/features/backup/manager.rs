use std::{
    fmt,
    io::{Cursor, Seek, Write},
};

use anyhow::ensure;
use binrw::{BinRead, BinResult, BinWrite, Endian};
use log::debug;

use crate::{
    features::{
        backup::artifact::{BackupArtifact, sha256_hex},
        base::CameraBase,
        outcome::{OutcomeStatus, StateChangeAudit},
    },
    ptp::{CommandCode, ObjectFormat, ObjectInfo, Ptp, PtpOperation},
};

pub const OBJECT_HANDLE: [u32; 1] = [0x0];
pub const EXPORT_OBJECT_INFO_HANDLE: [u32; 1] = [0x0];
pub const IMPORT_OBJECT_INFO_HANDLE: [u32; 2] = [0x0, 0x0];
const BACKUP_OBJECT_INFO_PADDING_BYTES: usize = 1020;
const BACKUP_OBJECT_INFO_BYTES: usize = 1076;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackupImportPhase {
    ObjectInfo,
    ObjectData,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackupImportState {
    Unknown,
}

#[derive(Debug)]
pub struct BackupImportError {
    phase: BackupImportPhase,
    state: BackupImportState,
    audit: StateChangeAudit,
    source: anyhow::Error,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BackupRestoreAccepted {
    audit: StateChangeAudit,
    expected_payload_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BackupRestoreOutcome {
    audit: StateChangeAudit,
    payload_sha256: String,
}

#[derive(Debug)]
pub struct BackupPostconditionError {
    audit: StateChangeAudit,
    expected_payload_sha256: String,
    observed_payload_sha256: String,
}

impl BackupImportError {
    pub fn phase(&self) -> BackupImportPhase {
        self.phase
    }

    pub fn state(&self) -> BackupImportState {
        self.state
    }

    pub const fn audit(&self) -> StateChangeAudit {
        self.audit
    }
}

impl BackupRestoreAccepted {
    fn new(payload: &[u8]) -> Self {
        Self {
            audit: StateChangeAudit::ptp_accepted(),
            expected_payload_sha256: sha256_hex(payload),
        }
    }

    pub const fn audit(&self) -> StateChangeAudit {
        self.audit
    }

    /// Compares a backup exported from a fresh camera session with the requested restore payload.
    ///
    /// # Errors
    ///
    /// Returns [`BackupPostconditionError`] when the accepted payload is not the payload being
    /// verified or when the fresh-session export differs byte-for-byte from the requested state.
    pub fn verify_post_restore_export(
        self,
        expected: &BackupArtifact,
        observed: &BackupArtifact,
    ) -> Result<BackupRestoreOutcome, BackupPostconditionError> {
        let expected_payload_sha256 = sha256_hex(expected.payload());
        let observed_payload_sha256 = sha256_hex(observed.payload());
        if self.expected_payload_sha256 == expected_payload_sha256
            && expected.payload() == observed.payload()
        {
            Ok(BackupRestoreOutcome {
                audit: self
                    .audit
                    .with_semantic(OutcomeStatus::Succeeded)
                    .with_persistence(OutcomeStatus::Succeeded),
                payload_sha256: observed_payload_sha256,
            })
        } else {
            Err(BackupPostconditionError {
                audit: self
                    .audit
                    .with_semantic(OutcomeStatus::Failed)
                    .with_persistence(OutcomeStatus::Failed),
                expected_payload_sha256,
                observed_payload_sha256,
            })
        }
    }
}

impl BackupRestoreOutcome {
    pub const fn audit(&self) -> StateChangeAudit {
        self.audit
    }

    pub fn payload_sha256(&self) -> &str {
        &self.payload_sha256
    }
}

impl BackupPostconditionError {
    pub const fn audit(&self) -> StateChangeAudit {
        self.audit
    }

    pub fn expected_payload_sha256(&self) -> &str {
        &self.expected_payload_sha256
    }

    pub fn observed_payload_sha256(&self) -> &str {
        &self.observed_payload_sha256
    }
}

impl fmt::Display for BackupImportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let phase = match self.phase {
            BackupImportPhase::ObjectInfo => "object metadata",
            BackupImportPhase::ObjectData => "object data",
        };
        write!(
            formatter,
            "backup import failed while sending {phase}; camera state is unknown and the operation must not be retried automatically"
        )
    }
}

impl std::error::Error for BackupImportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.source.as_ref())
    }
}

impl fmt::Display for BackupPostconditionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "backup restore was accepted by PTP, but a fresh-session export did not match the requested payload (expected {}, observed {}); camera state is unknown and the restore must not be retried automatically",
            self.expected_payload_sha256, self.observed_payload_sha256
        )
    }
}

impl std::error::Error for BackupPostconditionError {}

trait BackupTransport {
    fn send_object_info(&mut self, object_info: &[u8]) -> anyhow::Result<()>;

    fn send_object_data(&mut self, buffer: &[u8]) -> anyhow::Result<()>;
}

pub(crate) trait BackupExportTransport {
    fn get_object_info(&mut self) -> anyhow::Result<Vec<u8>>;

    fn get_object_data(&mut self) -> anyhow::Result<Vec<u8>>;
}

impl BackupTransport for Ptp {
    fn send_object_info(&mut self, object_info: &[u8]) -> anyhow::Result<()> {
        self.send_for_operation(
            PtpOperation::CameraProcessing,
            CommandCode::SendObjectInfo,
            &IMPORT_OBJECT_INFO_HANDLE,
            Some(object_info),
        )?;
        Ok(())
    }

    fn send_object_data(&mut self, buffer: &[u8]) -> anyhow::Result<()> {
        self.send_for_operation(
            PtpOperation::LargeTransfer,
            CommandCode::SendObject,
            &OBJECT_HANDLE,
            Some(buffer),
        )?;
        Ok(())
    }
}

impl BackupExportTransport for Ptp {
    fn get_object_info(&mut self) -> anyhow::Result<Vec<u8>> {
        self.send(CommandCode::GetObjectInfo, &EXPORT_OBJECT_INFO_HANDLE, None)
    }

    fn get_object_data(&mut self) -> anyhow::Result<Vec<u8>> {
        self.send_for_operation(
            PtpOperation::LargeTransfer,
            CommandCode::GetObject,
            &OBJECT_HANDLE,
            None,
        )
    }
}

pub(crate) fn export_backup_with_transport(
    transport: &mut impl BackupExportTransport,
) -> anyhow::Result<Vec<u8>> {
    let object_info = transport.get_object_info()?;
    ensure!(
        object_info.len() == BACKUP_OBJECT_INFO_BYTES,
        "Fuji backup object info must contain exactly {BACKUP_OBJECT_INFO_BYTES} bytes"
    );
    let mut reader = Cursor::new(&object_info);
    let object_info_value = ObjectInfo::read_options(&mut reader, Endian::Little, ())?;
    let decoded_len = usize::try_from(reader.position())?;
    let padding = &object_info[decoded_len..];
    ensure!(
        padding.len() == BACKUP_OBJECT_INFO_PADDING_BYTES && padding.iter().all(|byte| *byte == 0),
        "Fuji backup object info must have exactly {BACKUP_OBJECT_INFO_PADDING_BYTES} zero padding bytes"
    );
    ensure!(
        object_info_value.object_format == ObjectFormat::FujiBackup,
        "GetObjectInfo did not describe a Fujifilm backup object"
    );

    let payload = transport.get_object_data()?;
    ensure!(
        usize::try_from(object_info_value.compressed_size)? == payload.len(),
        "backup payload length does not match GetObjectInfo"
    );
    Ok(payload)
}

fn import_backup_with_transport(
    transport: &mut impl BackupTransport,
    object_info: &[u8],
    buffer: &[u8],
) -> anyhow::Result<BackupRestoreAccepted> {
    transport.send_object_info(object_info).map_err(|source| {
        let audit = StateChangeAudit::from_write_error(&source);
        BackupImportError {
            phase: BackupImportPhase::ObjectInfo,
            state: BackupImportState::Unknown,
            audit,
            source,
        }
    })?;
    transport.send_object_data(buffer).map_err(|source| {
        let audit = StateChangeAudit::from_write_error(&source);
        BackupImportError {
            phase: BackupImportPhase::ObjectData,
            state: BackupImportState::Unknown,
            audit,
            source,
        }
    })?;
    Ok(BackupRestoreAccepted::new(buffer))
}

#[cfg(test)]
fn import_backup_artifact_with_transport(
    transport: &mut impl BackupTransport,
    artifact: Vec<u8>,
) -> anyhow::Result<BackupRestoreAccepted> {
    let artifact = BackupArtifact::parse(artifact)?;
    let object_info = BackupObjectInfo::new(artifact.payload().len())?;
    let object_info = crate::ptp::codec::encode(&object_info)?;
    import_backup_with_transport(transport, &object_info, artifact.payload())
}

fn import_validated_backup_with_transport(
    transport: &mut impl BackupTransport,
    artifact: &BackupArtifact,
) -> anyhow::Result<BackupRestoreAccepted> {
    let object_info = BackupObjectInfo::new(artifact.payload().len())?;
    let object_info = crate::ptp::codec::encode(&object_info)?;
    import_backup_with_transport(transport, &object_info, artifact.payload())
}

// NOTE: Naively assuming that all cameras backup/restore in the same way.
pub trait CameraBackupManager: CameraBase {
    fn export_backup(&self, ptp: &mut Ptp) -> anyhow::Result<Vec<u8>> {
        debug!("Starting backup export");
        let response = export_backup_with_transport(ptp)?;
        debug!("Backup export completed");

        Ok(response)
    }

    fn import_backup(
        &self,
        ptp: &mut Ptp,
        artifact: &BackupArtifact,
    ) -> anyhow::Result<BackupRestoreAccepted> {
        debug!("Starting backup import");
        let accepted = import_validated_backup_with_transport(ptp, artifact)?;
        debug!("Backup import completed");

        Ok(accepted)
    }
}

impl<T> CameraBackupManager for T where T: CameraBase {}

// NOTE: Naively assuming that all cameras support backup/restore using the same structs.
pub struct BackupObjectInfo {
    compressed_size: u32,
}

impl BinWrite for BackupObjectInfo {
    type Args<'a> = ();

    fn write_options<W: Write + Seek>(
        &self,
        writer: &mut W,
        endian: Endian,
        (): Self::Args<'_>,
    ) -> BinResult<()> {
        let object_info = ObjectInfo {
            object_format: ObjectFormat::FujiBackup,
            compressed_size: self.compressed_size,
            ..Default::default()
        };

        object_info.write_options(writer, endian, ())?;
        writer.write_all(&[0x0u8; BACKUP_OBJECT_INFO_PADDING_BYTES])?;
        Ok(())
    }
}

impl BackupObjectInfo {
    pub fn new(buffer_len: usize) -> anyhow::Result<Self> {
        Ok(Self {
            compressed_size: u32::try_from(buffer_len)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::io;

    use crate::{
        features::{
            backup::{BackupArtifact, BackupIdentity, BackupPurpose},
            outcome::OutcomeStatus,
        },
        ptp::codec::encode,
    };

    use super::{
        BACKUP_OBJECT_INFO_BYTES, BACKUP_OBJECT_INFO_PADDING_BYTES, BackupExportTransport,
        BackupImportError, BackupImportPhase, BackupImportState, BackupObjectInfo, BackupTransport,
        export_backup_with_transport, import_backup_artifact_with_transport,
        import_backup_with_transport,
    };

    struct FailingDataTransport;

    struct FailingInfoTransport;

    struct WrongFormatExportTransport {
        object_data_calls: usize,
    }

    struct WrongLengthExportTransport;

    fn sample_artifact(payload: &[u8]) -> BackupArtifact {
        BackupArtifact::create(
            BackupPurpose::Portable,
            BackupIdentity {
                camera_name: "FUJIFILM X-T5".to_owned(),
                vendor_id: 0x04cb,
                product_id: 0x02fc,
                manufacturer: "FUJIFILM".to_owned(),
                model: "X-T5".to_owned(),
                firmware: "4.31".to_owned(),
                serial_sha256: crate::features::backup::sha256_hex(b"serial"),
            },
            payload,
        )
        .expect("backup fixture must be valid")
    }

    #[derive(Default)]
    struct FakeBackupTransport {
        object_info_calls: usize,
        object_data_calls: usize,
    }

    impl BackupTransport for FailingInfoTransport {
        fn send_object_info(&mut self, _object_info: &[u8]) -> anyhow::Result<()> {
            Err(io::Error::new(io::ErrorKind::TimedOut, "ambiguous info timeout").into())
        }

        fn send_object_data(&mut self, _buffer: &[u8]) -> anyhow::Result<()> {
            panic!("object data must not be sent after object-info failure")
        }
    }

    impl BackupTransport for FailingDataTransport {
        fn send_object_info(&mut self, _object_info: &[u8]) -> anyhow::Result<()> {
            Ok(())
        }

        fn send_object_data(&mut self, _buffer: &[u8]) -> anyhow::Result<()> {
            Err(io::Error::new(io::ErrorKind::TimedOut, "ambiguous timeout").into())
        }
    }

    impl BackupTransport for FakeBackupTransport {
        fn send_object_info(&mut self, _object_info: &[u8]) -> anyhow::Result<()> {
            self.object_info_calls += 1;
            Ok(())
        }

        fn send_object_data(&mut self, _buffer: &[u8]) -> anyhow::Result<()> {
            self.object_data_calls += 1;
            Ok(())
        }
    }

    impl BackupExportTransport for WrongFormatExportTransport {
        fn get_object_info(&mut self) -> anyhow::Result<Vec<u8>> {
            let info = crate::ptp::ObjectInfo {
                object_format: crate::ptp::ObjectFormat::FujiRAF,
                compressed_size: 6,
                ..Default::default()
            };
            let mut bytes = encode(&info)?;
            bytes.extend_from_slice(&[0; BACKUP_OBJECT_INFO_PADDING_BYTES]);
            Ok(bytes)
        }

        fn get_object_data(&mut self) -> anyhow::Result<Vec<u8>> {
            self.object_data_calls += 1;
            Ok(b"backup".to_vec())
        }
    }

    impl BackupExportTransport for WrongLengthExportTransport {
        fn get_object_info(&mut self) -> anyhow::Result<Vec<u8>> {
            let info = crate::ptp::ObjectInfo {
                object_format: crate::ptp::ObjectFormat::FujiBackup,
                compressed_size: 7,
                ..Default::default()
            };
            let mut bytes = encode(&info)?;
            bytes.extend_from_slice(&[0; BACKUP_OBJECT_INFO_PADDING_BYTES]);
            Ok(bytes)
        }

        fn get_object_data(&mut self) -> anyhow::Result<Vec<u8>> {
            Ok(b"backup".to_vec())
        }
    }

    #[test]
    fn arbitrary_opaque_backup_is_rejected_before_transport() {
        let mut transport = FakeBackupTransport::default();

        let result = import_backup_artifact_with_transport(
            &mut transport,
            b"not a Fujifilm backup".to_vec(),
        );

        assert!(
            result.is_err() && transport.object_info_calls == 0 && transport.object_data_calls == 0,
            "invalid backup must be rejected before transport; result={result:?}, object_info_calls={}, object_data_calls={}",
            transport.object_info_calls,
            transport.object_data_calls,
        );
    }

    #[test]
    fn export_rejects_non_backup_object_info_before_reading_data() {
        let mut transport = WrongFormatExportTransport {
            object_data_calls: 0,
        };

        let result = export_backup_with_transport(&mut transport);

        assert!(
            result.is_err() && transport.object_data_calls == 0,
            "non-backup metadata must stop export; result={result:?}, data calls={}",
            transport.object_data_calls
        );
    }

    #[test]
    fn export_rejects_payload_length_mismatch() {
        let error = export_backup_with_transport(&mut WrongLengthExportTransport)
            .expect_err("object metadata must match the returned payload length");

        assert!(error.to_string().contains("payload length"));
    }

    #[test]
    fn transport_acceptance_is_explicitly_unverified() {
        let mut transport = FakeBackupTransport::default();

        let accepted = import_backup_with_transport(&mut transport, b"info", b"backup")
            .expect("PTP acceptance should produce a typed, unverified outcome");

        assert_eq!(transport.object_info_calls, 1);
        assert_eq!(transport.object_data_calls, 1);
        assert_eq!(accepted.audit().transport(), OutcomeStatus::Succeeded);
        assert_eq!(accepted.audit().ptp_response(), OutcomeStatus::Succeeded);
        assert_eq!(accepted.audit().semantic(), OutcomeStatus::NotAttempted);
        assert_eq!(accepted.audit().persistence(), OutcomeStatus::NotAttempted);
    }

    #[test]
    fn post_restore_export_mismatch_is_typed_semantic_and_persistence_failure() {
        let expected = sample_artifact(b"requested backup state");
        let observed = sample_artifact(b"different camera state");
        let accepted = import_backup_with_transport(
            &mut FakeBackupTransport::default(),
            b"info",
            expected.payload(),
        )
        .expect("PTP acceptance should remain available for explicit verification");

        let error = accepted
            .verify_post_restore_export(&expected, &observed)
            .expect_err("a different fresh-session export must fail verification");

        assert_eq!(error.audit().transport(), OutcomeStatus::Succeeded);
        assert_eq!(error.audit().ptp_response(), OutcomeStatus::Succeeded);
        assert_eq!(error.audit().semantic(), OutcomeStatus::Failed);
        assert_eq!(error.audit().persistence(), OutcomeStatus::Failed);
        assert_ne!(
            error.expected_payload_sha256(),
            error.observed_payload_sha256()
        );
    }

    #[test]
    fn data_phase_failure_preserves_typed_source_and_marks_state_unknown() {
        let error = import_backup_with_transport(&mut FailingDataTransport, b"info", b"backup")
            .expect_err("data failure must be classified");
        let classified = error
            .downcast_ref::<BackupImportError>()
            .expect("failure must retain the partial-state contract");

        assert_eq!(classified.phase(), BackupImportPhase::ObjectData);
        assert_eq!(classified.state(), BackupImportState::Unknown);
        assert_eq!(
            classified
                .source
                .downcast_ref::<io::Error>()
                .map(io::Error::kind),
            Some(io::ErrorKind::TimedOut)
        );
    }

    #[test]
    fn object_info_failure_preserves_typed_source_and_marks_state_unknown() {
        let error = import_backup_with_transport(&mut FailingInfoTransport, b"info", b"backup")
            .expect_err("object-info failure must be classified");
        let classified = error
            .downcast_ref::<BackupImportError>()
            .expect("failure must retain the partial-state contract");

        assert_eq!(classified.phase(), BackupImportPhase::ObjectInfo);
        assert_eq!(classified.state(), BackupImportState::Unknown);
        assert_eq!(
            classified
                .source
                .downcast_ref::<io::Error>()
                .map(io::Error::kind),
            Some(io::ErrorKind::TimedOut)
        );
    }

    #[test]
    fn binrw_backup_object_info_encoding_preserves_wire_layout() {
        let value = BackupObjectInfo::new(0x1234).expect("backup object info must be valid");
        let encoded = encode(&value).expect("binrw backup object info encoding must succeed");

        assert_eq!(encoded.len(), BACKUP_OBJECT_INFO_BYTES);
        assert_eq!(&encoded[..12], [0, 0, 0, 0, 0, 80, 0, 0, 52, 18, 0, 0]);
        assert!(encoded[56..].iter().all(|byte| *byte == 0));
    }
}
