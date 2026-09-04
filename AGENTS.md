# Fuji CLI Agent Guide

## Scope

These instructions apply to the entire repository. Follow higher-priority user
and platform instructions first, then this file. Make the smallest coherent
change that delivers the requested behavior, preserve unrelated work, and
report validation and any missing device evidence separately.

## Project model

`fujicli` is an edition-2024 Rust CLI for Fujifilm cameras. It has two distinct
halves:

1. At build time, `build.rs` runs `cue export ./fml --out json` and passes the
   result to `crates/codegen`.
2. At runtime, the CLI and library use the generated camera, option,
   simulation, and render types to communicate over PTP through `rusb`.

Keep that boundary intact. The schema and generators should own camera-specific
knowledge; the runtime should remain generic and dispatch through its existing
traits.

## Sources of truth and change routing

- `fml/` is the source of truth for cameras, generations, options, validation
  rules, transformations, and feature declarations. Most camera support changes
  belong here.
- `crates/codegen/` owns FML parsing, semantic analyses, and emitted Rust. Change
  it only when extending the schema language or changing generated behavior.
- `src/lib/ptp/codec.rs` owns the `binrw` boundary and the PTP-specific string,
  array, and exact-buffer codecs.
- `src/lib/` owns USB/PTP transport, feature traits, input handling, and generic
  runtime dispatch.
- `src/cli/` and `src/main.rs` own the user-facing command-line contract.
- `docs/` documents user-visible support and the FML/codegen/runtime design.

Read the nearby documentation before changing a subsystem. Start with
`docs/contributors/README.md`; for schema or generator work, also read
`docs/internals/README.md` and the relevant FML or internals page.

## Generated code

Never edit or commit generated Rust under Cargo's build `OUT_DIR`. Make the
change in `fml/` or `crates/codegen/`, regenerate through Cargo, and inspect the
generated output locally when it helps review the result. Builds must not write
generated modules into the source tree.

Generated output must be deterministic. Preserve stable ordering, canonical
module paths, and formatting through `prettyplease`; do not patch generated text
after emission.

## Environment and dependencies

- Run commands from the workspace root containing `Cargo.toml`.
- `cue` must be on `PATH` for compiler-backed Cargo commands. `nix develop`
  provides the intended Rust/CUE environment.
- Source builds also require a C toolchain and `libusb-1.0` headers.
- Rust 1.98.1 stable, `rustfmt`, and `clippy` are pinned in `rust-toolchain.toml`.
  Change the toolchain deliberately and validate the full workspace gate.
- `Cargo.lock` is committed application state. Pass `--locked` to routine Cargo
  commands. Change the lockfile only for a deliberate dependency update and
  review the resulting dependency diff.
- Prefer existing dependencies. Adding a production dependency requires an
  explicit need plus review of maintenance, security, license, and platform
  impact.

## Implementation rules

- Add a regression test before a behavior fix when the failure can be captured
  locally. Keep the red-green-refactor loop scoped to the owning crate.
- Fix behavior at its source. Do not duplicate FML facts in runtime code, patch
  generated files, weaken validation, or special-case one camera in a generic
  path when the schema can express the distinction.
- Preserve the existing separation between AST parsing, semantic analysis in
  `crates/codegen/src/schema/`, mechanical emission in
  `crates/codegen/src/common/`, and runtime traits.
- Treat CLI arguments, stdout, stderr, JSON shape, and exit status as public
  contracts. Update user documentation and tests when they change.
- Use `anyhow::Result` and contextual errors at application boundaries. Do not
  panic on user input, device responses, I/O, or external command failures.
  `expect` is reserved for invariants already guaranteed by the CUE schema.
- Format Rust with the pinned stable `rustfmt`. Keep CUE field ordering
  consistent with neighboring definitions: `id`, then `spec`, then optional
  `codegen` data.

## Camera and wire-protocol integrity

- Do not invent USB IDs, PTP property codes, render profile codes, field order,
  padding, supported values, or camera capabilities. Base them on captured
  traffic, vendor evidence, or an explicitly identified physical-device run.
- Declare a camera feature only after the corresponding reverse/probe workflow
  succeeds. Pin allowed values and ranges per camera so future global option
  additions do not silently expand older-camera claims.
- In `docs/users/support.md`, `Y` means personally verified on that physical
  model. A schema build, fixture, another camera, or generated code is not
  physical-device proof.
- Keep local build/test evidence, generated-code inspection, and physical-camera
  evidence distinct in the handoff. For a device run, record the model,
  firmware when known, command, and observed result; attach privacy-reviewed
  `-vvv` output when appropriate.

## Validation

Run the smallest relevant check first, then broaden according to the change.
Compiler-backed Cargo commands on this Mac must use the machine-wide
`build-gate`; acquire it once around the top-level command and use at most four
build jobs. Formatting and metadata inspection are lightweight and do not need
the gate.

Targeted checks:

```sh
cargo fmt --all --check
build-gate -- cargo test --locked -p codegen --jobs 4
build-gate -- cargo test --locked -p fujicli --lib ptp:: --jobs 4
build-gate -- cargo build --locked --workspace --jobs 4
```

- FML changes require a workspace build because compilation is the schema,
  codegen, and generated-Rust smoke test.
- Codegen analysis changes require the corresponding `codegen` unit tests plus
  a workspace build.
- PTP codec changes require the focused `fujicli` PTP tests, relevant codegen
  emitter tests, and a workspace build.
- CLI behavior changes require focused argument/output/error-path coverage.
- Hardware behavior requires a real-device run when access is available; lack
  of one must be reported, not replaced by a local claim.

Before completion, run the repository-equivalent full local gate when the
environment supports it:

```sh
cargo fmt --all --check
build-gate -- cargo check --locked --all-features --all-targets --workspace --jobs 4
build-gate -- cargo clippy --locked --all-features --all-targets --workspace --jobs 4 -- -D warnings
build-gate -- cargo test --locked --all-features --all-targets --workspace --jobs 4
```

For all-features release-profile validation, keep LTO compilation at two jobs:

```sh
build-gate -- cargo build --locked --release --all-features --all-targets --workspace --jobs 2
```

Distributable binaries must use default features so the `reverse-tools` escape
hatch is not packaged:

```sh
build-gate -- cargo build --locked --release --workspace --jobs 2
```

If `build-gate` is unavailable outside the managed development Mac, run the
inner Cargo command directly and retain the same job ceilings. Do not claim a
check passed unless its output was observed.

## Project skills

The canonical project skills live in `.agents/skills/`. Claude Code discovers
their symlink mirrors in `.claude/skills/`, while Codex discovers the universal
catalog directly. Do not hand-edit the mirrors. Keep `skills-lock.json` in sync
and manage the selected set with the `skills` CLI from this project directory.

## Completion checklist

- The change is made in the correct source-of-truth layer.
- Relevant regression and failure-path tests exist and pass.
- Generated output was regenerated only through the build and was not committed.
- Formatting, clippy, tests, and the affected build were run as required.
- User docs and the support table match only verified behavior.
- Local, generated, CI, and physical-device evidence are described precisely.
- The final diff contains no unrelated edits, secrets, build output, or stale
  generated files.
