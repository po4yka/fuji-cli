use std::{
    fmt,
    io::{Seek, Write},
};

use binrw::{BinResult, BinWrite, Endian};
use log::debug;

use crate::{
    features::base::CameraBase,
    ptp::{CommandCode, ObjectFormat, ObjectInfo, Ptp},
};

pub const OBJECT_HANDLE: [u32; 1] = [0x0];
pub const EXPORT_OBJECT_INFO_HANDLE: [u32; 1] = [0x0];
pub const IMPORT_OBJECT_INFO_HANDLE: [u32; 2] = [0x0, 0x0];

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

pub fn import_backup_over_ptp(ptp: &mut Ptp, buffer: &[u8]) -> anyhow::Result<()> {
    let object_info = BackupObjectInfo::new(buffer.len())?;
    let object_info = crate::ptp::codec::encode(&object_info)?;
    import_backup_with_transport(ptp, &object_info, buffer)
}

// NOTE: Naively assuming that all cameras backup/restore in the same way.
pub trait CameraBackupManager: CameraBase {
    fn export_backup(&self, ptp: &mut Ptp) -> anyhow::Result<Vec<u8>> {
        debug!("Starting backup export");
        let _ = ptp.send(CommandCode::GetObjectInfo, &EXPORT_OBJECT_INFO_HANDLE, None)?;
        let response = ptp.send(CommandCode::GetObject, &OBJECT_HANDLE, None)?;
        debug!("Backup export completed");

        Ok(response)
    }

    fn import_backup(&self, ptp: &mut Ptp, buffer: &[u8]) -> anyhow::Result<()> {
        debug!("Starting backup import");
        import_backup_over_ptp(ptp, buffer)?;
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
        writer.write_all(&[0x0u8; 1020])?;
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
        BackupImportError, BackupImportPhase, BackupImportState, BackupObjectInfo, BackupTransport,
        import_backup_with_transport,
    };

    struct FailingDataTransport;

    struct FailingInfoTransport;

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

        assert_eq!(encoded.len(), 1076);
        assert_eq!(&encoded[..12], [0, 0, 0, 0, 0, 80, 0, 0, 52, 18, 0, 0]);
        assert!(encoded[56..].iter().all(|byte| *byte == 0));
    }
}
