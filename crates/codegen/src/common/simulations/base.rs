use std::collections::BTreeMap;

use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use crate::{
    ast::{Camera, FujiOption, SpecKind},
    schema::grammar::build_settings,
};

struct UnionEntry {
    id: String,
    type_path: TokenStream,
    option_id: Option<String>,
    kind: SpecKind,
}

pub fn generate(
    options: &BTreeMap<String, FujiOption>,
    cameras: &BTreeMap<String, Camera>,
) -> anyhow::Result<TokenStream> {
    let union = build_union(options, cameras)?;
    Ok(generate_struct_def(&union))
}

fn build_union(
    options: &BTreeMap<String, FujiOption>,
    cameras: &BTreeMap<String, Camera>,
) -> anyhow::Result<Vec<UnionEntry>> {
    let by_id = cameras
        .values()
        .filter_map(|camera| camera.spec.features.as_ref()?.simulation.as_ref())
        .try_fold(
            BTreeMap::<String, UnionEntry>::new(),
            |mut by_id, simulation| -> anyhow::Result<_> {
                let settings = build_settings(options, &simulation.settings)?;
                simulation.settings.iter().for_each(|setting| {
                    let id = setting.id.clone();
                    let info = settings
                        .get(id.as_str())
                        .expect("ctx was just built from these settings");
                    by_id.entry(id.clone()).or_insert_with(|| UnionEntry {
                        id,
                        type_path: info.type_path(),
                        option_id: info.option.map(|option| option.id.clone()),
                        kind: info.kind,
                    });
                });
                Ok(by_id)
            },
        )?;
    Ok(by_id.into_values().collect())
}

fn generate_struct_def(union: &[UnionEntry]) -> TokenStream {
    let base_fields = union.iter().map(|entry| {
        let ident = format_ident!("{}", entry.id);
        let ty = &entry.type_path;
        quote! {
            #[serde(skip_serializing_if = "Option::is_none")]
            pub #ident: Option<#ty>,
        }
    });
    let capability_validations = union.iter().filter_map(|entry| {
        if entry.kind != SpecKind::Enum {
            return None;
        }
        let option_id = entry.option_id.as_ref()?;
        let ident = format_ident!("{}", entry.id);
        Some(quote! {
            if let Some(value) = self.#ident.as_ref() {
                profile.validate_option_value(#option_id, value.capability_value_id())?;
            }
        })
    });

    quote! {
        #[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
        #[serde(default, deny_unknown_fields, rename_all = "camelCase")]
        pub struct SimulationBase {
            #( #base_fields )*
        }

        impl SimulationBase {
            pub(crate) fn validate_firmware_capabilities(
                &self,
                profile: &crate::generated::cameras::CameraFirmwareCapabilityProfile,
            ) -> ::anyhow::Result<()> {
                #(#capability_validations)*
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use proc_macro2::TokenStream;

    use crate::ast::SpecKind;

    use super::{UnionEntry, generate_struct_def};

    #[test]
    fn simulation_base_rejects_unknown_json_fields() {
        let generated = generate_struct_def(&[]).to_string();

        assert!(
            generated.contains("deny_unknown_fields"),
            "generated SimulationBase must reject misspelled fields:\n{generated}",
        );
    }

    #[test]
    fn simulation_base_exposes_firmware_capability_validation() {
        let generated = generate_struct_def(&[]).to_string();

        assert!(
            generated.contains("validate_firmware_capabilities"),
            "all simulation inputs need a pre-mutation firmware validation seam: {generated}"
        );
    }

    #[test]
    fn simulation_base_validates_enum_values_against_firmware_profile() {
        let generated = generate_struct_def(&[UnionEntry {
            id: "film_simulation".to_owned(),
            type_path: "crate::generated::options::FilmSimulation"
                .parse::<TokenStream>()
                .expect("type path must parse"),
            option_id: Some("film_simulation".to_owned()),
            kind: SpecKind::Enum,
        }])
        .to_string();

        assert!(
            generated.contains("validate_option_value")
                && generated.contains("capability_value_id"),
            "enum logical values must be checked against exact firmware: {generated}"
        );
    }
}
