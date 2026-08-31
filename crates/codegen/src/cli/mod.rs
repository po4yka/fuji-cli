pub mod render;
pub mod simulation;

use std::collections::BTreeMap;

use anyhow::Context;
use proc_macro2::TokenStream;
use quote::quote;

use crate::ast::{Camera, FujiOption, SpecKind};

fn argument_attrs(kind: SpecKind) -> TokenStream {
    match kind {
        SpecKind::Integer | SpecKind::Float => {
            quote! { #[clap(long, allow_negative_numbers(true))] }
        }
        SpecKind::String | SpecKind::Enum => quote! { #[clap(long)] },
    }
}

pub fn generate(
    options: &BTreeMap<String, FujiOption>,
    cameras: &BTreeMap<String, Camera>,
) -> anyhow::Result<TokenStream> {
    let simulation = simulation::generate(options, cameras).context("generating SimulationArgs")?;
    let render = render::generate(options, cameras).context("generating RenderArgs")?;

    Ok(quote! {
        //! Generated CLI types. Do not edit.

        #simulation
        #render
    })
}
