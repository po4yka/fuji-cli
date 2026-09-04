#![forbid(unsafe_code)]

pub mod ast;
mod cli;
mod common;
mod schema;
mod util;

use std::{
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};

use anyhow::{Context, bail};
use proc_macro2::TokenStream;
use quote::quote;

pub fn generate(json: &str, out_dir: &Path) -> anyhow::Result<()> {
    let fml: ast::Fml = serde_json::from_str(json).context("parsing FML JSON")?;
    let staging = create_staging_dir(out_dir)?;

    if let Err(error) = generate_into(&fml, &staging) {
        return match fs::remove_dir_all(&staging) {
            Ok(()) => Err(error),
            Err(cleanup) => Err(error.context(format!(
                "also failed to remove staging directory {}: {cleanup}",
                staging.display(),
            ))),
        };
    }

    publish(&staging, out_dir)
}

fn generate_into(fml: &ast::Fml, out_dir: &Path) -> anyhow::Result<()> {
    schema::capabilities::validate_verified_profile_coverage(&fml.options, &fml.cameras)
        .context("validating verified firmware capability coverage")?;
    schema::preflight::validate_option_data_types(&fml.options)
        .context("validating pinned option datatypes")?;
    schema::preflight::validate_static_descriptors(&fml.options, &fml.cameras)
        .context("validating preflight static descriptors")?;

    let options = common::options::generate(&fml.options).context("generating option types")?;
    write(out_dir, "options", options)?;

    let cameras = common::cameras::generate(&fml.cameras).context("generating camera registry")?;
    write(out_dir, "cameras", cameras)?;

    let simulations = common::simulations::generate(&fml.options, &fml.cameras)
        .context("generating simulation types")?;
    write(out_dir, "simulations", simulations)?;

    let renders = common::renders::generate(&fml.options, &fml.cameras)
        .context("generating render profile types")?;
    write(out_dir, "renders", renders)?;

    let cli = cli::generate(&fml.options, &fml.cameras).context("generating CLI args")?;
    write(out_dir, "cli", cli)?;

    let mod_rs = root(fml);
    write(out_dir, "mod", mod_rs)?;

    Ok(())
}

static NEXT_TEMPORARY: AtomicUsize = AtomicUsize::new(0);

fn create_staging_dir(out_dir: &Path) -> anyhow::Result<PathBuf> {
    let parent = out_dir
        .parent()
        .context("generated output directory must have a parent")?;
    let name = out_dir
        .file_name()
        .context("generated output directory must have a file name")?
        .to_string_lossy();

    for _ in 0..100 {
        let sequence = NEXT_TEMPORARY.fetch_add(1, Ordering::Relaxed);
        let staging = parent.join(format!(".{name}.tmp-{}-{sequence}", std::process::id(),));
        match fs::create_dir(&staging) {
            Ok(()) => return Ok(staging),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("creating staging directory {}", staging.display()));
            }
        }
    }

    bail!(
        "could not allocate a staging directory next to {}",
        out_dir.display(),
    )
}

fn unused_sibling(out_dir: &Path, label: &str) -> anyhow::Result<PathBuf> {
    let parent = out_dir
        .parent()
        .context("generated output directory must have a parent")?;
    let name = out_dir
        .file_name()
        .context("generated output directory must have a file name")?
        .to_string_lossy();

    for _ in 0..100 {
        let sequence = NEXT_TEMPORARY.fetch_add(1, Ordering::Relaxed);
        let candidate = parent.join(format!(".{name}.{label}-{}-{sequence}", std::process::id(),));
        match fs::symlink_metadata(&candidate) {
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(candidate),
            Ok(_) => {}
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("checking temporary path {}", candidate.display()));
            }
        }
    }

    bail!(
        "could not allocate a {label} path next to {}",
        out_dir.display(),
    )
}

fn publish(staging: &Path, out_dir: &Path) -> anyhow::Result<()> {
    if !out_dir.exists() {
        return fs::rename(staging, out_dir).with_context(|| {
            format!(
                "publishing generated modules from {} to {}",
                staging.display(),
                out_dir.display(),
            )
        });
    }

    let previous = unused_sibling(out_dir, "previous")?;
    fs::rename(out_dir, &previous).with_context(|| {
        format!(
            "moving previous generated modules from {} to {}",
            out_dir.display(),
            previous.display(),
        )
    })?;

    if let Err(publish_error) = fs::rename(staging, out_dir) {
        let rollback = fs::rename(&previous, out_dir);
        drop(fs::remove_dir_all(staging));
        return match rollback {
            Ok(()) => Err(publish_error).with_context(|| {
                format!(
                    "publishing generated modules from {} to {}",
                    staging.display(),
                    out_dir.display(),
                )
            }),
            Err(rollback_error) => Err(publish_error).context(format!(
                "publishing generated modules failed and restoring {} from {} also failed: {rollback_error}",
                out_dir.display(),
                previous.display(),
            )),
        };
    }

    fs::remove_dir_all(&previous)
        .with_context(|| format!("removing previous output {}", previous.display()))?;
    Ok(())
}

fn root(fml: &ast::Fml) -> TokenStream {
    let banner = format!(
        "Generated via codegen. Do not edit. \
         Inventory: {} cameras, {} options",
        fml.cameras.len(),
        fml.options.len(),
    );

    quote! {
        #![doc = #banner]
        #![allow(
            clippy::nonminimal_bool,
            clippy::trivially_copy_pass_by_ref,
            clippy::unused_self,
            reason = "generated schema predicates and uniform signatures prioritize deterministic emission"
        )]

        pub mod cameras;
        pub mod options;
        pub mod simulations;
        pub mod renders;
        pub mod cli;
    }
}

fn write(out_dir: &Path, name: &str, tokens: TokenStream) -> anyhow::Result<()> {
    let formatted =
        format(tokens).with_context(|| format!("formatting generated module `{name}`"))?;
    let path = out_dir.join(format!("{name}.rs"));
    fs::write(&path, formatted).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

fn format(tokens: TokenStream) -> anyhow::Result<String> {
    let file: syn::File = syn::parse2(tokens).context("parsing generated TokenStream")?;
    Ok(prettyplease::unparse(&file))
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        sync::atomic::{AtomicUsize, Ordering},
    };

    use super::generate;

    static NEXT_TEMP_DIR: AtomicUsize = AtomicUsize::new(0);

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> std::io::Result<Self> {
            let sequence = NEXT_TEMP_DIR.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "fujicli-codegen-test-{}-{sequence}",
                std::process::id(),
            ));
            fs::create_dir(&path)?;
            Ok(Self(path))
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            drop(fs::remove_dir_all(&self.0));
        }
    }

    #[test]
    fn failed_generation_preserves_the_last_complete_output() -> anyhow::Result<()> {
        let temp = TempDir::new()?;
        let output = temp.0.join("generated");
        fs::create_dir(&output)?;
        let sentinel = output.join("sentinel");
        fs::write(&sentinel, "known good")?;

        let invalid_fml = r#"{
            "cameras": {},
            "generations": {},
            "options": {
                "broken": {
                    "id": "broken",
                    "spec": {
                        "name": "Broken",
                        "kind": "integer",
                        "rules": { "min": -1, "max": 40000, "step": 1 },
                        "encoding": { "kind": "raw" }
                    }
                }
            }
        }"#;

        generate(invalid_fml, &output)
            .expect_err("the invalid wire range must make generation fail");

        let published = fs::read_to_string(sentinel).unwrap_or_else(|_| "<missing>".to_owned());
        assert_eq!(published, "known good");

        Ok(())
    }

    fn listed_files(out_dir: &Path) -> anyhow::Result<Vec<String>> {
        Ok(fs::read_dir(out_dir)?
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect())
    }

    #[test]
    fn generate_twice_from_live_fml_is_byte_identical() -> anyhow::Result<()> {
        use anyhow::{Context, ensure};
        use std::process::Command;

        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .context("resolving the repository root two levels above crates/codegen")?;
        ensure!(
            repo_root.join("fml").is_dir(),
            "the fml schema directory must sit at the repository root"
        );

        let export = match Command::new("cue")
            .args(["export", "./fml", "--out", "json"])
            .current_dir(&repo_root)
            .output()
        {
            Ok(output) => output,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                eprintln!(
                    "skipping generate_twice_from_live_fml_is_byte_identical: `cue` is not on PATH"
                );
                return Ok(());
            }
            Err(error) => {
                return Err(error).context("spawning `cue export ./fml --out json`");
            }
        };
        ensure!(
            export.status.success(),
            "`cue export ./fml --out json` failed:\n{}",
            String::from_utf8_lossy(&export.stderr),
        );
        let json = str::from_utf8(&export.stdout).context("the CUE export must be UTF-8 JSON")?;

        let first = TempDir::new()?;
        let second = TempDir::new()?;
        let out_first = first.0.join("generated");
        let out_second = second.0.join("generated");

        generate(json, &out_first)?;
        generate(json, &out_second)?;

        // The published file set is exactly what `generate_into` writes, so an
        // extra or missing file is itself a determinism failure.
        const EXPECTED_FILES: [&str; 6] = [
            "cameras.rs",
            "cli.rs",
            "mod.rs",
            "options.rs",
            "renders.rs",
            "simulations.rs",
        ];
        let mut first_files = listed_files(&out_first)?;
        let mut second_files = listed_files(&out_second)?;
        first_files.sort();
        second_files.sort();
        assert_eq!(
            first_files, second_files,
            "both runs must produce the same file set"
        );
        assert_eq!(
            first_files, EXPECTED_FILES,
            "generation must publish exactly the six expected modules"
        );

        for name in &first_files {
            let left = fs::read(out_first.join(name))?;
            let right = fs::read(out_second.join(name))?;
            assert_eq!(
                left,
                right,
                "generated file {name} is not byte-identical between the two runs \
                 ({} vs {} bytes)",
                left.len(),
                right.len(),
            );
        }

        Ok(())
    }
}
