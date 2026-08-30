use std::collections::{BTreeMap, BTreeSet};

use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use crate::{
    ast::{Camera, FujiOption, SpecKind},
    common::simulations,
    schema::grammar::build_settings,
};

struct UnionEntry {
    id: String,
    type_path: TokenStream,
    is_copy: bool,
    option_id: Option<String>,
    kind: SpecKind,
}

pub fn generate(
    options: &BTreeMap<String, FujiOption>,
    cameras: &BTreeMap<String, Camera>,
) -> anyhow::Result<TokenStream> {
    let render_union = build_union(options, cameras)?;
    let simulation_field_ids = collect_simulation_field_ids(cameras);

    let struct_def = generate_struct_def(&render_union);
    let merge_impl = generate_merge_impl(&render_union);
    let apply_simulation_impl =
        generate_apply_simulation_impl(&render_union, &simulation_field_ids);

    Ok(quote! {
        #struct_def
        #merge_impl
        #apply_simulation_impl
    })
}

fn build_union(
    options: &BTreeMap<String, FujiOption>,
    cameras: &BTreeMap<String, Camera>,
) -> anyhow::Result<Vec<UnionEntry>> {
    let by_id = cameras
        .values()
        .filter_map(|camera| camera.spec.features.as_ref()?.render.as_ref())
        .try_fold(
            BTreeMap::<String, UnionEntry>::new(),
            |mut by_id, render| -> anyhow::Result<_> {
                let settings = build_settings(options, &render.fields)?;
                render.fields.iter().for_each(|field| {
                    let id = field.id().to_string();
                    let info = settings
                        .get(id.as_str())
                        .expect("ctx was just built from these fields");
                    by_id.entry(id.clone()).or_insert_with(|| UnionEntry {
                        id,
                        type_path: info.type_path(),
                        is_copy: !matches!(info.kind, SpecKind::String),
                        option_id: info.option.map(|option| option.id.clone()),
                        kind: info.kind,
                    });
                });
                Ok(by_id)
            },
        )?;
    Ok(by_id.into_values().collect())
}

fn collect_simulation_field_ids(cameras: &BTreeMap<String, Camera>) -> BTreeSet<String> {
    cameras
        .values()
        .filter_map(|camera| camera.spec.features.as_ref()?.simulation.as_ref())
        .flat_map(|simulation| simulation.settings.iter().map(|s| s.id.clone()))
        .collect()
}

fn generate_struct_def(union: &[UnionEntry]) -> TokenStream {
    let fields = union.iter().map(|entry| {
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
                let logical = value.capability_value_id();
                profile.validate_option_value(#option_id, logical)?;
                profile.write_wire_value(#option_id, logical)?;
            }
        })
    });

    quote! {
        #[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
        #[serde(default, rename_all = "camelCase")]
        pub struct RenderBase {
            #( #fields )*
        }

        impl RenderBase {
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

fn generate_merge_impl(union: &[UnionEntry]) -> TokenStream {
    let assigns = union.iter().map(|entry| {
        let ident = format_ident!("{}", entry.id);
        quote! {
            if let Some(value) = overlay.#ident {
                self.#ident = Some(value);
            }
        }
    });

    quote! {
        impl RenderBase {
            pub fn merge(&mut self, overlay: Self) {
                #( #assigns )*
            }
        }
    }
}

fn generate_apply_simulation_impl(
    union: &[UnionEntry],
    simulation_field_ids: &BTreeSet<String>,
) -> TokenStream {
    let simulations_path = simulations::path();
    let assigns = union
        .iter()
        .filter(|entry| simulation_field_ids.contains(&entry.id))
        .map(|entry| {
            let ident = format_ident!("{}", entry.id);
            let access = if entry.is_copy {
                quote! { simulation.#ident }
            } else {
                quote! { simulation.#ident.clone() }
            };

            quote! {
                if let Some(value) = #access {
                    self.#ident = Some(value);
                }
            }
        });

    quote! {
        impl RenderBase {
            pub fn try_update_from(
                &mut self,
                simulation: &#simulations_path::SimulationBase,
            ) {
                #( #assigns )*
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
    fn render_base_exposes_firmware_capability_validation() {
        let generated = generate_struct_def(&[]).to_string();

        assert!(
            generated.contains("validate_firmware_capabilities"),
            "render input needs a pre-upload firmware validation seam: {generated}"
        );
    }

    #[test]
    fn render_base_validates_enum_values_against_firmware_profile() {
        let generated = generate_struct_def(&[UnionEntry {
            id: "film_simulation".to_owned(),
            type_path: "crate::generated::options::FilmSimulation"
                .parse::<TokenStream>()
                .expect("type path must parse"),
            is_copy: true,
            option_id: Some("film_simulation".to_owned()),
            kind: SpecKind::Enum,
        }])
        .to_string();

        assert!(
            generated.contains("validate_option_value")
                && generated.contains("capability_value_id"),
            "render enum values must be checked against exact firmware: {generated}"
        );
    }

    #[test]
    fn render_base_accepts_firmware_scoped_wire_encoding() {
        let generated = generate_struct_def(&[UnionEntry {
            id: "film_simulation".to_owned(),
            type_path: "crate::generated::options::FilmSimulation"
                .parse::<TokenStream>()
                .expect("type path must parse"),
            is_copy: true,
            option_id: Some("film_simulation".to_owned()),
            kind: SpecKind::Enum,
        }])
        .to_string();

        assert!(generated.contains("write_wire_value"));
        assert!(!generated.contains("capability_global_wire_value"));
    }
}
