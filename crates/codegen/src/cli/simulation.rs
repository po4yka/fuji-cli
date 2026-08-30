use std::collections::{BTreeMap, BTreeSet};

use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use crate::{
    ast::{Camera, FujiOption},
    common::{options, simulations},
    util::ident::safe_upper_camel_case_ident,
};

struct Entry {
    id: String,
    ident: proc_macro2::Ident,
    type_path: TokenStream,
}

pub fn generate(
    options: &BTreeMap<String, FujiOption>,
    cameras: &BTreeMap<String, Camera>,
) -> anyhow::Result<TokenStream> {
    let simulation_options = collect_simulation_option_ids(cameras);
    let entries = build_entries(options, &simulation_options);

    let struct_def = generate_struct(&entries);
    let from_impl = generate_from_impl(&entries, &simulation_options);
    let prop_codes_const = generate_prop_codes_const(options, &simulation_options);

    Ok(quote! {
        #struct_def
        #from_impl
        #prop_codes_const
    })
}

fn collect_simulation_option_ids(cameras: &BTreeMap<String, Camera>) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for camera in cameras.values() {
        let Some(simulation) = camera
            .spec
            .features
            .as_ref()
            .and_then(|f| f.simulation.as_ref())
        else {
            continue;
        };
        for setting in &simulation.settings {
            out.insert(setting.r#ref.clone());
        }
    }
    out
}

fn build_entries(
    options: &BTreeMap<String, FujiOption>,
    simulation_options: &BTreeSet<String>,
) -> Vec<Entry> {
    options
        .values()
        .filter(|opt| !opt.codegen.skip_args)
        .filter(|opt| simulation_options.contains(&opt.id))
        .map(|opt| {
            let ident = format_ident!("{}", opt.id);
            let type_ident = safe_upper_camel_case_ident(&opt.id);
            let options_path = options::path();
            let type_path = quote! { #options_path::#type_ident };
            Entry {
                id: opt.id.clone(),
                ident,
                type_path,
            }
        })
        .collect()
}

fn generate_struct(entries: &[Entry]) -> TokenStream {
    let fields = entries.iter().map(|entry| {
        let ident = &entry.ident;
        let ty = &entry.type_path;

        let attrs = quote! { #[clap(long, allow_hyphen_values(true))] };

        quote! {
            #attrs
            pub #ident: Option<#ty>,
        }
    });

    quote! {
        #[derive(::clap::Args, Debug, Default, Clone)]
        pub struct SimulationArgs {
            #( #fields )*
        }
    }
}

fn generate_from_impl(entries: &[Entry], simulation_options: &BTreeSet<String>) -> TokenStream {
    let simulations_path = simulations::path();
    let mut fields: Vec<TokenStream> = Vec::new();
    let mut covered = 0usize;

    for entry in entries {
        if !simulation_options.contains(&entry.id) {
            continue;
        }
        let ident = &entry.ident;
        fields.push(quote! { #ident: args.#ident, });
        covered += 1;
    }

    let tail = if covered == simulation_options.len() {
        quote! {}
    } else {
        quote! { ..::std::default::Default::default() }
    };

    quote! {
        impl ::std::convert::From<SimulationArgs> for #simulations_path::SimulationBase {
            fn from(args: SimulationArgs) -> Self {
                Self {
                    #( #fields )*
                    #tail
                }
            }
        }
    }
}

fn generate_prop_codes_const(
    options: &BTreeMap<String, FujiOption>,
    simulation_options: &BTreeSet<String>,
) -> TokenStream {
    let prop_codes = collect_simulation_prop_codes(options, simulation_options);
    let prop_code_lits = prop_codes
        .iter()
        .map(|c| proc_macro2::Literal::u16_suffixed(*c));

    quote! {
        pub const SIMULATION_PROP_CODES: &[u16] = &[
            #( #prop_code_lits ),*
        ];
    }
}

fn collect_simulation_prop_codes(
    options: &BTreeMap<String, FujiOption>,
    simulation_options: &BTreeSet<String>,
) -> Vec<u16> {
    let mut codes: BTreeSet<u16> = BTreeSet::new();
    for id in simulation_options {
        let Some(option) = options.get(id) else {
            continue;
        };
        if let Some(code) = option.spec.prop_code() {
            codes.insert(code);
        }
    }
    codes.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::ast::{Camera, FujiOption};

    use super::generate;

    fn parse<T: serde::de::DeserializeOwned>(json: &str) -> T {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn simulation_args_exclude_render_only_options() {
        let options: BTreeMap<String, FujiOption> = parse(
            r#"{
                "film_simulation": {
                    "id": "film_simulation",
                    "spec": {
                        "name": "Film Simulation",
                        "kind": "integer",
                        "encoding": { "kind": "raw" }
                    }
                },
                "file_type": {
                    "id": "file_type",
                    "spec": {
                        "name": "File Type",
                        "kind": "integer",
                        "encoding": { "kind": "raw" }
                    }
                },
                "exposure_offset": {
                    "id": "exposure_offset",
                    "spec": {
                        "name": "Exposure Offset",
                        "kind": "integer",
                        "encoding": { "kind": "raw" }
                    }
                },
                "teleconverter": {
                    "id": "teleconverter",
                    "spec": {
                        "name": "Teleconverter",
                        "kind": "integer",
                        "encoding": { "kind": "raw" }
                    }
                }
            }"#,
        );
        let cameras: BTreeMap<String, Camera> = parse(
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

        let generated = generate(&options, &cameras).unwrap().to_string();

        assert!(generated.contains("pub film_simulation"), "{generated}");
        for render_only in ["file_type", "exposure_offset", "teleconverter"] {
            assert!(
                !generated.contains(&format!("pub {render_only}")),
                "render-only option `{render_only}` leaked into SimulationArgs:\n{generated}",
            );
        }
    }
}
