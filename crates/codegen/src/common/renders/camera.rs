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
    let firmware_codec_impl = generate_firmware_profile_codec_impl(
        &settings,
        &render.fields,
        &struct_ident,
        n_props,
        &presence_info.conditions,
        &render.transformations,
        &convert_order,
    )?;
    let trait_impl = generate_camera_render_manager_impl(
        &struct_ident,
        &camera_struct_path,
        &renders_path,
        &render.fields,
    );

    Ok(quote! {
        #struct_def
        #inherent_impl
        #serialize_impl
        #deserialize_impl
        #firmware_codec_impl
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
    let header_padding = usize::try_from(render.header_padding)
        .context("render header padding does not fit the target pointer width")?;
    let header_padding_lit = Literal::usize_suffixed(header_padding);

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
            pub const HEADER_PADDING: usize = #header_padding_lit;

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
                crate::ptp::codec::write_zero_padding(writer, Self::HEADER_PADDING)?;

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

fn generate_write_one_for_firmware(
    settings: &BTreeMap<&str, SettingInfo<'_>>,
    field: &Field,
) -> TokenStream {
    if field.skip_write() {
        return quote! {};
    }
    let info = settings.get(field.id()).expect("settings indexed");
    let ident = info.field_ident();
    let type_path = info.type_path();
    if let Some(option) = info.option {
        let option_id = &option.id;
        quote! {
            match self.#ident.as_ref() {
                Some(value) => {
                    <#type_path as crate::ptp::option::ConversionProfileField>
                        ::write_conversion_profile_field_for_firmware(
                            value,
                            &mut writer,
                            endian,
                            firmware_profile,
                            #option_id,
                        )?;
                }
                None => {
                    <i32 as ::binrw::BinWrite>::write_options(
                        &0i32, &mut writer, endian, (),
                    )?;
                }
            }
        }
    } else {
        quote! {
            let value: i32 = self.#ident.unwrap_or(0);
            <i32 as ::binrw::BinWrite>::write_options(&value, &mut writer, endian, ())?;
        }
    }
}

fn generate_firmware_profile_codec_impl(
    settings: &BTreeMap<&str, SettingInfo<'_>>,
    fields: &[Field],
    struct_ident: &Ident,
    n_props: i16,
    presence_conditions: &BTreeMap<String, Dnf>,
    transformations: &[Transformation],
    convert_order: &[String],
) -> anyhow::Result<TokenStream> {
    let n_props_lit = Literal::i16_suffixed(n_props);
    let writes = fields
        .iter()
        .map(|field| generate_write_one_for_firmware(settings, field));
    let raw_reads = fields
        .iter()
        .filter(|field| !field.skip_read())
        .map(|field| {
            let info = settings.get(field.id()).expect("settings indexed");
            let raw_ident = raw_local_ident(&info.field_ident());
            quote! {
                let #raw_ident = <i32 as ::binrw::BinRead>::read_options(
                    &mut reader, endian, (),
                )?;
            }
        });
    let conversions = convert_order
        .iter()
        .map(|id| {
            let field = fields
                .iter()
                .find(|field| field.id() == id)
                .expect("convert order references known field");
            generate_convert_one_for_firmware(settings, field, presence_conditions)
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let inverses = generate_inverses(settings, transformations, &quote! { camera_profile })?;

    Ok(quote! {
        impl #struct_ident {
            pub(crate) fn encode_for_firmware(
                &self,
                firmware_profile: &crate::generated::cameras::CameraFirmwareCapabilityProfile,
            ) -> ::anyhow::Result<Vec<u8>> {
                let mut writer = ::std::io::Cursor::new(Vec::new());
                let endian = ::binrw::Endian::Little;
                <i16 as ::binrw::BinWrite>::write_options(
                    &#n_props_lit, &mut writer, endian, (),
                )?;
                let profile_code_text = format!("{:x}", Self::PROFILE_CODE);
                let profile_code = crate::ptp::codec::PtpExactString::from(
                    profile_code_text.as_str(),
                );
                <crate::ptp::codec::PtpExactString as ::binrw::BinWrite>::write_options(
                    &profile_code, &mut writer, endian, (),
                )?;
                crate::ptp::codec::write_zero_padding(&mut writer, Self::HEADER_PADDING)?;
                #(#writes)*
                Ok(writer.into_inner())
            }

            #[allow(
                clippy::field_reassign_with_default,
                reason = "generated field decoding assigns options in wire order"
            )]
            pub(crate) fn decode_for_firmware(
                bytes: &[u8],
                firmware_profile: &crate::generated::cameras::CameraFirmwareCapabilityProfile,
            ) -> ::anyhow::Result<Self> {
                let mut reader = ::std::io::Cursor::new(bytes);
                let endian = ::binrw::Endian::Little;
                let n_props = <i16 as ::binrw::BinRead>::read_options(
                    &mut reader, endian, (),
                )?;
                ::anyhow::ensure!(
                    n_props == #n_props_lit,
                    "{}: expected {} props on the wire, got {}",
                    stringify!(#struct_ident),
                    #n_props_lit,
                    n_props,
                );
                let profile_code_str =
                    <crate::ptp::codec::PtpExactString as ::binrw::BinRead>
                        ::read_options(&mut reader, endian, ())?;
                let parsed = u32::from_str_radix(profile_code_str.as_str(), 16)?;
                ::anyhow::ensure!(
                    parsed == Self::PROFILE_CODE,
                    "{}: expected profile code {:#x}, got {:#x}",
                    stringify!(#struct_ident),
                    Self::PROFILE_CODE,
                    parsed,
                );
                crate::ptp::codec::consume_padding(&mut reader, Self::HEADER_PADDING)?;
                #(#raw_reads)*

                let mut camera_profile = Self::default();
                #(#conversions)*
                #inverses

                ::anyhow::ensure!(
                    reader.position() == bytes.len() as u64,
                    "{} firmware profile has {} trailing bytes",
                    stringify!(#struct_ident),
                    bytes.len() as u64 - reader.position(),
                );
                Ok(camera_profile)
            }
        }
    })
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

            #[allow(
                clippy::field_reassign_with_default,
                reason = "generated field decoding assigns options in wire order"
            )]
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
                crate::ptp::codec::consume_padding(reader, Self::HEADER_PADDING)?;

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

fn generate_convert_one_for_firmware(
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
    let convert = if let Some(option) = info.option {
        let option_id = &option.id;
        quote! {
            let mut raw_reader = ::std::io::Cursor::new(#raw_ident.to_le_bytes());
            camera_profile.#ident = Some(
                <#type_path as crate::ptp::option::ConversionProfileField>
                    ::read_conversion_profile_field_for_firmware(
                        &mut raw_reader,
                        ::binrw::Endian::Little,
                        firmware_profile,
                        #option_id,
                    )?,
            );
        }
    } else {
        quote! { camera_profile.#ident = Some(#raw_ident); }
    };

    if let Some(condition) = presence_conditions.get(field.id()) {
        let profile_accessor = quote! { camera_profile };
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
    fields: &[Field],
) -> TokenStream {
    let field_ids = fields.iter().map(Field::id).collect::<Vec<_>>();
    quote! {
        impl crate::features::render::CameraRenderManager for #camera_struct_path {
            fn render(
                &self,
                ptp: &mut crate::ptp::Ptp,
                image: &[u8],
                partial: #renders_path::RenderBase,
                draft: bool,
            ) -> ::anyhow::Result<crate::features::render::RenderOutcome> {
                let firmware_profile = ptp.firmware_capability_profile()?;
                firmware_profile.validate_raw_conversion_signature(
                    #struct_ident::PROFILE_CODE,
                    #struct_ident::HEADER_PADDING,
                    &[#(#field_ids),*],
                )?;
                partial.validate_firmware_capabilities(firmware_profile)?;
                let original_profile =
                    crate::features::render::manager::snapshot_raw_conversion_profile(ptp)?;
                let mut profile = #struct_ident::decode_for_firmware(
                    &original_profile,
                    firmware_profile,
                )?;
                profile.try_update_from(partial)?;
                let candidate_profile = profile.encode_for_firmware(firmware_profile)?;
                ptp.validate_raw_conversion_profile(
                    #struct_ident::PROFILE_CODE,
                    #struct_ident::HEADER_PADDING,
                    &[#(#field_ids),*],
                    &candidate_profile,
                )?;

                <Self as crate::features::render::CameraRenderManager>::send_image(
                    self, ptp, image,
                )?;

                if candidate_profile == original_profile {
                    let render =
                        <Self as crate::features::render::CameraRenderManager>::render_object(
                            self, ptp, draft,
                        );
                    return crate::features::render::combine_render_and_restore(render, Ok(()));
                }

                let render = (|| {
                    crate::features::render::manager::write_raw_conversion_profile_verified(
                        ptp,
                        &candidate_profile,
                    )?;
                    <Self as crate::features::render::CameraRenderManager>::render_object(
                        self, ptp, draft,
                    )
                })();

                let restore = if ptp.is_healthy() {
                    ::anyhow::Context::context(
                        crate::features::render::manager::write_raw_conversion_profile_verified(
                            ptp,
                            &original_profile,
                        ),
                        "restoring the original RAW conversion profile",
                    )
                } else {
                    Err(::anyhow::anyhow!(
                        "PTP session is unhealthy; RAW conversion profile state is unknown",
                    ))
                };

                crate::features::render::combine_render_and_restore(render, restore)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use proc_macro2::{Ident, Span};

    use crate::{
        ast::{
            Camera, Codegen, Field, FieldRef, FujiOption, NumericEncoding, OptionSpec, SpecKind,
        },
        schema::grammar::SettingInfo,
    };

    use super::{generate, generate_ptp_deserialize_impl, generate_ptp_serialize_impl};

    #[test]
    fn generated_render_restores_and_verifies_the_conversion_profile() {
        let camera = serde_json::from_str::<Camera>(
            r#"{
                "id": "fixture",
                "spec": {
                    "name": "Fixture",
                    "generation": "fixture",
                    "usb": { "vendor_id": 1227, "product_id": 1, "chunk_size_ceiling": 1024 },
                    "features": {
                        "render": { "profile_code": 1, "header_padding": 0, "fields": [] }
                    }
                }
            }"#,
        )
        .expect("render fixture must parse");
        let generated = generate(
            &BTreeMap::new(),
            &BTreeMap::from([(camera.id.clone(), camera)]),
        )
        .expect("render fixture must generate")
        .to_string();

        let snapshot = generated
            .find("snapshot_raw_conversion_profile")
            .expect("render must snapshot the raw profile");
        let upload = generated
            .find("CameraRenderManager > :: send_image")
            .expect("render must upload the RAF");
        assert!(
            snapshot < upload,
            "profile snapshot must precede upload: {generated}"
        );
        assert!(
            generated
                .matches("write_raw_conversion_profile_verified")
                .count()
                == 2
                && generated.contains("restoring the original RAW conversion profile")
                && generated.contains("combine_render_and_restore"),
            "render must verify both target and restored raw values and retain cleanup outcomes: {generated}",
        );
    }

    #[test]
    fn generated_render_validates_profile_before_uploading_image() {
        let camera = serde_json::from_str::<Camera>(
            r#"{
                "id": "fixture",
                "spec": {
                    "name": "Fixture",
                    "generation": "fixture",
                    "usb": { "vendor_id": 1227, "product_id": 1, "chunk_size_ceiling": 1024 },
                    "features": {
                        "render": { "profile_code": 1, "header_padding": 0, "fields": [] }
                    }
                }
            }"#,
        )
        .expect("render fixture must parse");
        let generated = generate(
            &BTreeMap::new(),
            &BTreeMap::from([(camera.id.clone(), camera)]),
        )
        .expect("render fixture must generate")
        .to_string();

        let profile_read = generated
            .find("snapshot_raw_conversion_profile")
            .expect("generated render must read the conversion profile");
        let upload = generated
            .find("CameraRenderManager > :: send_image")
            .expect("generated render must upload the image");

        assert!(
            profile_read < upload,
            "conversion profile must be validated before RAF upload: {generated}"
        );
        assert!(
            generated.contains("decode_for_firmware")
                && generated.contains("encode_for_firmware")
                && generated.contains("validate_raw_conversion_profile"),
            "RAW conversion must use the exact firmware codec and upload gate: {generated}"
        );
    }

    #[test]
    fn generated_render_rejects_unsupported_values_before_profile_io() {
        let camera = serde_json::from_str::<Camera>(
            r#"{
                "id": "fixture",
                "spec": {
                    "name": "Fixture",
                    "generation": "fixture",
                    "usb": { "vendor_id": 1227, "product_id": 1, "chunk_size_ceiling": 1024 },
                    "features": {
                        "render": { "profile_code": 1, "header_padding": 0, "fields": [] }
                    }
                }
            }"#,
        )
        .expect("render fixture must parse");
        let generated = generate(
            &BTreeMap::new(),
            &BTreeMap::from([(camera.id.clone(), camera)]),
        )
        .expect("render fixture must generate")
        .to_string();

        let capability_validation = generated
            .find("validate_firmware_capabilities")
            .expect("generated render must validate firmware values");
        let signature_validation = generated
            .find("validate_raw_conversion_signature")
            .expect("generated render must validate the exact firmware wire layout");
        let profile_read = generated
            .find("snapshot_raw_conversion_profile")
            .expect("generated render must read the conversion profile");

        assert!(capability_validation < profile_read);
        assert!(signature_validation < profile_read);
    }

    #[test]
    fn render_header_padding_is_required_and_drives_both_wire_directions() {
        let parsed = serde_json::from_str::<Camera>(
            r#"{
                "id": "fixture",
                "spec": {
                    "name": "Fixture",
                    "generation": "fixture",
                    "usb": {
                        "vendor_id": 1227,
                        "product_id": 1,
                        "chunk_size_ceiling": 1024
                    },
                    "features": {
                        "render": {
                            "profile_code": 1,
                            "header_padding": 37,
                            "fields": []
                        }
                    }
                }
            }"#,
        );
        assert!(
            parsed.is_ok(),
            "camera-specific render header padding must be accepted: {parsed:?}"
        );

        let camera = parsed.expect("checked above");
        let cameras = BTreeMap::from([(camera.id.clone(), camera)]);
        let generated = generate(&BTreeMap::new(), &cameras)
            .expect("fixture camera should generate")
            .to_string();

        assert!(
            generated.contains("pub const HEADER_PADDING : usize = 37usize"),
            "generated profile must retain its camera-specific padding: {generated}"
        );
        assert_eq!(
            generated.matches("Self :: HEADER_PADDING").count(),
            4,
            "global and firmware codecs must use the same camera-specific padding: {generated}"
        );
        assert!(
            generated.contains("write_zero_padding (writer , Self :: HEADER_PADDING)"),
            "serializer must use bounded padding I/O: {generated}"
        );
        assert!(
            generated.contains("consume_padding (reader , Self :: HEADER_PADDING)"),
            "deserializer must use bounded padding I/O: {generated}"
        );

        let missing = serde_json::from_str::<Camera>(
            r#"{
                "id": "fixture",
                "spec": {
                    "name": "Fixture",
                    "generation": "fixture",
                    "usb": {
                        "vendor_id": 1227,
                        "product_id": 1,
                        "chunk_size_ceiling": 1024
                    },
                    "features": {
                        "render": { "profile_code": 1, "fields": [] }
                    }
                }
            }"#,
        );
        assert!(missing.is_err(), "render header padding must be required");
    }

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
