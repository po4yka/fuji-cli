# Continuous Integration

GitHub Actions validates every pull request and every push to `main`. All
third-party actions are pinned to full commit hashes, and Dependabot proposes
updates to those pins.

## Required checks

- `CI / Rust` installs checksum-verified CUE 0.16.1 and the native USB
  dependencies, then generates and checks the code, verifies formatting and
  spelling, runs Clippy and rustdoc with warnings denied, tests the Rust code
  and deterministic release packager, builds the workspace, and checks unused
  dependencies using the committed Rust toolchain and lockfile.
- `CI / Platform (macOS x86_64)` and `CI / Platform (Windows x86_64)` build and
  test the executable on the other documented host platforms. These jobs use
  the locked Cargo graph and the same pinned CUE release as Linux.
- `CI / Nix` evaluates and builds the flake, checks formatting, and rejects
  malformed Markdown without modifying `flake.lock`.
- `CI / Docs` rejects broken documentation links or anchors without depending
  on external network availability.
- `Security / cargo-deny` enforces the dependency, license, advisory, source,
  and duplicate-version policy in `deny.toml`.
- `Security / dependency review` rejects pull requests that introduce
  dependencies with moderate-or-higher known vulnerabilities.
- `Security / workflow lint` runs actionlint and zizmor against the workflow
  definitions.

The scheduled security run repeats the dependency and workflow checks daily.
Hardware behaviour remains outside CI: a successful workflow does not prove a
camera feature on a physical device.

## Reproducing the Rust gate

Install the prerequisites from [the installation guide](../users/installation.md),
then run:

```sh
cargo check --locked --all-features --all-targets --workspace --jobs 4
cargo fmt --all --check
typos . .github
cargo clippy --locked --all-features --all-targets --workspace --jobs 4 -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --locked --workspace --all-features --no-deps --jobs 4
cargo test --locked --all-features --all-targets --workspace --jobs 4
cargo build --locked --workspace --jobs 4
cargo udeps --locked --workspace --all-features --all-targets --jobs 4
cargo deny --locked check
markdownlint-cli2 "docs/**/*.md"
lychee --offline --include-fragments=anchor-only --no-progress "docs/**/*.md"
```

On the managed development Mac, wrap each compiler-backed Cargo command with
`build-gate --`. With Nix installed, run
`nix flake check --no-update-lock-file --print-build-logs` as the separate Nix
gate.

## Release rehearsal

The Release workflow can be started manually from `main`. A manual run performs
the complete validation and produces deterministic ZIP archives plus SPDX SBOMs
for Linux x86_64, macOS x86_64, and Windows x86_64. It never creates a GitHub
Release or requests attestations; publication remains restricted to an
annotated `vMAJOR.MINOR.PATCH` tag whose commit belongs to `main`.

After changing packaging, run the manual workflow and inspect all three
`release-assets-*` artifacts before creating a tag.
