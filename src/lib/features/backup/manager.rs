use std::{
    fmt,
    io::{Cursor, Seek, Write},
};

use anyhow::ensure;
use binrw::{BinRead, BinResult, BinWrite, Endian};
use log::debug;

use crate::{
    features::{backup::artifact::BackupArtifact, base::CameraBase},
    ptp::{CommandCode, ObjectFormat, ObjectInfo, Ptp},
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
    source: anyhow::Error,
}

impl BackupImportError {
    pub fn phase(&self) -> BackupImportPhase {
        self.phase
    }

    pub fn state(&self) -> BackupImportState {
        self.state
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

trait BackupTransport {
    fn send_object_info(&mut self, object_info: &[u8]) -> anyhow::Result<()>;

    fn send_object_data(&mut self, buffer: &[u8]) -> anyhow::Result<()>;
}

trait BackupExportTransport {
    fn get_object_info(&mut self) -> anyhow::Result<Vec<u8>>;

    fn get_object_data(&mut self) -> anyhow::Result<Vec<u8>>;
}

impl BackupTransport for Ptp {
    fn send_object_info(&mut self, object_info: &[u8]) -> anyhow::Result<()> {
        self.send(
            CommandCode::SendObjectInfo,
            &IMPORT_OBJECT_INFO_HANDLE,
            Some(object_info),
        )?;
        Ok(())
    }

    fn send_object_data(&mut self, buffer: &[u8]) -> anyhow::Result<()> {
        self.send(CommandCode::SendObject, &OBJECT_HANDLE, Some(buffer))?;
        Ok(())
    }
}

impl BackupExportTransport for Ptp {
    fn get_object_info(&mut self) -> anyhow::Result<Vec<u8>> {
        self.send(CommandCode::GetObjectInfo, &EXPORT_OBJECT_INFO_HANDLE, None)
    }

    fn get_object_data(&mut self) -> anyhow::Result<Vec<u8>> {
        self.send(CommandCode::GetObject, &OBJECT_HANDLE, None)
    }
}

fn export_backup_with_transport(
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
) -> anyhow::Result<()> {
    transport
        .send_object_info(object_info)
        .map_err(|source| BackupImportError {
            phase: BackupImportPhase::ObjectInfo,
            state: BackupImportState::Unknown,
            source,
        })?;
    transport
        .send_object_data(buffer)
        .map_err(|source| BackupImportError {
            phase: BackupImportPhase::ObjectData,
            state: BackupImportState::Unknown,
            source,
        })?;
    Ok(())
}

#[cfg(test)]
fn import_backup_artifact_with_transport(
    transport: &mut impl BackupTransport,
    artifact: Vec<u8>,
) -> anyhow::Result<()> {
    let artifact = BackupArtifact::parse(artifact)?;
    let object_info = BackupObjectInfo::new(artifact.payload().len())?;
    let object_info = crate::ptp::codec::encode(&object_info)?;
    import_backup_with_transport(transport, &object_info, artifact.payload())
}

fn import_validated_backup_with_transport(
    transport: &mut impl BackupTransport,
    artifact: &BackupArtifact,
) -> anyhow::Result<()> {
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

    fn import_backup(&self, ptp: &mut Ptp, artifact: &BackupArtifact) -> anyhow::Result<()> {
        debug!("Starting backup import");
        import_validated_backup_with_transport(ptp, artifact)?;
        debug!("Backup import completed");

        Ok(())
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

    use crate::ptp::codec::encode;

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
