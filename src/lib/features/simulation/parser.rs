use crate::features::simulation::Simulation;

pub trait CameraSimulationParser {
    fn deserialize_simulation(&self, simulation: &[u8]) -> anyhow::Result<Box<dyn Simulation>>;

    fn serialize_simulation(&self, simulation: &dyn Simulation) -> anyhow::Result<Vec<u8>>;
}

#[cfg(test)]
mod tests {
    use crate::generated::cameras::XS20;

    use super::CameraSimulationParser;

    #[test]
    fn import_rejects_an_empty_incomplete_profile() {
        let error = match XS20.deserialize_simulation(b"{}") {
            Ok(_) => panic!("an imported profile must contain all required settings"),
            Err(error) => error,
        };

        assert!(
            error.to_string().contains("required setting"),
            "unexpected error: {error:#}",
        );
    }

    #[test]
    fn import_rejects_a_misspelled_setting() {
        let error = match XS20.deserialize_simulation(br#"{"filmSimulaton": 1}"#) {
            Ok(_) => panic!("unknown JSON fields must not be ignored"),
            Err(error) => error,
        };

        assert!(
            error.to_string().contains("unknown field"),
            "unexpected error: {error:#}",
        );
    }
}
