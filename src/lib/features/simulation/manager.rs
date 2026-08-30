use crate::{
    features::{
        base::CameraBase,
        simulation::{
            Simulation, SimulationTransactionError, SimulationTransactionSuccess,
            parser::CameraSimulationParser,
        },
    },
    generated::{options::CustomSetting, simulations::SimulationBase},
    ptp::Ptp,
};

pub trait CameraSimulationManager: CameraBase + CameraSimulationParser {
    fn custom_settings_slots(&self) -> Vec<CustomSetting>;

    fn get_simulation(
        &self,
        ptp: &mut Ptp,
        slot: CustomSetting,
    ) -> anyhow::Result<Box<dyn Simulation>>;

    fn get_simulations(
        &self,
        ptp: &mut Ptp,
        slots: &[CustomSetting],
    ) -> anyhow::Result<Vec<(CustomSetting, Box<dyn Simulation>)>>;

    fn update_simulation(
        &self,
        ptp: &mut Ptp,
        slot: CustomSetting,
        partial: SimulationBase,
    ) -> Result<SimulationTransactionSuccess, SimulationTransactionError>;

    fn set_simulation(
        &self,
        ptp: &mut Ptp,
        slot: CustomSetting,
        simulation: &dyn Simulation,
    ) -> Result<SimulationTransactionSuccess, SimulationTransactionError>;
}
