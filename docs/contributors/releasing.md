# Releasing

Releases are built and published only from annotated SemVer tags on commits
that are already part of `main`. The version without the `v` prefix must match
both the root Cargo package and the Nix package.

## Before tagging

1. Move the relevant `Unreleased` entries in [`CHANGELOG.md`](../../CHANGELOG.md)
   into a dated version section and update its comparison links.
2. Update the root package version in `Cargo.toml` and refresh `Cargo.lock`.
3. Merge that version change into `main` and wait for all required checks on
   the exact commit to pass.
4. Confirm that the `release` GitHub environment has the intended reviewers and
   permits only tags matching `v*`.

Create and push an annotated tag only after those conditions hold:

```sh
git switch main
git pull --ff-only
git tag -s v0.2.0 -m "fujicli v0.2.0"
git push origin v0.2.0
```

Use the actual package version in place of `0.2.0`. A lightweight tag, a
non-SemVer tag, a version mismatch, or a tag outside `main` fails before any
release asset is published.

## Pipeline and artifacts

The release workflow verifies the version through Cargo and Nix, then reruns
Cargo check, formatting, Clippy, rustdoc, all workspace tests, and the release
packager tests. Each supported target is subsequently built with two compiler
jobs. The workflow publishes:

- deterministic ZIP archives for Linux x86_64, macOS x86_64, and Windows
  x86_64, containing the stripped binary, `LICENSE`, shell completions for
  Bash, Zsh, Fish, and PowerShell, and section 1 man pages for the complete
  command hierarchy;
- an SPDX JSON software bill of materials;
- `SHA256SUMS`;
- Sigstore bundles for build provenance and the SBOM.

Completion files and man pages live below the archive's `share/` directory in
the same package-relative paths used by the Nix package. The Linux binary
dynamically links to glibc (Ubuntu 22.04 baseline) and
`libusb-1.0.so.0`. Publication waits on the protected `release` environment,
and the workflow verifies both the release attestation and the attested assets
after upload.

The completion and man assets are generated from the production clap command
model and checked byte-for-byte by the Rust tests. After an intentional CLI
grammar or help change, regenerate and review them with:

```sh
BLESS_CLI_ASSETS=1 cargo test --locked --bin fujicli \
  packaged_cli_assets_match_the_command_model
```

No release is implied by a green CI run. A release exists only after the tag
workflow and its protected publish job complete successfully.
