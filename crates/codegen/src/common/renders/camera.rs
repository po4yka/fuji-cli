use std::collections::BTreeMap;

use anyhow::Context;
use proc_macro2::{Ident, Literal, TokenStream};
use quote::{format_ident, quote};

use crate::{
    ast::{Camera, Dnf, Field, FujiOption, Render, Transformation},
    common::{cameras, renders},
    schema::{
        alias::{NormalizedRule, NormalizedTransformation},
        grammar::{
            Scopes, SettingInfo, build_settings, generate_apply_transformations, generate_dnf,
            generate_emit_warnings_and_infos,
        },
        inverse::generate_inverses,
        presence::PresenceDag,
        repair::{generate_pin_set, generate_solve},
    },
    util::{dag::Dag, ident::safe_upper_camel_case_ident},
};

// NOTE: Naively assume the same padding holds for all Fujifilm cameras
// until we have a second render-capable camera to compare against.
const RENDER_HEADER_PADDING: usize = 0x1EE;

pub fn generate(
    options: &BTreeMap<String, FujiOption>,
    cameras: &BTreeMap<String, Camera>,
) -> anyhow::Result<TokenStream> {
    let mut blocks = Vec::with_capacity(cameras.len());
    for camera in cameras.values() {
        let block = generate_one(options, camera)
            .with_context(|| format!("generating render profile for camera `{}`", camera.id))?;
        blocks.push(block);
    }
    Ok(quote! { #( #blocks )* })
}

fn generate_one(
    options: &BTreeMap<String, FujiOption>,
    camera: &Camera,
) -> anyhow::Result<TokenStream> {
    let Some(render) = camera
        .spec
        .features
        .as_ref()
        .and_then(|f| f.render.as_ref())
    else {
        return Ok(quote! {});
    };

    let settings = build_settings(options, &render.fields)?;

    let aliases: Vec<NormalizedTransformation> = render
        .transformations
        .iter()
        .cloned()
        .filter_map(Option::from)
        .collect();
    let effective_rules: Vec<NormalizedRule> = render
        .rules
        .iter()
        .map(|r| NormalizedRule::from_rule(r, &aliases))
        .collect();

    let struct_ident = format_ident!("{}RenderProfile", safe_upper_camel_case_ident(&camera.id));
    let camera_struct_ident = safe_upper_camel_case_ident(&camera.id);
    let cameras_path = cameras::path();
    let camera_struct_path = quote! { #cameras_path::#camera_struct_ident };
    let renders_path = renders::path();

    let presence_info = PresenceDag::try_from_rules(&effective_rules)
        .with_context(|| format!("extracting read DAG for `{}`", camera.id))?;

    let nodes: Vec<&str> = render.fields.iter().map(Field::id).collect();
    let edges: Vec<(&str, &str)> = presence_info
        .edges
        .iter()
        .map(|(from, to)| (from.as_str(), to.as_str()))
        .collect();

    let convert_order: Vec<String> = Dag::new(nodes, edges)
        .topological_order()?
        .into_iter()
        .map(str::to_owned)
        .collect();

    let n_props = i16::try_from(render.fields.len())
        .with_context(|| format!("too many render fields on camera `{}`", camera.id))?;
    let profile_code = render.profile_code;

    let struct_def = generate_struct_def(&settings, &render.fields, &struct_ident);
    let inherent_impl = generate_inherent_impl(
        &settings,
        render,
        &effective_rules,
        &struct_ident,
        &renders_path,
        profile_code,
    )?;
    let serialize_impl =
        generate_ptp_serialize_impl(&settings, &render.fields, &struct_ident, n_props);
    let deserialize_impl = generate_ptp_deserialize_impl(
        &settings,
        &render.fields,
        &struct_ident,
        n_props,
        &presence_info.conditions,
        &render.transformations,
        &convert_order,
    )?;
    let trait_impl =
        generate_camera_render_manager_impl(&struct_ident, &camera_struct_path, &renders_path);

    Ok(quote! {
        #struct_def
        #inherent_impl
        #serialize_impl
        #deserialize_impl
        #trait_impl
    })
}

fn generate_struct_def(
    settings: &BTreeMap<&str, SettingInfo<'_>>,
    fields: &[Field],
    struct_ident: &Ident,
) -> TokenStream {
    let field_defs = fields.iter().map(|f| {
        let info = settings.get(f.id()).expect("settings indexed");
        let ident = info.field_ident();
        let type_path = info.type_path();
        quote! { pub #ident: Option<#type_path>, }
    });

    quote! {
        #[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
        #[serde(default, rename_all = "camelCase")]
        pub struct #struct_ident {
            #( #field_defs )*
        }
    }
}

fn generate_inherent_impl(
    settings: &BTreeMap<&str, SettingInfo<'_>>,
    render: &Render,
    effective_rules: &[NormalizedRule],
    struct_ident: &Ident,
    renders_path: &TokenStream,
    profile_code: u32,
) -> anyhow::Result<TokenStream> {
    let profile_code_lit = Literal::u32_suffixed(profile_code);

    let apply_transformations = generate_apply_transformations(settings, &render.transformations)?;
    let self_acc = quote! { self };
    let original_acc = quote! { original };
    let warnings_infos = generate_emit_warnings_and_infos(
        settings,
        effective_rules,
        Scopes::with_original(&self_acc, &original_acc),
    )?;
    let solve = generate_solve(settings, effective_rules, true)?;
    let try_update_from =
        generate_try_update_from(settings, &render.fields, renders_path, struct_ident)?;

    Ok(quote! {
        impl #struct_ident {
            pub const PROFILE_CODE: u32 = #profile_code_lit;

            #apply_transformations
            #warnings_infos
            #solve
            #try_update_from
        }
    })
}

fn generate_try_update_from(
    settings: &BTreeMap<&str, SettingInfo<'_>>,
    fields: &[Field],
    renders_path: &TokenStream,
    struct_ident: &Ident,
) -> anyhow::Result<TokenStream> {
    let init_fields = fields.iter().map(|f| {
        let info = settings.get(f.id()).expect("settings indexed");
        let ident = info.field_ident();
        quote! { #ident: partial.#ident, }
    });

    let merge_assigns = fields.iter().map(|f| {
        let info = settings.get(f.id()).expect("settings indexed");
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
            partial: #renders_path::RenderBase,
        ) -> ::anyhow::Result<()> {
            let original = self.clone();
            let mut partial_profile: #struct_ident = #struct_ident {
                #( #init_fields )*
            };
            partial_profile.apply_transformations();

            let pin = #pin_set_expr;

            let mut candidate = self.clone();
            #( #merge_assigns )*
            candidate.apply_transformations();

            candidate.solve(&pin, &original)?;
            candidate.emit_warnings_and_infos(&original)?;

            *self = candidate;
            Ok(())
        }
    })
}

fn generate_ptp_serialize_impl(
    settings: &BTreeMap<&str, SettingInfo<'_>>,
    fields: &[Field],
    struct_ident: &Ident,
    n_props: i16,
) -> TokenStream {
    let n_props_lit = Literal::i16_suffixed(n_props);
    let padding_lit = Literal::usize_suffixed(RENDER_HEADER_PADDING);

    let writes = fields
        .iter()
        .map(|field| generate_write_one(settings, field));

    quote! {
        impl ::binrw::BinWrite for #struct_ident {
            type Args<'a> = ();

            fn write_options<W: ::std::io::Write + ::std::io::Seek>(
                &self,
                writer: &mut W,
                _endian: ::binrw::Endian,
                (): Self::Args<'_>,
            ) -> ::binrw::BinResult<()> {
                let endian = ::binrw::Endian::Little;
                let n_props: i16 = #n_props_lit;
                <i16 as ::binrw::BinWrite>::write_options(
                    &n_props, writer, endian, (),
                )?;
                let profile_code_text = format!("{:x}", Self::PROFILE_CODE);
                let profile_code = crate::ptp::codec::PtpExactString::from(
                    profile_code_text.as_str(),
                );
                <crate::ptp::codec::PtpExactString as ::binrw::BinWrite>::write_options(
                    &profile_code, writer, endian, (),
                )?;
                let padding = [0u8; #padding_lit];
                ::std::io::Write::write_all(writer, &padding)?;

                #( #writes )*

                Ok(())
            }
        }
    }
}

fn generate_write_one(settings: &BTreeMap<&str, SettingInfo<'_>>, field: &Field) -> TokenStream {
    if field.skip_write() {
        return quote! {};
    }
    let info = settings.get(field.id()).expect("settings indexed");
    let ident = info.field_ident();
    let type_path = info.type_path();
    if info.option.is_some() {
        quote! {
            match self.#ident.as_ref() {
                Some(value) => {
                    <#type_path as crate::ptp::option::ConversionProfileField>
                        ::write_conversion_profile_field(value, writer, endian)?;
                }
                None => {
                    <i32 as ::binrw::BinWrite>::write_options(
                        &0i32, writer, endian, (),
                    )?;
                }
            }
        }
    } else {
        quote! {
            let value: i32 = self.#ident.unwrap_or(0);
            <i32 as ::binrw::BinWrite>::write_options(&value, writer, endian, ())?;
        }
    }
}

fn generate_ptp_deserialize_impl(
    settings: &BTreeMap<&str, SettingInfo<'_>>,
    fields: &[Field],
    struct_ident: &Ident,
    n_props: i16,
    presence_conditions: &BTreeMap<String, Dnf>,
    transformations: &[Transformation],
    convert_order: &[String],
) -> anyhow::Result<TokenStream> {
    let n_props_lit = Literal::i16_suffixed(n_props);
    let padding_lit = Literal::usize_suffixed(RENDER_HEADER_PADDING);

    let raw_reads: Vec<TokenStream> = fields
        .iter()
        .filter(|f| !f.skip_read())
        .map(|field| {
            let info = settings.get(field.id()).expect("settings indexed");
            let raw_ident = raw_local_ident(&info.field_ident());
            quote! {
                let #raw_ident = <i32 as ::binrw::BinRead>::read_options(
                    reader, endian, (),
                )?;
            }
        })
        .collect();

    let conversions = convert_order
        .iter()
        .map(|id| {
            let field = fields
                .iter()
                .find(|f| f.id() == id.as_str())
                .expect("convert order references known field");
            generate_convert_one(settings, field, presence_conditions)
        })
        .collect::<anyhow::Result<Vec<_>>>()?;

    let inverses = generate_inverses(settings, transformations, &quote! { profile })?;

    Ok(quote! {
        impl ::binrw::BinRead for #struct_ident {
            type Args<'a> = ();

            #[allow(clippy::field_reassign_with_default)]
            fn read_options<R: ::std::io::Read + ::std::io::Seek>(
                reader: &mut R,
                _endian: ::binrw::Endian,
                (): Self::Args<'_>,
            ) -> ::binrw::BinResult<Self> {
                let endian = ::binrw::Endian::Little;
                let position = ::std::io::Seek::stream_position(reader)?;
                let n_props = <i16 as ::binrw::BinRead>::read_options(reader, endian, ())?;
                if n_props != #n_props_lit {
                    return Err(::binrw::Error::AssertFail {
                        pos: position,
                        message: format!(
                            "{}: expected {} props on the wire, got {}",
                            stringify!(#struct_ident),
                            #n_props_lit,
                            n_props,
                        ),
                    });
                }
                let profile_code_str =
                    <crate::ptp::codec::PtpExactString as ::binrw::BinRead>
                        ::read_options(reader, endian, ())?;
                let parsed = u32::from_str_radix(profile_code_str.as_str(), 16)
                    .map_err(|err| ::binrw::Error::Custom {
                        pos: position,
                        err: Box::new(::std::io::Error::new(
                            ::std::io::ErrorKind::InvalidData,
                            format!(
                                "{}: invalid profile-code hex `{}`: {}",
                                stringify!(#struct_ident),
                                profile_code_str.as_str(),
                                err,
                            ),
                        )),
                    })?;
                if parsed != Self::PROFILE_CODE {
                    return Err(::binrw::Error::AssertFail {
                        pos: position,
                        message: format!(
                            "{}: expected profile code {:#x}, got {:#x}",
                            stringify!(#struct_ident),
                            Self::PROFILE_CODE,
                            parsed,
                        ),
                    });
                }
                let mut padding = [0u8; #padding_lit];
                <R as ::std::io::Read>::read_exact(reader, &mut padding)?;

                #( #raw_reads )*

                let mut profile = Self::default();
                #( #conversions )*

                #inverses

                Ok(profile)
            }
        }
    })
}

fn generate_convert_one(
    settings: &BTreeMap<&str, SettingInfo<'_>>,
    field: &Field,
    presence_conditions: &BTreeMap<String, Dnf>,
) -> anyhow::Result<TokenStream> {
    if field.skip_read() {
        return Ok(quote! {});
    }

    let info = settings.get(field.id()).expect("settings indexed");
    let ident = info.field_ident();
    let type_path = info.type_path();
    let raw_ident = raw_local_ident(&ident);

    let convert = if info.option.is_some() {
        quote! {
            let mut raw_reader = ::std::io::Cursor::new(#raw_ident.to_le_bytes());
            profile.#ident = Some(
                <#type_path as crate::ptp::option::ConversionProfileField>
                    ::read_conversion_profile_field(&mut raw_reader, ::binrw::Endian::Little)?,
            );
        }
    } else {
        quote! { profile.#ident = Some(#raw_ident); }
    };

    if let Some(condition) = presence_conditions.get(field.id()) {
        let profile_accessor = quote! { profile };
        let cond = generate_dnf(settings, condition, Scopes::new(&profile_accessor))?;
        Ok(quote! {
            if #cond {
                #convert
            }
        })
    } else {
        Ok(convert)
    }
}

fn raw_local_ident(ident: &Ident) -> Ident {
    format_ident!("raw_{}", ident)
}

fn generate_camera_render_manager_impl(
    struct_ident: &Ident,
    camera_struct_path: &TokenStream,
    renders_path: &TokenStream,
) -> TokenStream {
    quote! {
        impl crate::features::render::CameraRenderManager for #camera_struct_path {
            fn render(
                &self,
                ptp: &mut crate::ptp::Ptp,
                image: &[u8],
                partial: #renders_path::RenderBase,
                draft: bool,
            ) -> ::anyhow::Result<Vec<u8>> {
                <Self as crate::features::render::CameraRenderManager>::send_image(self, ptp, image)?;
                let mut profile: #struct_ident = ptp.get_prop(
                    crate::ptp::DevicePropCode::FujiRawConversionProfile,
                )?;
                profile.try_update_from(partial)?;
                ptp.set_prop(
                    crate::ptp::DevicePropCode::FujiRawConversionProfile,
                    &profile,
                )?;
                <Self as crate::features::render::CameraRenderManager>::render_image(
                    self, ptp, draft,
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use proc_macro2::{Ident, Span};

    use crate::{
        ast::{Codegen, Field, FieldRef, FujiOption, NumericEncoding, OptionSpec, SpecKind},
        schema::grammar::SettingInfo,
    };

    use super::{generate_ptp_deserialize_impl, generate_ptp_serialize_impl};

    #[test]
    fn generated_render_decoder_is_streaming_binread() {
        let generated = generate_ptp_deserialize_impl(
            &BTreeMap::new(),
            &[],
            &Ident::new("TestRenderProfile", Span::call_site()),
            0,
            &BTreeMap::new(),
            &[],
            &[],
        )
        .expect("minimal render decoder should generate")
        .to_string();

        assert!(
            generated.contains("impl :: binrw :: BinRead")
                && !generated.contains("ptp_cursor")
                && !generated.contains("expect_end"),
            "generated decoder must defer exact-buffer checks to the PTP codec boundary:\n{generated}",
        );
    }

    #[test]
    fn generated_render_profile_uses_binrw_manual_codec() {
        let option = FujiOption {
            id: "exposure_compensation".to_owned(),
            spec: OptionSpec::Integer {
                name: "Exposure Compensation".to_owned(),
                rules: None,
                encoding: NumericEncoding::Raw { prop_code: None },
            },
            codegen: Codegen::default(),
        };
        let fields = [Field::Ref(FieldRef {
            id: "exposure_compensation".to_owned(),
            r#ref: "exposure_compensation".to_owned(),
            skip_read: false,
            skip_write: false,
        })];
        let mut settings = BTreeMap::new();
        settings.insert(
            "exposure_compensation",
            SettingInfo {
                id: "exposure_compensation",
                kind: SpecKind::Integer,
                option: Some(&option),
            },
        );
        let struct_ident = Ident::new("TestRenderProfile", Span::call_site());
        let serialize = generate_ptp_serialize_impl(&settings, &fields, &struct_ident, 1);
        let deserialize = generate_ptp_deserialize_impl(
            &settings,
            &fields,
            &struct_ident,
            1,
            &BTreeMap::new(),
            &[],
            &["exposure_compensation".to_owned()],
        )
        .expect("minimal render decoder should generate");
        let generated = format!("{serialize} {deserialize}");

        assert!(
            generated.contains("impl :: binrw :: BinWrite")
                && generated.contains("impl :: binrw :: BinRead")
                && generated.contains("ConversionProfileField")
                && generated.contains("raw_exposure_compensation")
                && !generated.contains("ptp_cursor"),
            "generated render profile must use binrw while preserving conversion order: {generated}",
        );
    }
}
