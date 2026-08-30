#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum OutcomeStatus {
    NotAttempted,
    Succeeded,
    Failed,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StateChangeAudit {
    transport: OutcomeStatus,
    ptp_response: OutcomeStatus,
    semantic: OutcomeStatus,
    persistence: OutcomeStatus,
}

impl StateChangeAudit {
    pub const fn transport(self) -> OutcomeStatus {
        self.transport
    }

    pub const fn ptp_response(self) -> OutcomeStatus {
        self.ptp_response
    }

    pub const fn semantic(self) -> OutcomeStatus {
        self.semantic
    }

    pub const fn persistence(self) -> OutcomeStatus {
        self.persistence
    }

    pub(crate) const fn not_attempted() -> Self {
        Self {
            transport: OutcomeStatus::NotAttempted,
            ptp_response: OutcomeStatus::NotAttempted,
            semantic: OutcomeStatus::NotAttempted,
            persistence: OutcomeStatus::NotAttempted,
        }
    }

    pub(crate) const fn ptp_accepted() -> Self {
        Self {
            transport: OutcomeStatus::Succeeded,
            ptp_response: OutcomeStatus::Succeeded,
            semantic: OutcomeStatus::NotAttempted,
            persistence: OutcomeStatus::NotAttempted,
        }
    }

    pub(crate) const fn attempt_unknown() -> Self {
        Self {
            transport: OutcomeStatus::Unknown,
            ptp_response: OutcomeStatus::Unknown,
            semantic: OutcomeStatus::Unknown,
            persistence: OutcomeStatus::Unknown,
        }
    }

    pub(crate) fn from_write_error(error: &anyhow::Error) -> Self {
        if let Some(error) = error.downcast_ref::<crate::ptp::error::Error>() {
            match error {
                crate::ptp::error::Error::Response(_) => Self {
                    transport: OutcomeStatus::Succeeded,
                    ptp_response: OutcomeStatus::Failed,
                    semantic: OutcomeStatus::NotAttempted,
                    persistence: OutcomeStatus::NotAttempted,
                },
                crate::ptp::error::Error::Malformed(_) => Self {
                    transport: OutcomeStatus::Succeeded,
                    ptp_response: OutcomeStatus::Unknown,
                    semantic: OutcomeStatus::Unknown,
                    persistence: OutcomeStatus::Unknown,
                },
                crate::ptp::error::Error::Usb(_) | crate::ptp::error::Error::Io(_) => {
                    Self::transport_failed()
                }
            }
        } else if error.downcast_ref::<rusb::Error>().is_some()
            || error.downcast_ref::<std::io::Error>().is_some()
        {
            Self::transport_failed()
        } else {
            Self::attempt_unknown()
        }
    }

    pub(crate) const fn with_semantic(self, semantic: OutcomeStatus) -> Self {
        Self { semantic, ..self }
    }

    pub(crate) const fn with_persistence(self, persistence: OutcomeStatus) -> Self {
        Self {
            persistence,
            ..self
        }
    }

    const fn transport_failed() -> Self {
        Self {
            transport: OutcomeStatus::Failed,
            ptp_response: OutcomeStatus::Unknown,
            semantic: OutcomeStatus::Unknown,
            persistence: OutcomeStatus::Unknown,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{OutcomeStatus, StateChangeAudit};

    #[test]
    fn ptp_rejection_is_distinct_from_transport_failure() {
        let response = anyhow::Error::new(crate::ptp::error::Error::Response(0x2002));
        let response = StateChangeAudit::from_write_error(&response);
        let transport = anyhow::Error::new(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "simulated transport timeout",
        ));
        let transport = StateChangeAudit::from_write_error(&transport);

        assert_eq!(response.transport(), OutcomeStatus::Succeeded);
        assert_eq!(response.ptp_response(), OutcomeStatus::Failed);
        assert_eq!(transport.transport(), OutcomeStatus::Failed);
        assert_eq!(transport.ptp_response(), OutcomeStatus::Unknown);
    }
}
