use std::collections::{BTreeMap, BTreeSet};

use anyhow::Context;
use proc_macro2::{Ident, TokenStream};
use quote::{format_ident, quote};

use crate::{
    ast::{Camera, Dnf, FujiOption, Leaf, Setting},
    common::{cameras, options},
    schema::{
        alias::{NormalizedRule, NormalizedTransformation},
        grammar::{
            Scopes, SettingInfo, build_settings, generate_apply_transformations, generate_dnf,
            generate_emit_warnings_and_infos,
        },
        presence::PresenceDag,
        repair::{generate_pin_set, generate_solve},
    },
    util::{dag::Dag, ident::safe_upper_camel_case_ident},
};

pub fn generate(
    options: &BTreeMap<String, FujiOption>,
    cameras: &BTreeMap<String, Camera>,
) -> anyhow::Result<TokenStream> {
    let base_union = collect_base_union(cameras);
    let mut blocks = Vec::with_capacity(cameras.len());
    for camera in cameras.values() {
        let block = generate_one(options, camera, &base_union)
            .with_context(|| format!("generating simulation for camera `{}`", camera.id))?;
        blocks.push(block);
    }
    Ok(quote! { #( #blocks )* })
}

fn collect_base_union(cameras: &BTreeMap<String, Camera>) -> BTreeSet<String> {
    cameras
        .values()
        .filter_map(|c| c.spec.features.as_ref()?.simulation.as_ref())
        .flat_map(|s| s.settings.iter().map(|setting| setting.id.clone()))
        .collect()
}

fn generate_one(
    options: &BTreeMap<String, FujiOption>,
    camera: &Camera,
    base_union: &BTreeSet<String>,
) -> anyhow::Result<TokenStream> {
    let Some(simulation) = camera
        .spec
        .features
        .as_ref()
        .and_then(|f| f.simulation.as_ref())
    else {
        return Ok(quote! {});
    };

    let settings = build_settings(options, &simulation.settings)?;

    let aliases: Vec<NormalizedTransformation> = simulation
        .transformations
        .iter()
        .cloned()
        .filter_map(Option::from)
        .collect();
    let effective_rules: Vec<NormalizedRule> = simulation
        .rules
        .iter()
        .map(|r| NormalizedRule::from_rule(r, &aliases))
        .collect();

    let struct_ident = format_ident!("{}Simulation", safe_upper_camel_case_ident(&camera.id));
    let camera_struct_ident = safe_upper_camel_case_ident(&camera.id);
    let cameras_path = cameras::path();
    let camera_struct_path = quote! { #cameras_path::#camera_struct_ident };
    let options_path = options::path();

    let presence_info = PresenceDag::try_from_rules(&effective_rules)
        .with_context(|| format!("extracting presence DAG for `{}`", camera.id))?;

    let nodes: Vec<&str> = simulation.settings.iter().map(|s| s.id.as_str()).collect();
    let edges: Vec<(&str, &str)> = presence_info
        .edges
        .iter()
        .map(|(from, to)| (from.as_str(), to.as_str()))
        .collect();

    let write_order: Vec<String> = Dag::new(nodes, edges)
        .topological_order()?
        .into_iter()
        .map(str::to_owned)
        .collect();
    let read_order = write_order.clone();

    let struct_def = generate_struct_def(&settings, &simulation.settings, &struct_ident);
    let inherent_impl = generate_inherent_impl(
        &settings,
        simulation,
        &effective_rules,
        &struct_ident,
        &options_path,
    )?;
    let from_sim_impl =
        generate_from_sim_for_base_impl(&settings, &simulation.settings, &struct_ident, base_union);
    let try_from_base_impl = generate_try_from_base_impl(
        &settings,
        &simulation.settings,
        &effective_rules,
        &struct_ident,
    );
    let display_impl = generate_display_impl(&settings, &simulation.settings, &struct_ident);
    let transaction_impl = generate_transaction_profile_impl(
        &settings,
        &struct_ident,
        &read_order,
        &write_order,
        &presence_info.conditions,
    )?;
    let simulation_impl = generate_simulation_impl(&struct_ident, &options_path);
    let parser_impl = generate_parser_impl(&struct_ident, &camera_struct_path);
    let manager_impl = generate_manager_impl(&struct_ident, &camera_struct_path, &options_path);

    Ok(quote! {
        #struct_def
        #inherent_impl
        #from_sim_impl
        #try_from_base_impl
        #display_impl
        #transaction_impl
        #simulation_impl
        #parser_impl
        #manager_impl
    })
}

fn generate_struct_def(
    settings: &BTreeMap<&str, SettingInfo<'_>>,
    fields: &[Setting],
    struct_ident: &Ident,
) -> TokenStream {
    let field_defs = fields.iter().map(|s| {
        let info = settings.get(s.id.as_str()).expect("settings indexed");
        let ident = info.field_ident();
        let type_path = info.type_path();
        quote! {
            #[serde(skip_serializing_if = "Option::is_none")]
            pub #ident: Option<#type_path>,
        }
    });

    quote! {
        #[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
        #[serde(default, deny_unknown_fields, rename_all = "camelCase")]
        pub struct #struct_ident {
            #( #field_defs )*
        }
    }
}

fn generate_inherent_impl(
    settings: &BTreeMap<&str, SettingInfo<'_>>,
    simulation: &crate::ast::Simulation,
    effective_rules: &[NormalizedRule],
    struct_ident: &Ident,
    options_path: &TokenStream,
) -> anyhow::Result<TokenStream> {
    let slots = simulation.slots;

    let apply_transformations =
        generate_apply_transformations(settings, &simulation.transformations)?;
    let self_acc = quote! { self };
    let warnings_infos =
        generate_emit_warnings_and_infos(settings, effective_rules, Scopes::new(&self_acc))?;
    let solve = generate_solve(settings, effective_rules, false)?;
    let try_update_from = generate_try_update_from(settings, &simulation.settings, struct_ident)?;
    let name = generate_name(settings, options_path);

    Ok(quote! {
        impl #struct_ident {
            pub const SLOTS: u32 = #slots;

            #apply_transformations
            #warnings_infos
            #solve
            #try_update_from
            #name
        }
    })
}

fn generate_try_update_from(
    settings: &BTreeMap<&str, SettingInfo<'_>>,
    fields: &[Setting],
    struct_ident: &Ident,
) -> anyhow::Result<TokenStream> {
    let init_fields = fields.iter().map(|s| {
        let info = settings.get(s.id.as_str()).expect("settings indexed");
        let ident = info.field_ident();
        quote! { #ident: partial.#ident, }
    });

    let merge_assigns = fields.iter().map(|s| {
        let info = settings.get(s.id.as_str()).expect("settings indexed");
        let ident = info.field_ident();
        quote! {
            if let Some(value) = partial_profile.#ident.take() {
                candidate.#ident = Some(value);
            }
        }
    });

    let pin_set_expr = generate_pin_set(settings, &quote! { partial_profile });

    Ok(quote! {
        pub fn try_update_from(
            &mut self,
            partial: crate::generated::simulations::SimulationBase,
        ) -> ::anyhow::Result<()> {
            let mut partial_profile: #struct_ident = #struct_ident {
                #( #init_fields )*
            };
            partial_profile.apply_transformations();

            let pin = #pin_set_expr;

            let mut candidate = self.clone();
            #( #merge_assigns )*
            candidate.apply_transformations();

            candidate.solve(&pin)?;
            candidate.emit_warnings_and_infos()?;

            *self = candidate;
            Ok(())
        }
    })
}

fn generate_name(
    settings: &BTreeMap<&str, SettingInfo<'_>>,
    options_path: &TokenStream,
) -> TokenStream {
    let body = if settings.contains_key("custom_setting_name") {
        quote! { self.custom_setting_name.clone() }
    } else {
        quote! { None }
    };
    quote! {
        pub fn name(&self) -> Option<#options_path::CustomSettingName> {
            #body
        }
    }
}

fn generate_from_sim_for_base_impl(
    settings: &BTreeMap<&str, SettingInfo<'_>>,
    fields: &[Setting],
    struct_ident: &Ident,
    base_union: &BTreeSet<String>,
) -> TokenStream {
    let init_fields = fields.iter().map(|s| {
        let info = settings.get(s.id.as_str()).expect("settings indexed");
        let ident = info.field_ident();
        let value = if matches!(info.kind, crate::ast::SpecKind::String) {
            quote! { simulation.#ident.clone() }
        } else {
            quote! { simulation.#ident }
        };
        quote! { #ident: #value, }
    });

    let tail = if fields.len() == base_union.len() {
        TokenStream::new()
    } else {
        quote! { ..::std::default::Default::default() }
    };

    quote! {
        impl ::std::convert::From<&#struct_ident>
            for crate::generated::simulations::SimulationBase
        {
            fn from(simulation: &#struct_ident) -> Self {
                Self {
                    #( #init_fields )*
                    #tail
                }
            }
        }
    }
}

fn generate_try_from_base_impl(
    settings: &BTreeMap<&str, SettingInfo<'_>>,
    fields: &[Setting],
    rules: &[NormalizedRule],
    struct_ident: &Ident,
) -> TokenStream {
    let mut optional_fields: BTreeSet<String> = BTreeSet::new();
    for rule in rules {
        for conj in &rule.when {
            for leaf in conj {
                if let Leaf::Present(p) = leaf
                    && !p.present
                {
                    optional_fields.insert(p.r#ref.clone());
                }
            }
        }
    }

    let struct_name = struct_ident.to_string();
    let required_checks =
        generate_required_field_checks(settings, fields, &optional_fields, &struct_name);

    quote! {
        impl ::std::convert::TryFrom<crate::generated::simulations::SimulationBase>
            for #struct_ident
        {
            type Error = ::anyhow::Error;
            fn try_from(
                base: crate::generated::simulations::SimulationBase,
            ) -> ::anyhow::Result<Self> {
                let mut sim = Self::default();
                sim.try_update_from(base)?;
                #required_checks
                Ok(sim)
            }
        }
    }
}

fn generate_required_field_checks(
    settings: &BTreeMap<&str, SettingInfo<'_>>,
    fields: &[Setting],
    optional: &BTreeSet<String>,
    struct_name: &str,
) -> TokenStream {
    let parts = fields.iter().filter_map(|s| {
        let id = s.id.as_str();
        if optional.contains(id) {
            return None;
        }
        let info = settings.get(id).expect("settings indexed");
        let ident = info.field_ident();
        let id_str = id.to_string();
        Some(quote! {
            if sim.#ident.is_none() {
                ::anyhow::bail!(
                    "{}: required setting `{}` is missing",
                    #struct_name,
                    #id_str,
                );
            }
        })
    });
    quote! { #( #parts )* }
}

fn generate_display_impl(
    settings: &BTreeMap<&str, SettingInfo<'_>>,
    fields: &[Setting],
    struct_ident: &Ident,
) -> TokenStream {
    let lines = fields.iter().map(|s| {
        let info = settings.get(s.id.as_str()).expect("settings indexed");
        let ident = info.field_ident();
        let label = info
            .option
            .map_or_else(|| info.id.to_string(), |o| o.spec.name().to_string());
        let escaped = label.replace('{', "{{").replace('}', "}}");
        let fmt = format!("{escaped}: {{value}}");
        quote! {
            if let Some(value) = self.#ident.as_ref() {
                writeln!(f, #fmt)?;
            }
        }
    });
    quote! {
        impl ::std::fmt::Display for #struct_ident {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                #( #lines )*
                Ok(())
            }
        }
    }
}

fn generate_simulation_impl(struct_ident: &Ident, options_path: &TokenStream) -> TokenStream {
    quote! {
        impl crate::features::simulation::Simulation for #struct_ident {
            fn as_any(&self) -> &dyn ::std::any::Any { self }

            fn name(&self) -> Option<#options_path::CustomSettingName> {
                <Self>::name(self)
            }

            fn try_update_from(
                &mut self,
                partial: crate::generated::simulations::SimulationBase,
            ) -> ::anyhow::Result<()> {
                <Self>::try_update_from(self, partial)
            }

            fn to_base(&self) -> crate::generated::simulations::SimulationBase {
                <crate::generated::simulations::SimulationBase as ::std::convert::From<&#struct_ident>>::from(self)
            }
        }
    }
}

fn generate_transaction_profile_impl(
    settings: &BTreeMap<&str, SettingInfo<'_>>,
    struct_ident: &Ident,
    read_order: &[String],
    write_order: &[String],
    presence_conditions: &BTreeMap<String, Dnf>,
) -> anyhow::Result<TokenStream> {
    let staging_accessor = quote! { staged };
    let reads = read_order
        .iter()
        .map(|id| {
            let info = settings
                .get(id.as_str())
                .expect("read order references known setting");
            let ident = info.field_ident();
            let type_path = info.type_path();

            let read_call = quote! {
                Some(<#type_path as crate::ptp::option::SimulationSetting>::try_pull_from(io)?)
            };

            let body = if let Some(dnf) = presence_conditions.get(id) {
                let cond = generate_dnf(settings, dnf, Scopes::new(&staging_accessor))?;
                quote! {
                    if #cond { #read_call } else { None }
                }
            } else {
                read_call
            };

            Ok(quote! {
                staged.#ident = #body;
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;

    let changes = write_order.iter().enumerate().map(|(index, id)| {
        let info = settings
            .get(id.as_str())
            .expect("write order references known setting");
        let ident = info.field_ident();
        let type_path = info.type_path();
        let id = id.as_str();

        quote! {
            if self.#ident.is_some() && self.#ident != original.#ident {
                changes.push(crate::features::simulation::SimulationPropertyChange {
                    index: #index,
                    setting: #id,
                    property_code: <#type_path as crate::ptp::option::SimulationSetting>::prop_code(),
                    restorable: original.#ident.is_some(),
                });
            }
        }
    });

    let writes = write_order.iter().enumerate().map(|(index, id)| {
        let info = settings
            .get(id.as_str())
            .expect("write order references known setting");
        let ident = info.field_ident();
        let type_path = info.type_path();
        let missing = format!("planned simulation setting `{id}` has no value");
        let context = format!("writing simulation setting `{id}`");
        quote! {
            #index => {
                let value = self.#ident.as_ref().ok_or_else(|| {
                    crate::features::simulation::SimulationPropertyWriteError::unconfirmed(
                        ::anyhow::anyhow!(#missing),
                    )
                })?;
                <#type_path as crate::ptp::option::SimulationSetting>::try_push_to(value, io)
                    .map_err(|error| error.context(#context))
            }
        }
    });

    let verifications = write_order.iter().enumerate().map(|(index, id)| {
        let info = settings
            .get(id.as_str())
            .expect("write order references known setting");
        let ident = info.field_ident();
        let type_path = info.type_path();
        let missing = format!("planned simulation setting `{id}` has no value");
        let context = format!("reading back simulation setting `{id}`");
        let mismatch = format!(
            "simulation setting `{id}` readback did not match the requested value"
        );
        quote! {
            #index => {
                let expected = self.#ident.as_ref().ok_or_else(|| {
                    crate::features::simulation::SimulationPropertyWriteError::confirmed(
                        ::anyhow::anyhow!(#missing),
                    )
                })?;
                let observed = <#type_path as crate::ptp::option::SimulationSetting>::try_pull_from(io)
                    .map_err(|error| {
                        crate::features::simulation::SimulationPropertyWriteError::confirmed(error)
                            .context(#context)
                    })?;
                if &observed == expected {
                    Ok(())
                } else {
                    Err(crate::features::simulation::SimulationPropertyWriteError::confirmed(
                        ::anyhow::anyhow!(#mismatch),
                    ))
                }
            }
        }
    });

    Ok(quote! {
        impl crate::features::simulation::SimulationTransactionProfile for #struct_ident {
            fn changes_from(
                &self,
                original: &Self,
            ) -> ::anyhow::Result<Vec<crate::features::simulation::SimulationPropertyChange>> {
                let mut changes = Vec::new();
                #( #changes )*
                Ok(changes)
            }

            fn pull_from<IO: crate::features::simulation::SimulationPropertyIo>(
                io: &mut IO,
            ) -> ::anyhow::Result<Self> {
                let mut staged = Self::default();
                #( #reads )*
                Ok(staged)
            }

            fn push_change<IO: crate::features::simulation::SimulationPropertyIo>(
                &self,
                change: crate::features::simulation::SimulationPropertyChange,
                io: &mut IO,
            ) -> ::std::result::Result<
                (),
                crate::features::simulation::SimulationPropertyWriteError,
            > {
                match change.index {
                    #( #writes )*
                    _ => Err(crate::features::simulation::SimulationPropertyWriteError::unconfirmed(
                        ::anyhow::anyhow!(
                            "unknown simulation property change index {}",
                            change.index,
                        ),
                    )),
                }
            }

            fn verify_change<IO: crate::features::simulation::SimulationPropertyIo>(
                &self,
                change: crate::features::simulation::SimulationPropertyChange,
                io: &mut IO,
            ) -> ::std::result::Result<
                (),
                crate::features::simulation::SimulationPropertyWriteError,
            > {
                match change.index {
                    #( #verifications )*
                    _ => Err(crate::features::simulation::SimulationPropertyWriteError::confirmed(
                        ::anyhow::anyhow!(
                            "unknown simulation property change index {}",
                            change.index,
                        ),
                    )),
                }
            }
        }
    })
}

fn generate_parser_impl(struct_ident: &Ident, camera_struct_path: &TokenStream) -> TokenStream {
    quote! {
        impl crate::features::simulation::CameraSimulationParser for #camera_struct_path {
            fn deserialize_simulation(
                &self,
                data: &[u8],
            ) -> ::anyhow::Result<Box<dyn crate::features::simulation::Simulation>> {
                let decoded: #struct_ident = ::serde_json::from_slice(data)?;
                let base = crate::generated::simulations::SimulationBase::from(&decoded);
                let sim = #struct_ident::try_from(base)?;
                Ok(Box::new(sim))
            }

            fn serialize_simulation(
                &self,
                simulation: &dyn crate::features::simulation::Simulation,
            ) -> ::anyhow::Result<Vec<u8>> {
                Ok(::serde_json::to_vec(simulation)?)
            }
        }
    }
}

fn generate_manager_impl(
    struct_ident: &Ident,
    camera_struct_path: &TokenStream,
    options_path: &TokenStream,
) -> TokenStream {
    let struct_name = struct_ident.to_string();
    quote! {
        impl #struct_ident {
            fn try_pull_selected(
                io: &mut crate::features::simulation::AuthorizedSimulationIo<'_>,
                slot: #options_path::CustomSetting,
            ) -> ::anyhow::Result<Self> {
                let mut io = crate::features::simulation::SelectedSimulationIo::new(io, slot);
                <Self as crate::features::simulation::SimulationTransactionProfile>::pull_from(&mut io)
            }
        }

        impl crate::features::simulation::CameraSimulationManager for #camera_struct_path {
            fn custom_settings_slots(&self) -> Vec<#options_path::CustomSetting> {
                <#options_path::CustomSetting as ::strum::IntoEnumIterator>::iter()
                    .take(#struct_ident::SLOTS as usize)
                    .collect()
            }

            fn get_simulation(
                &self,
                io: &mut crate::features::simulation::AuthorizedSimulationIo<'_>,
                slot: #options_path::CustomSetting,
            ) -> ::anyhow::Result<Box<dyn crate::features::simulation::Simulation>> {
                crate::features::simulation::with_temporary_simulation_selector(
                    io,
                    <#options_path::CustomSetting as crate::ptp::option::SimulationSetting>::prop_code(),
                    |io| Ok(Box::new(#struct_ident::try_pull_selected(io, slot)?)
                        as Box<dyn crate::features::simulation::Simulation>),
                )
            }

            fn get_simulations(
                &self,
                io: &mut crate::features::simulation::AuthorizedSimulationIo<'_>,
                slots: &[#options_path::CustomSetting],
            ) -> ::anyhow::Result<Vec<(
                #options_path::CustomSetting,
                Box<dyn crate::features::simulation::Simulation>,
            )>> {
                crate::features::simulation::with_temporary_simulation_selector(
                    io,
                    <#options_path::CustomSetting as crate::ptp::option::SimulationSetting>::prop_code(),
                    |io| {
                        let mut simulations = Vec::with_capacity(slots.len());
                        for slot in slots.iter().copied() {
                            simulations.push((
                                slot,
                                Box::new(#struct_ident::try_pull_selected(io, slot)?)
                                    as Box<dyn crate::features::simulation::Simulation>,
                            ));
                        }
                        Ok(simulations)
                    },
                )
            }

            fn update_simulation(
                &self,
                io: &mut crate::features::simulation::AuthorizedSimulationIo<'_>,
                slot: #options_path::CustomSetting,
                partial: crate::generated::simulations::SimulationBase,
            ) -> ::std::result::Result<
                crate::features::simulation::SimulationTransactionSuccess,
                crate::features::simulation::SimulationTransactionError,
            > {
                let firmware_profile = io.firmware_capability_profile().map_err(|error| {
                    crate::features::simulation::SimulationTransactionError::preparation(
                        io.is_healthy(),
                        error,
                    )
                })?;
                partial.validate_firmware_capabilities(firmware_profile).map_err(|error| {
                    crate::features::simulation::SimulationTransactionError::preparation(
                        io.is_healthy(),
                        error,
                    )
                })?;
                crate::features::simulation::with_restored_simulation_selector(
                    io,
                    <#options_path::CustomSetting as crate::ptp::option::SimulationSetting>::prop_code(),
                    |io| {
                        let mut io = crate::features::simulation::SelectedSimulationIo::new(io, slot);
                        let original =
                            <#struct_ident as crate::features::simulation::SimulationTransactionProfile>::pull_from(&mut io)
                                .map_err(|error| {
                                    crate::features::simulation::SimulationTransactionError::preparation(
                                        crate::features::simulation::SimulationPropertyIo::is_healthy(&io),
                                        error,
                                    )
                                })?;
                        let mut candidate = original.clone();
                        candidate.try_update_from(partial).map_err(|error| {
                            crate::features::simulation::SimulationTransactionError::preparation(
                                crate::features::simulation::SimulationPropertyIo::is_healthy(&io),
                                error,
                            )
                        })?;
                        crate::generated::simulations::SimulationBase::from(&candidate)
                            .validate_firmware_capabilities(firmware_profile)
                            .map_err(|error| {
                                crate::features::simulation::SimulationTransactionError::preparation(
                                    crate::features::simulation::SimulationPropertyIo::is_healthy(&io),
                                    error,
                                )
                            })?;
                        crate::features::simulation::execute_simulation_transaction(
                            &mut io,
                            &original,
                            &candidate,
                        )
                    },
                )
            }

            fn set_simulation(
                &self,
                io: &mut crate::features::simulation::AuthorizedSimulationIo<'_>,
                slot: #options_path::CustomSetting,
                simulation: &dyn crate::features::simulation::Simulation,
            ) -> ::std::result::Result<
                crate::features::simulation::SimulationTransactionSuccess,
                crate::features::simulation::SimulationTransactionError,
            > {
                let sim = simulation
                    .as_any()
                    .downcast_ref::<#struct_ident>()
                    .ok_or_else(|| ::anyhow::anyhow!(
                        "Simulation type mismatch: expected {}", #struct_name
                    ))
                    .map_err(|error| {
                        crate::features::simulation::SimulationTransactionError::preparation(
                            io.is_healthy(),
                            error,
                        )
                    })?;
                let firmware_profile = io.firmware_capability_profile().map_err(|error| {
                    crate::features::simulation::SimulationTransactionError::preparation(
                        io.is_healthy(),
                        error,
                    )
                })?;
                crate::generated::simulations::SimulationBase::from(sim)
                    .validate_firmware_capabilities(firmware_profile)
                    .map_err(|error| {
                        crate::features::simulation::SimulationTransactionError::preparation(
                            io.is_healthy(),
                            error,
                        )
                    })?;
                crate::features::simulation::with_restored_simulation_selector(
                    io,
                    <#options_path::CustomSetting as crate::ptp::option::SimulationSetting>::prop_code(),
                    |io| {
                        let mut io = crate::features::simulation::SelectedSimulationIo::new(io, slot);
                        let original =
                            <#struct_ident as crate::features::simulation::SimulationTransactionProfile>::pull_from(&mut io)
                                .map_err(|error| {
                                    crate::features::simulation::SimulationTransactionError::preparation(
                                        crate::features::simulation::SimulationPropertyIo::is_healthy(&io),
                                        error,
                                    )
                                })?;
                        crate::features::simulation::execute_simulation_transaction(&mut io, &original, sim)
                    },
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::ast::{Camera, FujiOption};

    use super::generate;

    fn parse<T: serde::de::DeserializeOwned>(json: &str) -> T {
        serde_json::from_str(json).unwrap()
    }

    fn fixture() -> (BTreeMap<String, FujiOption>, BTreeMap<String, Camera>) {
        let options = parse(
            r#"{
                "film_simulation": {
                    "id": "film_simulation",
                    "spec": {
                        "name": "Film Simulation",
                        "kind": "integer",
                        "encoding": { "kind": "raw", "prop_code": 20481 }
                    }
                }
            }"#,
        );
        let cameras = parse(
            r#"{
                "demo": {
                    "id": "demo",
                    "spec": {
                        "name": "Demo",
                        "generation": "demo_generation",
                        "usb": { "vendor_id": 1, "product_id": 2, "chunk_size_ceiling": 1024 },
                        "features": {
                            "simulation": {
                                "slots": 1,
                                "settings": [{ "id": "film_simulation", "ref": "film_simulation" }]
                            }
                        }
                    }
                }
            }"#,
        );
        (options, cameras)
    }

    #[test]
    fn imported_simulation_is_validated_as_a_complete_profile() {
        let (options, cameras) = fixture();
        let generated = generate(&options, &cameras).unwrap().to_string();

        assert!(
            generated.contains("deny_unknown_fields"),
            "per-camera JSON must reject fields not supported by that camera:\n{generated}",
        );
        assert!(
            generated.contains("let decoded : DemoSimulation = :: serde_json :: from_slice"),
            "parser must decode the strict per-camera JSON shape:\n{generated}",
        );
        assert!(
            generated.contains("let sim = DemoSimulation :: try_from (base)"),
            "parser must run required-field validation:\n{generated}",
        );
    }

    #[test]
    fn generated_setting_writes_include_the_setting_name() {
        let (options, cameras) = fixture();
        let generated = generate(&options, &cameras).unwrap().to_string();

        assert!(
            generated.contains("writing simulation setting `film_simulation`"),
            "write failures need actionable setting context:\n{generated}",
        );
    }

    #[test]
    fn generated_setting_writes_have_immediate_semantic_readback() {
        let (options, cameras) = fixture();
        let generated = generate(&options, &cameras).unwrap().to_string();

        assert!(
            generated.contains("fn verify_change")
                && generated.contains("reading back simulation setting `film_simulation`")
                && generated.contains(
                    "simulation setting `film_simulation` readback did not match the requested value"
                ),
            "every accepted setting write must be read back and compared before continuing:\n{generated}",
        );
    }

    #[test]
    fn generated_writes_validate_firmware_values_before_selecting_slot() {
        let (options, cameras) = fixture();
        let generated = generate(&options, &cameras).unwrap().to_string();
        let update = generated
            .find("fn update_simulation")
            .expect("generated manager must implement simulation updates");
        let update_path = &generated[update..];
        let validation = update_path
            .find("validate_firmware_capabilities")
            .expect("generated write path must validate firmware values");
        let slot_write = update_path
            .find("SelectedSimulationIo :: new (io , slot)")
            .expect("generated write path must prepare selected-slot I/O");

        assert!(
            validation < slot_write,
            "unsupported values must fail before the first selector mutation: {generated}"
        );
    }

    #[test]
    fn generated_writes_restore_the_previously_selected_slot() {
        let (options, cameras) = fixture();
        let generated = generate(&options, &cameras).unwrap().to_string();

        for method in ["fn update_simulation", "fn set_simulation"] {
            let start = generated
                .find(method)
                .unwrap_or_else(|| panic!("generated manager must implement {method}"));
            let body = &generated[start..];
            let restore = body
                .find("with_restored_simulation_selector")
                .unwrap_or_else(|| panic!("{method} must restore the slot selector: {body}"));
            let slot_write = body
                .find("SelectedSimulationIo :: new (io , slot)")
                .expect("write path must prepare selected-slot I/O");

            assert!(
                restore < slot_write,
                "{method} must snapshot the selector before selecting the target slot: {body}"
            );
        }
    }

    #[test]
    fn generated_manager_delegates_journaled_transactions_to_runtime() {
        let (options, cameras) = fixture();
        let generated = generate(&options, &cameras).unwrap().to_string();

        assert!(
            generated.contains("SimulationTransactionProfile for DemoSimulation"),
            "generated cameras must expose their ordered property adapter:\n{generated}",
        );
        assert!(
            generated.contains("SimulationSetting > :: try_pull_from (io)")
                && generated.contains("SimulationSetting > :: try_push_to (value , io)"),
            "transactional property I/O must preserve firmware-aware setting codecs:\n{generated}",
        );
        assert!(
            generated.contains("SelectedSimulationIo :: new (io , slot)")
                && generated.contains(
                    "execute_simulation_transaction (& mut io , & original , & candidate"
                ),
            "runtime transaction executor must own apply, journal, rollback, and readback:\n{generated}",
        );
        assert!(
            !generated.contains("finish_failed_simulation_apply")
                && !generated.contains("fn try_push_with_rollback"),
            "legacy full-profile rollback bypass must not be generated:\n{generated}",
        );
    }

    #[test]
    fn generated_reads_restore_the_temporary_selector_after_single_and_batch_access() {
        let (options, cameras) = fixture();
        let generated = generate(&options, &cameras).unwrap().to_string();

        assert_eq!(
            generated
                .matches("with_temporary_simulation_selector")
                .count(),
            2,
            "single and batch reads must each have one outer selector transaction:\n{generated}",
        );
        assert!(
            generated.contains("for slot in slots . iter () . copied ()"),
            "batch access must read every slot inside one outer transaction:\n{generated}",
        );
    }

    #[test]
    fn generated_update_writes_only_changed_properties() {
        let (options, cameras) = fixture();
        let generated = generate(&options, &cameras).unwrap().to_string();

        assert!(
            generated.contains(
                "self . film_simulation . is_some () && self . film_simulation != original . film_simulation"
            ) && generated.contains("setting : \"film_simulation\"")
                && generated.contains("execute_simulation_transaction (& mut io , & original , & candidate"),
            "the generated adapter must diff the full candidate and preserve the schema property order:\n{generated}",
        );
    }
}
