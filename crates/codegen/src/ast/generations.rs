use serde::Deserialize;

use crate::ast::CapabilitySet;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Generation {
    pub id: String,
    pub spec: GenerationSpec,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GenerationSpec {
    pub name: String,
    pub capabilities: Option<CapabilitySet>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generation_spec_accepts_default_option_capabilities() {
        let result = serde_json::from_str::<GenerationSpec>(
            r#"{
                "name": "X-Trans V",
                "capabilities": {
                    "option_overrides": [{
                        "ref": "film_simulation",
                        "allowed_values": ["provia"]
                    }]
                }
            }"#,
        );

        assert!(
            result.is_ok(),
            "generation capabilities must parse: {result:?}"
        );
    }
}
