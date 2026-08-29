pub mod manager;
pub mod parser;

pub use manager::CameraSimulationManager;
pub use parser::CameraSimulationParser;

use std::{any::Any, fmt};

use erased_serde::serialize_trait_object;
use serde::Serialize;

use crate::{
    generated::{
        options::{CustomSetting, CustomSettingName},
        simulations::SimulationBase,
    },
    ptp::Ptp,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimulationApplyState {
    OriginalRestored,
    Unknown,
}

#[derive(Debug)]
pub struct SimulationApplyError {
    apply: anyhow::Error,
    rollback: Option<anyhow::Error>,
    state: SimulationApplyState,
}

impl SimulationApplyError {
    pub fn state(&self) -> SimulationApplyState {
        self.state
    }

    pub fn apply_error(&self) -> &anyhow::Error {
        &self.apply
    }

    pub fn rollback_error(&self) -> Option<&anyhow::Error> {
        self.rollback.as_ref()
    }
}

impl fmt::Display for SimulationApplyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.state {
            SimulationApplyState::OriginalRestored => {
                write!(
                    formatter,
                    "failed to apply simulation; original settings were restored"
                )
            }
            SimulationApplyState::Unknown => {
                if let Some(rollback) = &self.rollback {
                    write!(
                        formatter,
                        "failed to apply simulation and rollback failed: {rollback}; simulation state is unknown"
                    )
                } else {
                    write!(
                        formatter,
                        "failed to apply simulation; simulation state is unknown"
                    )
                }
            }
        }
    }
}

impl std::error::Error for SimulationApplyError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.apply.as_ref())
    }
}

pub fn finish_failed_simulation_apply(
    apply: anyhow::Error,
    rollback: impl FnOnce() -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    match rollback() {
        Ok(()) => Err(SimulationApplyError {
            apply,
            rollback: None,
            state: SimulationApplyState::OriginalRestored,
        }
        .into()),
        Err(rollback) => Err(SimulationApplyError {
            apply,
            rollback: Some(rollback),
            state: SimulationApplyState::Unknown,
        }
        .into()),
    }
}

pub trait Simulation: fmt::Display + erased_serde::Serialize {
    fn as_any(&self) -> &dyn Any;

    fn name(&self) -> Option<CustomSettingName>;

    fn try_update_from(&mut self, partial: SimulationBase) -> anyhow::Result<()>;

    fn try_pull(ptp: &mut crate::ptp::Ptp) -> anyhow::Result<Self>
    where
        Self: Sized;

    fn try_push(&self, ptp: &mut Ptp) -> anyhow::Result<()>;

    fn to_base(&self) -> SimulationBase;
}

serialize_trait_object!(Simulation);

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SimulationListItem {
    pub slot: CustomSetting,
    pub name: Option<CustomSettingName>,
}

impl fmt::Display for SimulationListItem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.name {
            Some(name) => write!(f, "{}: {}", self.slot, name),
            None => write!(f, "{}: <unnamed>", self.slot),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{error::Error, fmt};

    use super::{SimulationApplyError, SimulationApplyState, finish_failed_simulation_apply};

    #[derive(Debug)]
    struct MarkerError(&'static str);

    impl fmt::Display for MarkerError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str(self.0)
        }
    }

    impl Error for MarkerError {}

    #[test]
    fn failed_rollback_preserves_both_typed_errors_and_unknown_state() {
        let error = finish_failed_simulation_apply(MarkerError("apply").into(), || {
            Err(MarkerError("rollback sentinel details").into())
        })
        .expect_err("apply and rollback failures must be classified");
        let classified = error
            .downcast_ref::<SimulationApplyError>()
            .expect("failed rollback must retain a structured error");

        assert_eq!(classified.state(), SimulationApplyState::Unknown);
        assert_eq!(
            classified
                .apply_error()
                .downcast_ref::<MarkerError>()
                .map(|error| error.0),
            Some("apply")
        );
        assert_eq!(
            classified
                .rollback_error()
                .and_then(|error| error.downcast_ref::<MarkerError>())
                .map(|error| error.0),
            Some("rollback sentinel details")
        );
        assert!(
            classified.to_string().contains("rollback sentinel details"),
            "CLI-facing diagnostics must expose that rollback failed: {classified}"
        );
    }
}
