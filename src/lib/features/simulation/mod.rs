pub(crate) mod manager;
pub(crate) mod parser;
mod transaction;

pub(crate) use transaction::{
    AuthorizedSimulationIo, SelectedSimulationIo, SimulationPropertyChange, SimulationPropertyIo,
    SimulationPropertyWriteError, SimulationTransactionProfile, execute_simulation_transaction,
    with_restored_simulation_selector, with_temporary_simulation_selector,
};
pub use transaction::{
    SimulationFailureState, SimulationTransactionError, SimulationTransactionPhase,
    SimulationTransactionSuccess, SimulationWriteReceipt, TemporarySimulationSelectorError,
    TemporarySimulationSelectorState,
};

pub(crate) use manager::CameraSimulationManager;
pub(crate) use parser::CameraSimulationParser;

use std::{any::Any, fmt};

use erased_serde::serialize_trait_object;
use serde::Serialize;

use crate::generated::{
    options::{CustomSetting, CustomSettingName},
    simulations::SimulationBase,
};

pub trait Simulation: fmt::Display + erased_serde::Serialize {
    fn as_any(&self) -> &dyn Any;

    fn name(&self) -> Option<CustomSettingName>;

    fn try_update_from(&mut self, partial: SimulationBase) -> anyhow::Result<()>;

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
    use std::str::FromStr as _;

    use crate::generated::options::CustomSettingName;

    #[test]
    fn custom_setting_name_human_output_escapes_terminal_controls() {
        let name = CustomSettingName::from_str("C1\u{1b}]0;pwn\u{7}\r\n")
            .expect("test name must satisfy schema bounds");

        assert_eq!(name.to_string(), "C1\\u{1b}]0;pwn\\u{7}\\r\\n");
    }

    #[test]
    fn custom_setting_name_json_preserves_the_wire_value() {
        let name = CustomSettingName::from_str("C1\u{1b}]0;pwn\u{7}\r\n")
            .expect("test name must satisfy schema bounds");

        assert_eq!(
            serde_json::to_value(name).expect("name must serialize"),
            "C1\u{1b}]0;pwn\u{7}\r\n"
        );
    }
}
