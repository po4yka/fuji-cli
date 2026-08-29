#![forbid(unsafe_code)]

use std::{fs, path::PathBuf, process::Command, str};

use anyhow::{Context, bail};

fn main() -> anyhow::Result<()> {
    println!("cargo:rerun-if-changed=fml/");
    println!("cargo:rerun-if-changed=crates/codegen/");

    let manifest = PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR must be set by cargo"),
    );
    let out_dir = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR must be set by cargo"));
    let generated = out_dir.join("generated");

    let cue = Command::new("cue")
        .args(["export", "./fml", "--out", "json"])
        .current_dir(&manifest)
        .output()
        .map_err(|err| {
            anyhow::anyhow!("failed to invoke `cue export`: {err}. Install CUE (https://cuelang.org) or run inside `nix develop`.")
        })?;

    if !cue.status.success() {
        bail!(
            "`cue export ./fml --out json` failed:\n{}",
            String::from_utf8_lossy(&cue.stderr),
        );
    }

    let json = str::from_utf8(&cue.stdout)?;
    codegen::generate(json, &generated)?;

    let generated_root = generated.join("mod.rs");
    let module = format!("#[path = {generated_root:?}]\npub mod generated;\n");
    let module_path = out_dir.join("generated_module.rs");
    fs::write(&module_path, module)
        .with_context(|| format!("writing generated module shim {}", module_path.display()))?;

    Ok(())
}
