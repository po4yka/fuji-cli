use std::collections::{BTreeMap, BTreeSet};

use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use crate::{
    ast::{Camera, Field, FujiOption},
    common::{options, renders},
    util::ident::safe_upper_camel_case_ident,
};

struct Entry {
    ident: proc_macro2::Ident,
    type_path: TokenStream,
}

pub fn generate(
    options: &BTreeMap<String, FujiOption>,
    cameras: &BTreeMap<String, Camera>,
) -> anyhow::Result<TokenStream> {
    let render_options = collect_render_option_ids(cameras);
    let render_inline_ids = collect_render_inline_ids(cameras);
    let entries = build_entries(options, &render_options, &render_inline_ids);

    let struct_def = generate_struct(&entries);
    let from_impl = generate_from_impl(&entries);

    Ok(quote! {
        #struct_def
        #from_impl
    })
}

fn collect_render_option_ids(cameras: &BTreeMap<String, Camera>) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for camera in cameras.values() {
        let Some(render) = camera
            .spec
            .features
            .as_ref()
            .and_then(|f| f.render.as_ref())
        else {
            continue;
        };
        for field in &render.fields {
            if let Field::Ref(r) = field {
                out.insert(r.r#ref.clone());
            }
        }
    }
    out
}

fn collect_render_inline_ids(cameras: &BTreeMap<String, Camera>) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for camera in cameras.values() {
        let Some(render) = camera
            .spec
            .features
            .as_ref()
            .and_then(|f| f.render.as_ref())
        else {
            continue;
        };
        for field in &render.fields {
            if let Field::Inline(i) = field {
                out.insert(i.id.clone());
            }
        }
    }
    out
}

fn build_entries(
    options: &BTreeMap<String, FujiOption>,
    render_options: &BTreeSet<String>,
    render_inline_ids: &BTreeSet<String>,
) -> Vec<Entry> {
    options
        .values()
        .filter(|opt| !opt.codegen.skip_args)
        .filter(|opt| render_options.contains(&opt.id) && !render_inline_ids.contains(&opt.id))
        .map(|opt| {
            let ident = format_ident!("{}", opt.id);
            let type_ident = safe_upper_camel_case_ident(&opt.id);
            let options_path = options::path();
            let type_path = quote! { #options_path::#type_ident };
            Entry { ident, type_path }
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
        pub struct RenderArgs {
            #( #fields )*
        }
    }
}

fn generate_from_impl(entries: &[Entry]) -> TokenStream {
    let renders_path = renders::path();
    let fields = entries.iter().map(|entry| {
        let ident = &entry.ident;
        quote! { #ident: args.#ident, }
    });

    quote! {
        impl ::std::convert::From<RenderArgs> for #renders_path::RenderBase {
            fn from(args: RenderArgs) -> Self {
                Self {
                    #( #fields )*
                    ..::std::default::Default::default()
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{CameraSpec, Features, FieldInline, FieldRef, Render, Usb};

    fn field_ref(id: &str, r#ref: &str) -> Field {
        Field::Ref(FieldRef {
            id: id.to_string(),
            r#ref: r#ref.to_string(),
            skip_read: false,
            skip_write: false,
        })
    }

    fn field_inline(id: &str) -> Field {
        Field::Inline(FieldInline {
            id: id.to_string(),
            skip_read: false,
            skip_write: false,
        })
    }

    fn camera_with_render_fields(id: &str, fields: Vec<Field>) -> Camera {
        Camera {
            id: id.to_string(),
            spec: CameraSpec {
                name: id.to_string(),
                generation: "gen".to_string(),
                usb: Usb {
                    vendor_id: 1,
                    product_id: 2,
                    chunk_size_ceiling: 1024,
                },
                ptp: None,
                preflight: Vec::new(),
                capabilities: None,
                features: Some(Features {
                    backup: false,
                    simulation: None,
                    render: Some(Render {
                        profile_code: 1,
                        header_padding: 0,
                        fields,
                        transformations: Vec::new(),
                        rules: Vec::new(),
                    }),
                }),
            },
        }
    }

    fn camera_without_features(id: &str) -> Camera {
        Camera {
            id: id.to_string(),
            spec: CameraSpec {
                name: id.to_string(),
                generation: "gen".to_string(),
                usb: Usb {
                    vendor_id: 1,
                    product_id: 2,
                    chunk_size_ceiling: 1024,
                },
                ptp: None,
                preflight: Vec::new(),
                capabilities: None,
                features: None,
            },
        }
    }

    fn camera_with_features_but_no_render(id: &str) -> Camera {
        Camera {
            id: id.to_string(),
            spec: CameraSpec {
                name: id.to_string(),
                generation: "gen".to_string(),
                usb: Usb {
                    vendor_id: 1,
                    product_id: 2,
                    chunk_size_ceiling: 1024,
                },
                ptp: None,
                preflight: Vec::new(),
                capabilities: None,
                features: Some(Features {
                    backup: false,
                    simulation: None,
                    render: None,
                }),
            },
        }
    }

    #[test]
    fn collects_ref_ids_and_skips_inline_fields() {
        let mut cameras = BTreeMap::new();
        cameras.insert(
            "demo".to_string(),
            camera_with_render_fields(
                "demo",
                vec![
                    field_ref("head_0", "image_size"),
                    field_ref("head_1", "film_simulation"),
                    field_inline("tail_0"),
                ],
            ),
        );

        let ref_ids = collect_render_option_ids(&cameras);

        assert_eq!(
            ref_ids,
            BTreeSet::from(["film_simulation".to_string(), "image_size".to_string()])
        );
    }

    #[test]
    fn collects_inline_ids_and_skips_ref_fields() {
        let mut cameras = BTreeMap::new();
        cameras.insert(
            "demo".to_string(),
            camera_with_render_fields(
                "demo",
                vec![
                    field_ref("head_0", "image_size"),
                    field_inline("tail_0"),
                    field_inline("tail_1"),
                ],
            ),
        );

        let inline_ids = collect_render_inline_ids(&cameras);

        assert_eq!(
            inline_ids,
            BTreeSet::from(["tail_0".to_string(), "tail_1".to_string()])
        );
    }

    #[test]
    fn dedupes_ids_shared_across_cameras() {
        let mut cameras = BTreeMap::new();
        cameras.insert(
            "camera_a".to_string(),
            camera_with_render_fields(
                "camera_a",
                vec![field_ref("head_0", "image_size"), field_inline("tail_0")],
            ),
        );
        cameras.insert(
            "camera_b".to_string(),
            camera_with_render_fields(
                "camera_b",
                vec![field_ref("head_0", "image_size"), field_inline("tail_0")],
            ),
        );

        let ref_ids = collect_render_option_ids(&cameras);
        let inline_ids = collect_render_inline_ids(&cameras);

        assert_eq!(ref_ids, BTreeSet::from(["image_size".to_string()]));
        assert_eq!(ref_ids.len(), 1);
        assert_eq!(inline_ids, BTreeSet::from(["tail_0".to_string()]));
        assert_eq!(inline_ids.len(), 1);
    }

    #[test]
    fn skips_cameras_without_a_render_feature() {
        let mut cameras = BTreeMap::new();
        cameras.insert(
            "no_features".to_string(),
            camera_without_features("no_features"),
        );
        cameras.insert(
            "features_no_render".to_string(),
            camera_with_features_but_no_render("features_no_render"),
        );
        cameras.insert(
            "normal".to_string(),
            camera_with_render_fields(
                "normal",
                vec![field_ref("head_0", "image_size"), field_inline("tail_0")],
            ),
        );

        let ref_ids = collect_render_option_ids(&cameras);
        let inline_ids = collect_render_inline_ids(&cameras);

        assert_eq!(ref_ids, BTreeSet::from(["image_size".to_string()]));
        assert_eq!(inline_ids, BTreeSet::from(["tail_0".to_string()]));
    }
}
