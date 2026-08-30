use std::{fmt, str::FromStr};

use anyhow::bail;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PhysicalUsbIdentity {
    pub vendor_id: u16,
    pub product_id: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LogicalCameraIdentity {
    pub vendor_id: u16,
    pub product_id: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModelBindingKind {
    Native,
    Emulated,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommandRisk {
    ReadOnly,
    TransientStateChange,
    PersistentSettingsWrite,
    OpaqueRestore,
    DestructiveRecoverySensitive,
    EmulationForbidden,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EmulationAcknowledgement {
    NotProvided,
    Provided,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SerialFingerprint(String);

impl SerialFingerprint {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SerialFingerprint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl FromStr for SerialFingerprint {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        anyhow::ensure!(
            value.len() == 64
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
            "serial fingerprint must contain exactly 64 lowercase hexadecimal characters"
        );
        Ok(Self(value.to_owned()))
    }
}

pub fn authorize(
    binding: ModelBindingKind,
    risk: CommandRisk,
    acknowledgement: EmulationAcknowledgement,
) -> anyhow::Result<()> {
    match binding {
        ModelBindingKind::Native => match risk {
            CommandRisk::ReadOnly | CommandRisk::EmulationForbidden => Ok(()),
            CommandRisk::TransientStateChange
            | CommandRisk::PersistentSettingsWrite
            | CommandRisk::OpaqueRestore
            | CommandRisk::DestructiveRecoverySensitive => {
                bail!("native state-changing access requires a validated camera preflight")
            }
        },
        ModelBindingKind::Unknown => {
            bail!("camera operations require a known physical camera identity")
        }
        ModelBindingKind::Emulated => match risk {
            CommandRisk::ReadOnly => Ok(()),
            CommandRisk::TransientStateChange => {
                if acknowledgement == EmulationAcknowledgement::Provided {
                    Ok(())
                } else {
                    bail!("emulated transient state changes require explicit acknowledgement")
                }
            }
            CommandRisk::PersistentSettingsWrite => {
                bail!("emulated camera access cannot write persistent settings")
            }
            CommandRisk::OpaqueRestore => {
                bail!("emulated camera access cannot restore opaque data")
            }
            CommandRisk::DestructiveRecoverySensitive => {
                bail!("emulated camera access cannot perform destructive operations")
            }
            CommandRisk::EmulationForbidden => {
                bail!("--emulate is not supported for this command")
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::{
        CommandRisk, EmulationAcknowledgement, ModelBindingKind, SerialFingerprint, authorize,
    };

    #[test]
    fn emulated_camera_access_rejects_persistent_settings_writes() {
        let result = authorize(
            ModelBindingKind::Emulated,
            CommandRisk::PersistentSettingsWrite,
            EmulationAcknowledgement::NotProvided,
        );

        assert!(result.is_err());
    }

    #[test]
    fn native_state_change_is_not_authorized_by_the_general_policy() {
        let result = authorize(
            ModelBindingKind::Native,
            CommandRisk::PersistentSettingsWrite,
            EmulationAcknowledgement::NotProvided,
        );

        assert!(result.is_err());
    }

    #[test]
    fn emulated_transient_state_change_requires_explicit_acknowledgement() {
        let result = authorize(
            ModelBindingKind::Emulated,
            CommandRisk::TransientStateChange,
            EmulationAcknowledgement::NotProvided,
        );

        assert!(result.is_err());
    }

    #[test]
    fn emulated_transient_state_change_allows_explicit_acknowledgement() {
        let result = authorize(
            ModelBindingKind::Emulated,
            CommandRisk::TransientStateChange,
            EmulationAcknowledgement::Provided,
        );

        assert!(result.is_ok());
    }

    #[test]
    fn emulated_read_only_access_is_allowed_without_acknowledgement() {
        let result = authorize(
            ModelBindingKind::Emulated,
            CommandRisk::ReadOnly,
            EmulationAcknowledgement::NotProvided,
        );

        assert!(result.is_ok());
    }

    #[test]
    fn emulated_destructive_access_is_rejected_even_with_acknowledgement() {
        let result = authorize(
            ModelBindingKind::Emulated,
            CommandRisk::DestructiveRecoverySensitive,
            EmulationAcknowledgement::Provided,
        );

        assert!(result.is_err());
    }

    #[test]
    fn emulation_forbidden_is_rejected_even_with_acknowledgement() {
        let error = authorize(
            ModelBindingKind::Emulated,
            CommandRisk::EmulationForbidden,
            EmulationAcknowledgement::Provided,
        )
        .expect_err("irrelevant emulation must be rejected");

        assert_eq!(
            error.to_string(),
            "--emulate is not supported for this command"
        );
    }

    #[test]
    fn serial_fingerprint_rejects_untrusted_shape() {
        assert!(SerialFingerprint::from_str("ABC").is_err());
        assert!(SerialFingerprint::from_str(&"0".repeat(64)).is_ok());
    }
}
