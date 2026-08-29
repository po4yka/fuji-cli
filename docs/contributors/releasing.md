# Releasing

Releases are built and published only from annotated SemVer tags on commits
that are already part of `main`. The version without the `v` prefix must match
both the root Cargo package and the Nix package.

## Before tagging

1. Update the root package version in `Cargo.toml` and refresh `Cargo.lock`.
2. Merge that version change into `main` and wait for all required checks on
   the exact commit to pass.
3. Confirm that the `release` GitHub environment has the intended reviewers and
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

The release workflow reruns the full Rust gate, verifies the version through
Cargo and Nix, and then builds on Ubuntu 22.04 with two compiler jobs. It
publishes:

- `fujicli-vX.Y.Z-x86_64-unknown-linux-gnu.tar.gz`, containing the stripped
  binary and `LICENSE`;
- an SPDX JSON software bill of materials;
- `SHA256SUMS`;
- Sigstore bundles for build provenance and the SBOM.

The Linux binary dynamically links to glibc (Ubuntu 22.04 baseline) and
`libusb-1.0.so.0`. Publication waits on the protected `release` environment,
and the workflow verifies both the release attestation and the attested assets
after upload.

No release is implied by a green CI run. A release exists only after the tag
workflow and its protected publish job complete successfully.
