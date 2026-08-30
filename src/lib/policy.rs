use anyhow::bail;

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
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EmulationPolicy {
    Allowed,
    RequireTransientWriteAcknowledgement,
    Forbidden,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommandSpec {
    pub risk: CommandRisk,
    pub emulation: EmulationPolicy,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EmulationAcknowledgement {
    NotProvided,
    Provided,
}

pub fn authorize(
    binding: ModelBindingKind,
    spec: CommandSpec,
    acknowledgement: EmulationAcknowledgement,
) -> anyhow::Result<()> {
    match binding {
        ModelBindingKind::Native => return Ok(()),
        ModelBindingKind::Unknown => {
            bail!("high-level camera operations require a known physical model")
        }
        ModelBindingKind::Emulated => {}
    }

    match spec.emulation {
        EmulationPolicy::Allowed => Ok(()),
        EmulationPolicy::RequireTransientWriteAcknowledgement
            if acknowledgement == EmulationAcknowledgement::Provided =>
        {
            Ok(())
        }
        EmulationPolicy::RequireTransientWriteAcknowledgement => {
            bail!("emulated transient state changes require --allow-emulated-transient-write")
        }
        EmulationPolicy::Forbidden => match spec.risk {
            CommandRisk::ReadOnly | CommandRisk::TransientStateChange => {
                bail!("--emulate is not supported for this command")
            }
            CommandRisk::PersistentSettingsWrite => {
                bail!("emulated camera access cannot write persistent settings")
            }
            CommandRisk::OpaqueRestore => {
                bail!("emulated camera access cannot restore opaque camera state")
            }
            CommandRisk::DestructiveRecoverySensitive => bail!(
                "emulated camera access cannot perform destructive or recovery-sensitive operations"
            ),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emulated_policy_matrix_is_fail_closed() {
        let denied = [
            CommandRisk::PersistentSettingsWrite,
            CommandRisk::OpaqueRestore,
            CommandRisk::DestructiveRecoverySensitive,
        ];
        for risk in denied {
            assert!(
                authorize(
                    ModelBindingKind::Emulated,
                    CommandSpec {
                        risk,
                        emulation: EmulationPolicy::Forbidden
                    },
                    EmulationAcknowledgement::Provided
                )
                .is_err(),
                "{risk:?} must remain denied even with acknowledgement"
            );
        }

        assert!(
            authorize(
                ModelBindingKind::Emulated,
                CommandSpec {
                    risk: CommandRisk::TransientStateChange,
                    emulation: EmulationPolicy::RequireTransientWriteAcknowledgement,
                },
                EmulationAcknowledgement::NotProvided
            )
            .is_err()
        );
        assert!(
            authorize(
                ModelBindingKind::Emulated,
                CommandSpec {
                    risk: CommandRisk::TransientStateChange,
                    emulation: EmulationPolicy::RequireTransientWriteAcknowledgement,
                },
                EmulationAcknowledgement::Provided
            )
            .is_ok()
        );
        assert!(
            authorize(
                ModelBindingKind::Emulated,
                CommandSpec {
                    risk: CommandRisk::ReadOnly,
                    emulation: EmulationPolicy::Allowed,
                },
                EmulationAcknowledgement::NotProvided
            )
            .is_ok()
        );
    }

    #[test]
    fn unknown_model_cannot_enter_high_level_operations() {
        assert!(
            authorize(
                ModelBindingKind::Unknown,
                CommandSpec {
                    risk: CommandRisk::ReadOnly,
                    emulation: EmulationPolicy::Allowed,
                },
                EmulationAcknowledgement::Provided
            )
            .is_err()
        );
    }
}
