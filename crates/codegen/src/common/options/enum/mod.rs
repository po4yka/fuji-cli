use std::collections::BTreeMap;

use anyhow::{Context, bail, ensure};
use proc_macro2::{Ident, TokenStream};
use quote::{format_ident, quote};

use super::common::{
    generate_try_from_wire_impl, resolve_enum_repr_signed, resolve_repr_type, resolve_repr_type_32,
    wire_literal,
};
use crate::{
    ast::{EnumEncoding, EnumRules, EnumVariant, LookupSpec, LookupValue},
    util::ident::safe_upper_camel_case_ident,
};

struct Resolved<'a> {
    variant: &'a EnumVariant,
    canonical: i32,
    alternates: Vec<i32>,
}

impl<'a> Resolved<'a> {
    fn from_variant(variant: &'a EnumVariant, spec: &LookupSpec) -> anyhow::Result<Self> {
        let lookup_value = spec
            .values
            .get(&variant.id)
            .with_context(|| format!("missing lookup entry for variant `{}`", variant.id))?;

        let (canonical, alternates) = match lookup_value {
            LookupValue::Single(n) => (*n, Vec::new()),
            LookupValue::Multi(list) => {
                let (&canonical, rest) = list.split_first().with_context(|| {
                    format!("empty multi-value lookup for variant `{}`", variant.id)
                })?;

                (canonical, rest.to_vec())
            }
        };

        Ok(Self {
            variant,
            canonical,
            alternates,
        })
    }
}

pub(crate) fn generate(
    id: &str,
    rules: &EnumRules,
    encoding: &EnumEncoding,
) -> anyhow::Result<TokenStream> {
    let EnumEncoding::Lookup {
        spec, prop_code, ..
    } = encoding;

    let resolved = rules
        .variants
        .iter()
        .map(|v| Resolved::from_variant(v, spec))
        .collect::<anyhow::Result<Vec<_>>>()?;

    let wire_values: Vec<_> = resolved
        .iter()
        .flat_map(|r| std::iter::once(&r.canonical).chain(&r.alternates))
        .copied()
        .collect();

    let signed = resolve_enum_repr_signed(&wire_values)
        .with_context(|| format!("determining representation type for enum option `{id}`"))?;
    let repr_type = resolve_repr_type(signed);
    let repr_type_32 = resolve_repr_type_32(signed);

    let enum_def = generate_enum_def(
        &safe_upper_camel_case_ident(id),
        &repr_type,
        signed,
        &resolved,
    )
    .with_context(|| format!("generating enum definition for enum option `{id}`"))?;
    let try_from_wire_impl = generate_try_from_wire_impl(
        &safe_upper_camel_case_ident(id),
        signed,
        &repr_type,
        &wire_items(&resolved),
    )
    .with_context(|| format!("generating try_from_wire impl for enum option `{id}`"))?;
    let display_impl = generate_display_impl(&safe_upper_camel_case_ident(id), &resolved)
        .with_context(|| format!("generating Display impl for enum option `{id}`"))?;
    let from_str_impl = generate_from_str_impl(&safe_upper_camel_case_ident(id), &resolved)
        .with_context(|| format!("generating FromStr impl for enum option `{id}`"))?;
    let capability_value_impl =
        generate_capability_value_impl(&safe_upper_camel_case_ident(id), &resolved);
    let round_trip_test = generate_round_trip_test(&safe_upper_camel_case_ident(id));

    let (ptp_serde_impl, simulation_setting_impl) = if let Some(prop_code) = prop_code {
        let serde = generate_ptp_serde_impl(&safe_upper_camel_case_ident(id), &repr_type)
            .with_context(|| format!("generating binrw codec impls for enum option `{id}`"))?;
        let setting = generate_simulation_setting_impl(
            id,
            &safe_upper_camel_case_ident(id),
            &repr_type,
            *prop_code,
        )
        .with_context(|| format!("generating SimulationSetting impl for enum option `{id}`"))?;
        (serde, setting)
    } else {
        (quote! {}, quote! {})
    };

    let conversion_profile_impl = generate_conversion_profile_impl(
        &safe_upper_camel_case_ident(id),
        &repr_type,
        &repr_type_32,
    )
    .with_context(|| format!("generating ConversionProfileField impl for enum option `{id}`"))?;

    Ok(quote! {
        #enum_def
        #try_from_wire_impl
        #display_impl
        #from_str_impl
        #capability_value_impl
        #ptp_serde_impl
        #simulation_setting_impl
        #conversion_profile_impl
        #round_trip_test
    })
}

/// A test per enum proving that `Display` output parses back to the same
/// variant through the runtime's real input normalization. This is what keeps
/// `clean_input_key` and `CleanAlphanumeric::clean` honest about each other.
fn generate_round_trip_test(type_name: &Ident) -> TokenStream {
    let module = format_ident!("{}_round_trip_tests", type_name.to_string().to_lowercase());
    quote! {
        #[cfg(test)]
        mod #module {
            #[test]
            fn display_output_parses_back_to_the_same_variant() {
                for variant in <super::#type_name as ::strum::IntoEnumIterator>::iter() {
                    let text = variant.to_string();
                    let parsed: super::#type_name = text
                        .parse()
                        .expect("Display output of a generated enum must parse back");
                    assert_eq!(parsed, variant, "{text} parsed to a different variant");
                }
            }
        }
    }
}

fn wire_items(resolved: &[Resolved<'_>]) -> Vec<(Ident, Vec<i32>)> {
    resolved
        .iter()
        .map(|r| {
            let mut wires = Vec::with_capacity(1 + r.alternates.len());
            wires.push(r.canonical);
            wires.extend(r.alternates.iter().copied());
            (safe_upper_camel_case_ident(&r.variant.id), wires)
        })
        .collect()
}

fn generate_enum_def(
    type_name: &Ident,
    repr_type: &Ident,
    signed: bool,
    resolved: &[Resolved<'_>],
) -> anyhow::Result<TokenStream> {
    let defs = resolved
        .iter()
        .map(|r| {
            let v = safe_upper_camel_case_ident(&r.variant.id);
            let canonical = wire_literal(r.canonical, signed)?;
            Ok(quote! { #v = #canonical, })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;

    Ok(quote! {
        #[repr(#repr_type)]
        #[derive(
            Debug,
            Clone,
            Copy,
            PartialEq,
            Eq,
            strum_macros::EnumIter,
            serde_with::SerializeDisplay,
            serde_with::DeserializeFromStr,
        )]
        pub enum #type_name {
            #(#defs)*
        }
    })
}

fn generate_display_impl(
    type_name: &Ident,
    resolved: &[Resolved<'_>],
) -> anyhow::Result<TokenStream> {
    let arms = resolved
        .iter()
        .map(|r| {
            let v = safe_upper_camel_case_ident(&r.variant.id);
            let display = &r.variant.name;
            quote! { Self::#v => write!(f, #display), }
        })
        .collect::<Vec<_>>();

    Ok(quote! {
        impl ::std::fmt::Display for #type_name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                match self {
                    #(#arms)*
                }
            }
        }
    })
}

/// Mirrors `crate::input::CleanAlphanumeric::clean` in the runtime. The parse
/// keys must be built with the same rule the runtime applies to user input,
/// or a key can never match. The generated round-trip test catches drift.
fn clean_input_key(value: &str) -> String {
    value
        .trim()
        .to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+'))
        .collect()
}

/// The normalized inputs each variant accepts: its id, its display name (so
/// `Display` output always parses back), and its aliases. Two variants of one
/// option may never accept the same normalized input; that would make the
/// first match arm win silently, as `HDR800+` once did against `HDR800`.
fn parse_keys<'a>(
    variants: impl IntoIterator<Item = &'a EnumVariant>,
) -> anyhow::Result<Vec<Vec<String>>> {
    let mut owners: BTreeMap<String, &str> = BTreeMap::new();
    variants
        .into_iter()
        .map(|variant| {
            let mut keys: Vec<String> = Vec::new();
            for raw in [&variant.id, &variant.name]
                .into_iter()
                .chain(&variant.aliases)
            {
                let key = clean_input_key(raw);
                ensure!(
                    !key.is_empty(),
                    "enum variant `{}` accepts `{raw}`, which normalizes to nothing",
                    variant.id
                );
                if keys.contains(&key) {
                    continue;
                }
                if let Some(owner) = owners.insert(key.clone(), &variant.id)
                    && owner != variant.id
                {
                    bail!(
                        "enum variants `{owner}` and `{}` both accept the normalized input `{key}`",
                        variant.id
                    );
                }
                keys.push(key);
            }
            Ok(keys)
        })
        .collect()
}

fn generate_from_str_impl(
    type_name: &Ident,
    resolved: &[Resolved<'_>],
) -> anyhow::Result<TokenStream> {
    let keys = parse_keys(resolved.iter().map(|r| r.variant))?;
    let arms = resolved
        .iter()
        .zip(&keys)
        .map(|(r, keys)| {
            let v = safe_upper_camel_case_ident(&r.variant.id);
            quote! {
                #(#keys)|* => return Ok(Self::#v),
            }
        })
        .collect::<Vec<_>>();

    Ok(quote! {
        impl ::std::str::FromStr for #type_name {
            type Err = ::anyhow::Error;
            fn from_str(s: &str) -> ::anyhow::Result<Self> {
                match crate::input::CleanAlphanumeric::clean(&s).as_str() {
                    #(#arms)*
                    _ => {}
                }
                if let Some(best) = <Self as crate::input::Choices>::closest(s) {
                    ::anyhow::bail!(
                        "Unknown {} '{s}'. Did you mean '{best}'?",
                        stringify!(#type_name),
                    );
                }
                ::anyhow::bail!("Unknown {} '{s}'", stringify!(#type_name));
            }
        }
    })
}

fn generate_capability_value_impl(type_name: &Ident, resolved: &[Resolved<'_>]) -> TokenStream {
    let value_arms = resolved.iter().map(|resolved| {
        let variant = safe_upper_camel_case_ident(&resolved.variant.id);
        let id = &resolved.variant.id;
        quote! { Self::#variant => #id, }
    });
    let wire_arms = resolved.iter().map(|resolved| {
        let variant = safe_upper_camel_case_ident(&resolved.variant.id);
        let canonical = resolved.canonical;
        quote! { Self::#variant => #canonical, }
    });
    let parse_arms = resolved.iter().map(|resolved| {
        let variant = safe_upper_camel_case_ident(&resolved.variant.id);
        let id = &resolved.variant.id;
        quote! { #id => Ok(Self::#variant), }
    });

    let parser = quote! {
        pub(crate) fn try_from_capability_value_id(value: &str) -> ::anyhow::Result<Self> {
            match value {
                #(#parse_arms)*
                _ => ::anyhow::bail!(
                    "unknown firmware capability value `{value}` for {}",
                    stringify!(#type_name),
                ),
            }
        }
    };

    quote! {
        impl #type_name {
            pub(crate) const fn capability_value_id(&self) -> &'static str {
                match self {
                    #(#value_arms)*
                }
            }

            #[allow(
                dead_code,
                reason = "generated enum codecs share one capability surface across camera features"
            )]
            pub(crate) const fn capability_global_wire_value(&self) -> i32 {
                match self {
                    #(#wire_arms)*
                }
            }

            #parser
        }
    }
}

fn generate_ptp_serde_impl(type_name: &Ident, repr_type: &Ident) -> anyhow::Result<TokenStream> {
    Ok(quote! {
        impl ::binrw::BinWrite for #type_name {
            type Args<'a> = ();

            fn write_options<W: ::std::io::Write + ::std::io::Seek>(
                &self,
                writer: &mut W,
                endian: ::binrw::Endian,
                (): Self::Args<'_>,
            ) -> ::binrw::BinResult<()> {
                let raw: #repr_type = *self as #repr_type;
                <#repr_type as ::binrw::BinWrite>::write_options(&raw, writer, endian, ())
            }
        }

        impl ::binrw::BinRead for #type_name {
            type Args<'a> = ();

            fn read_options<R: ::std::io::Read + ::std::io::Seek>(
                reader: &mut R,
                endian: ::binrw::Endian,
                (): Self::Args<'_>,
            ) -> ::binrw::BinResult<Self> {
                let position = ::std::io::Seek::stream_position(reader)?;
                let raw = <#repr_type as ::binrw::BinRead>::read_options(reader, endian, ())?;
                Self::try_from_wire(raw).map_err(|error| ::binrw::Error::Custom {
                    pos: position,
                    err: Box::new(error),
                })
            }
        }
    })
}

fn generate_simulation_setting_impl(
    id: &str,
    type_name: &Ident,
    repr_type: &Ident,
    prop_code: u16,
) -> anyhow::Result<TokenStream> {
    Ok(quote! {
        impl crate::ptp::option::SimulationSetting for #type_name {
            fn prop_code() -> u16 { #prop_code }

            fn try_push_to<IO: crate::features::simulation::SimulationPropertyIo>(
                &self,
                io: &mut IO,
            ) -> ::std::result::Result<
                (),
                crate::features::simulation::SimulationPropertyWriteError,
            > {
                let wire = io
                    .firmware_option_write_value(#id, self.capability_value_id())
                    .map_err(
                        crate::features::simulation::SimulationPropertyWriteError::unconfirmed,
                    )?;
                let raw: #repr_type = wire.try_into().map_err(|_| {
                    crate::features::simulation::SimulationPropertyWriteError::unconfirmed(
                        ::anyhow::anyhow!(
                            "firmware wire value {wire} for {} does not fit {}",
                            stringify!(#type_name),
                            stringify!(#repr_type),
                        ),
                    )
                })?;
                io.set_prop(Self::prop_code(), &raw)
            }

            fn try_pull_from<IO: crate::features::simulation::SimulationPropertyIo>(
                io: &mut IO,
            ) -> ::anyhow::Result<Self> {
                let raw: #repr_type = io.get_prop(Self::prop_code())?;
                let logical = io.firmware_option_read_logical_value(#id, i32::from(raw))?;
                Self::try_from_capability_value_id(logical)
            }
        }
    })
}

fn generate_conversion_profile_impl(
    type_name: &Ident,
    repr_type: &Ident,
    repr_type_32: &Ident,
) -> anyhow::Result<TokenStream> {
    Ok(quote! {
        impl crate::ptp::option::ConversionProfileField for #type_name {
            fn write_conversion_profile_field<W: ::std::io::Write + ::std::io::Seek>(
                &self,
                writer: &mut W,
                endian: ::binrw::Endian,
            ) -> ::binrw::BinResult<()> {
                let raw: #repr_type = *self as #repr_type;
                let extended = #repr_type_32::from(raw);
                <#repr_type_32 as ::binrw::BinWrite>::write_options(
                    &extended, writer, endian, (),
                )
            }

            fn read_conversion_profile_field<R: ::std::io::Read + ::std::io::Seek>(
                reader: &mut R,
                endian: ::binrw::Endian,
            ) -> ::binrw::BinResult<Self> {
                let position = ::std::io::Seek::stream_position(reader)?;
                let extended = <#repr_type_32 as ::binrw::BinRead>::read_options(
                    reader, endian, (),
                )?;
                let raw: #repr_type = extended.try_into().map_err(|_| {
                    ::binrw::Error::Custom {
                        pos: position,
                        err: Box::new(::std::io::Error::new(
                            ::std::io::ErrorKind::InvalidData,
                            format!(
                                "{} value {} doesn't fit in {}",
                                stringify!(#type_name),
                                extended,
                                stringify!(#repr_type),
                            ),
                        )),
                    }
                })?;
                Self::try_from_wire(raw).map_err(|error| ::binrw::Error::Custom {
                    pos: position,
                    err: Box::new(error),
                })
            }

            fn write_conversion_profile_field_for_firmware<
                W: ::std::io::Write + ::std::io::Seek,
            >(
                &self,
                writer: &mut W,
                endian: ::binrw::Endian,
                profile: &crate::generated::cameras::CameraFirmwareCapabilityProfile,
                option: &'static str,
            ) -> ::binrw::BinResult<()> {
                let position = ::std::io::Seek::stream_position(writer)?;
                let wire = profile
                    .write_wire_value(option, self.capability_value_id())
                    .map_err(|error| ::binrw::Error::Custom {
                        pos: position,
                        err: Box::new(error),
                    })?;
                <i32 as ::binrw::BinWrite>::write_options(&wire, writer, endian, ())
            }

            fn read_conversion_profile_field_for_firmware<
                R: ::std::io::Read + ::std::io::Seek,
            >(
                reader: &mut R,
                endian: ::binrw::Endian,
                profile: &crate::generated::cameras::CameraFirmwareCapabilityProfile,
                option: &'static str,
            ) -> ::binrw::BinResult<Self> {
                let position = ::std::io::Seek::stream_position(reader)?;
                let wire = <i32 as ::binrw::BinRead>::read_options(reader, endian, ())?;
                let logical = profile
                    .read_logical_value(option, wire)
                    .map_err(|error| ::binrw::Error::Custom {
                        pos: position,
                        err: Box::new(error),
                    })?;
                Self::try_from_capability_value_id(logical).map_err(|error| {
                    ::binrw::Error::Custom {
                        pos: position,
                        err: Box::new(error),
                    }
                })
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use proc_macro2::{Ident, Span};

    use super::{
        clean_input_key, generate_conversion_profile_impl, generate_ptp_serde_impl,
        generate_simulation_setting_impl, parse_keys,
    };
    use crate::ast::EnumVariant;

    fn variant(id: &str, name: &str, aliases: &[&str]) -> EnumVariant {
        EnumVariant {
            id: id.to_owned(),
            name: name.to_owned(),
            aliases: aliases.iter().map(|alias| (*alias).to_owned()).collect(),
        }
    }

    #[test]
    fn parse_keys_normalize_the_id_the_display_name_and_every_alias() {
        let variants = [
            variant("pro_neg_hi", "PRO Neg. Hi", &["proneghi", "PRO Neg High"]),
            variant("hdr800_plus", "HDR800+", &["800+", "dr800plus"]),
        ];

        let keys = parse_keys(&variants).expect("distinct variants must resolve");

        assert_eq!(
            keys[0],
            ["proneghi", "proneg.hi", "proneghigh"],
            "the id and the display name are implicit aliases and duplicates collapse"
        );
        assert_eq!(keys[1], ["hdr800plus", "hdr800+", "800+", "dr800plus"]);
        assert_eq!(clean_input_key("  TIFF 8-bit "), "tiff8-bit");
    }

    #[test]
    fn parse_keys_reject_two_variants_that_accept_the_same_normalized_input() {
        let variants = [
            variant("hdr800", "HDR800", &["800"]),
            variant("hdr800_plus", "HDR800+", &["800", "800+"]),
        ];

        let error = parse_keys(&variants).expect_err("a shared key must fail at build time");

        let message = error.to_string();
        assert!(
            message.contains("`hdr800`")
                && message.contains("`hdr800_plus`")
                && message.contains("`800`"),
            "{message}"
        );
    }

    #[test]
    fn parse_keys_reject_an_alias_that_normalizes_to_nothing() {
        let variants = [variant("off", "Off", &["_"])];

        let error = parse_keys(&variants).expect_err("an empty key would match nothing");

        assert!(error.to_string().contains("normalizes to nothing"));
    }

    #[test]
    fn generated_simulation_enum_uses_firmware_scoped_wire_codec() {
        let generated = generate_simulation_setting_impl(
            "film_simulation",
            &Ident::new("FilmSimulation", Span::call_site()),
            &Ident::new("u16", Span::call_site()),
            0xD192,
        )
        .expect("firmware-scoped enum setting must generate")
        .to_string();

        assert!(
            generated.contains("firmware_option_write_value")
                && generated.contains("firmware_option_read_logical_value"),
            "simulation enum must use the selected exact firmware codec: {generated}"
        );
    }

    #[test]
    fn generated_enum_uses_binrw_manual_codec() {
        let type_name = Ident::new("WhiteBalance", Span::call_site());
        let repr_type = Ident::new("u16", Span::call_site());

        let generated = generate_ptp_serde_impl(&type_name, &repr_type)
            .expect("enum wire codec generation must succeed")
            .to_string();

        assert!(
            generated.contains("impl :: binrw :: BinWrite")
                && generated.contains("impl :: binrw :: BinRead")
                && generated.contains("try_from_wire")
                && !generated.contains("ptp_cursor"),
            "generated enum must use the binrw manual codec: {generated}",
        );
    }

    #[test]
    fn generated_enum_conversion_profile_uses_binrw_io() {
        let type_name = Ident::new("WhiteBalance", Span::call_site());
        let repr_type = Ident::new("u16", Span::call_site());
        let repr_type_32 = Ident::new("u32", Span::call_site());

        let generated = generate_conversion_profile_impl(&type_name, &repr_type, &repr_type_32)
            .expect("enum conversion profile generation must succeed")
            .to_string();
        let uses_binrw_traits =
            generated.contains("binrw :: BinWrite") && generated.contains("binrw :: BinRead");
        let uses_binrw_context =
            generated.contains("binrw :: BinResult") && generated.contains("binrw :: Endian");

        assert!(
            (uses_binrw_traits || uses_binrw_context)
                && generated.contains("try_from_wire")
                && generated.contains("write_conversion_profile_field_for_firmware")
                && generated.contains("read_conversion_profile_field_for_firmware")
                && generated.contains("write_wire_value")
                && generated.contains("read_logical_value")
                && !generated.contains("ptp_cursor"),
            "generated enum conversion profile must use binrw I/O: {generated}",
        );
    }
}
