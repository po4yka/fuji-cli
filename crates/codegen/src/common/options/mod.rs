pub(crate) mod common;
mod r#enum;
mod float;
mod integer;
mod string;

use std::collections::BTreeMap;

use anyhow::Context;
use proc_macro2::{Literal, TokenStream};
use quote::{format_ident, quote};

use crate::ast::{FujiOption, NumericEncoding, OptionSpec};

pub fn generate(options: &BTreeMap<String, FujiOption>) -> anyhow::Result<TokenStream> {
    let mut blocks = Vec::with_capacity(options.len());

    for (id, opt) in options {
        let block = match &opt.spec {
            OptionSpec::Enum {
                rules, encoding, ..
            } => r#enum::generate(id, rules, encoding)
                .with_context(|| format!("generating enum option `{id}`"))?,
            OptionSpec::Integer {
                rules, encoding, ..
            } => match encoding {
                NumericEncoding::Lookup {
                    spec, prop_code, ..
                } => integer::lookup::generate(id, *prop_code, spec)
                    .with_context(|| format!("generating integer lookup option `{id}`"))?,
                NumericEncoding::Raw { prop_code, .. }
                | NumericEncoding::Scale { prop_code, .. } => {
                    integer::scaled::generate(id, *prop_code, rules.as_ref(), encoding)
                        .with_context(|| format!("generating integer option `{id}`"))?
                }
            },
            OptionSpec::Float {
                rules, encoding, ..
            } => match encoding {
                NumericEncoding::Lookup {
                    spec, prop_code, ..
                } => float::lookup::generate(id, *prop_code, spec)
                    .with_context(|| format!("generating float lookup option `{id}`"))?,
                NumericEncoding::Raw { prop_code, .. }
                | NumericEncoding::Scale { prop_code, .. } => {
                    float::scaled::generate(id, *prop_code, rules.as_ref(), encoding)
                        .with_context(|| format!("generating float option `{id}`"))?
                }
            },
            OptionSpec::String {
                rules, encoding, ..
            } => string::generate(id, rules.as_ref(), encoding)
                .with_context(|| format!("generating string option `{id}`"))?,
        };
        blocks.push(block);
    }

    let prop_codes = generate_prop_codes(options);

    let tokens = quote! {
        //! Generated option types. Do not edit.

        #(#blocks)*

        #prop_codes
    };

    Ok(tokens)
}

/// Emits one constant per option that is a PTP device property, so runtime
/// code reads a property through the constant its option owns instead of
/// repeating the code as a literal. `fml/option.cue` stays the only place a
/// property code is written down.
fn generate_prop_codes(options: &BTreeMap<String, FujiOption>) -> TokenStream {
    let consts = options.iter().filter_map(|(id, option)| {
        let code = option.spec.prop_code()?;
        let ident = format_ident!("{}", id.to_uppercase());
        let literal = Literal::u16_suffixed(code);
        let doc = format!("{} (PTP device property 0x{code:04X}).", option.spec.name());
        Some(quote! {
            #[doc = #doc]
            pub const #ident: u16 = #literal;
        })
    });

    quote! {
        /// PTP device property codes declared by `fml/option.cue`.
        pub mod prop_codes {
            #(#consts)*
        }
    }
}

pub fn path() -> TokenStream {
    quote! { crate::generated::options }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::ast::FujiOption;

    use super::generate_prop_codes;

    #[test]
    fn every_option_that_is_a_ptp_property_gets_a_named_constant() {
        let options: BTreeMap<String, FujiOption> = serde_json::from_str(
            r#"{
                "usb_mode": {
                    "id": "usb_mode",
                    "spec": {
                        "name": "USB Mode",
                        "kind": "integer",
                        "encoding": { "kind": "raw", "prop_code": 53614, "data_type": 4 }
                    }
                },
                "grain_effect": {
                    "id": "grain_effect",
                    "spec": {
                        "name": "Grain Effect",
                        "kind": "integer",
                        "encoding": { "kind": "raw" }
                    }
                }
            }"#,
        )
        .unwrap();

        let generated = generate_prop_codes(&options).to_string();

        assert!(
            generated.contains("pub const USB_MODE : u16 = 53614u16"),
            "{generated}"
        );
        assert!(
            !generated.contains("GRAIN_EFFECT"),
            "an option that is not a PTP property has no code to name: {generated}"
        );
    }
}
