pub mod render;
pub mod simulation;

use std::{cmp::Ordering, collections::BTreeMap};

use anyhow::Context;
use proc_macro2::TokenStream;
use quote::quote;

use crate::ast::{
    Camera, EnumRules, FujiOption, LookupSpec, NumericEncoding, NumericRules, OptionSpec, SpecKind,
    StringRules,
};

fn argument_attrs(kind: SpecKind, help: &str, long_help: &str) -> TokenStream {
    match kind {
        SpecKind::Integer | SpecKind::Float => {
            quote! {
                #[clap(
                    long,
                    help = #help,
                    long_help = #long_help,
                    allow_negative_numbers(true)
                )]
            }
        }
        SpecKind::String | SpecKind::Enum => {
            quote! { #[clap(long, help = #help, long_help = #long_help)] }
        }
    }
}

fn option_long_help(spec: &OptionSpec) -> String {
    match spec {
        OptionSpec::Integer {
            name,
            encoding: NumericEncoding::Lookup { spec, .. },
            ..
        } => numeric_lookup_long_help(name, spec, compare_integer_strings),
        OptionSpec::Float {
            name,
            encoding: NumericEncoding::Lookup { spec, .. },
            ..
        } => numeric_lookup_long_help(name, spec, compare_float_strings),
        OptionSpec::Integer { name, rules, .. } => numeric_long_help(name, rules.as_ref()),
        OptionSpec::Float { name, rules, .. } => numeric_long_help(name, rules.as_ref()),
        OptionSpec::String { name, rules, .. } => string_long_help(name, rules.as_ref()),
        OptionSpec::Enum { name, rules, .. } => enum_long_help(name, rules),
    }
}

fn numeric_lookup_long_help(
    name: &str,
    spec: &LookupSpec,
    compare: fn(&str, &str) -> Ordering,
) -> String {
    let mut values = spec.values.keys().map(String::as_str).collect::<Vec<_>>();
    values.sort_by(|left, right| compare(left, right));
    format!(
        "{name}\n\nSchema values: {}. Camera firmware may further restrict values.",
        values.join(", ")
    )
}

fn compare_integer_strings(left: &str, right: &str) -> Ordering {
    match (left.parse::<i64>(), right.parse::<i64>()) {
        (Ok(left), Ok(right)) => left.cmp(&right),
        _ => left.cmp(right),
    }
}

fn compare_float_strings(left: &str, right: &str) -> Ordering {
    match (left.parse::<f64>(), right.parse::<f64>()) {
        (Ok(left), Ok(right)) => left.total_cmp(&right),
        _ => left.cmp(right),
    }
}

fn enum_long_help(name: &str, rules: &EnumRules) -> String {
    if rules.variants.is_empty() {
        return name.to_owned();
    }
    let values = rules
        .variants
        .iter()
        .map(|variant| variant.id.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    format!("{name}\n\nSchema values: {values}. Camera firmware may further restrict values.")
}

fn string_long_help(name: &str, rules: Option<&StringRules>) -> String {
    let Some(rules) = rules else {
        return name.to_owned();
    };

    let constraint = match (rules.min_length, rules.max_length) {
        (Some(min), Some(max)) => format!("Length: {min}..={max} characters."),
        (Some(min), None) => format!("Minimum length: {min} characters."),
        (None, Some(max)) => format!("Maximum length: {max} characters."),
        (None, None) => return name.to_owned(),
    };
    format!("{name}\n\n{constraint}")
}

fn numeric_long_help<T: std::fmt::Display>(name: &str, rules: Option<&NumericRules<T>>) -> String {
    let Some(rules) = rules else {
        return name.to_owned();
    };

    let constraint = match (&rules.min, &rules.max) {
        (Some(min), Some(max)) => format!("Schema range: {min}..={max}."),
        (Some(min), None) => format!("Schema minimum: {min}."),
        (None, Some(max)) => format!("Schema maximum: {max}."),
        (None, None) => String::new(),
    };
    let step = rules
        .step
        .as_ref()
        .map_or_else(String::new, |step| format!(" Step: {step}."));
    if constraint.is_empty() && step.is_empty() {
        name.to_owned()
    } else {
        format!("{name}\n\n{constraint}{step} Camera firmware may further restrict values.")
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
