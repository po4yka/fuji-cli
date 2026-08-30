pub mod artifact;
pub(crate) mod manager;

pub use artifact::{
    BACKUP_ARTIFACT_FORMAT_VERSION, BackupArtifact, BackupIdentity, BackupManifest, BackupPurpose,
    MAX_BACKUP_ARTIFACT_BYTES, MAX_BACKUP_PAYLOAD_BYTES, sha256_hex,
};
pub(crate) use manager::CameraBackupManager;
pub use manager::{
    BackupImportError, BackupImportPhase, BackupImportState, BackupPostconditionError,
    BackupRestoreAccepted, BackupRestoreOutcome,
};
