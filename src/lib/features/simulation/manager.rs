use crate::{
    features::{
        base::CameraBase,
        simulation::{
            Simulation, SimulationTransactionError, SimulationTransactionSuccess,
            parser::CameraSimulationParser,
        },
    },
    generated::{options::CustomSetting, simulations::SimulationBase},
};

pub(crate) trait CameraSimulationManager: CameraBase + CameraSimulationParser {
    fn custom_settings_slots(&self) -> Vec<CustomSetting>;

    fn get_simulation(
        &self,
        io: &mut crate::features::simulation::AuthorizedSimulationIo<'_>,
        slot: CustomSetting,
    ) -> anyhow::Result<Box<dyn Simulation>>;

    fn get_simulations(
        &self,
        io: &mut crate::features::simulation::AuthorizedSimulationIo<'_>,
        slots: &[CustomSetting],
    ) -> anyhow::Result<Vec<(CustomSetting, Box<dyn Simulation>)>>;

    fn update_simulation(
        &self,
        io: &mut crate::features::simulation::AuthorizedSimulationIo<'_>,
        slot: CustomSetting,
        partial: SimulationBase,
    ) -> Result<SimulationTransactionSuccess, SimulationTransactionError>;

    fn set_simulation(
        &self,
        io: &mut crate::features::simulation::AuthorizedSimulationIo<'_>,
        slot: CustomSetting,
        simulation: &dyn Simulation,
    ) -> Result<SimulationTransactionSuccess, SimulationTransactionError>;
}
