# Contributing

Send contributions on [GitHub](https://github.com/po4yka/fuji-cli).

## Workflow

- Fork on GitHub and open a PR.
- Explain the observable change, why it belongs in its source-of-truth layer,
  and which checks you ran.
- Add a regression test before a behavior fix when the failure can be captured
  locally.
- Reproduce the applicable [continuous-integration checks](ci.md) locally; FML
  changes require a workspace build because compilation validates the schema,
  code generator, and generated Rust together.
- Run the project formatter if you have Nix (`nix fmt`); otherwise `cargo fmt`
  covers Rust.
- Keep physical-camera evidence separate from local tests and generated-code
  inspection. A green build does not authorize a camera write.

## Where Things Live

- `fml/` - the CUE schema. **Most contributions go here.** Camera definitions,
  option types, validation rules.
- `crates/codegen/` - the build-time crate that turns FML JSON into Rust. Touch
  this only if you're extending the schema language or changing what the
  generated code looks like.
- `src/lib/ptp/codec.rs` - the `binrw` boundary and PTP-specific strings,
  arrays, and exact-buffer validation. Touch when adding new low-level wire
  types.
- `src/lib/` - the runtime: USB, PTP transport, feature traits, image-side
  helpers. Build-time Rust is generated under Cargo's `OUT_DIR` and included as
  the public `generated` module; it never modifies the source tree.
- `src/main.rs` + `src/cli/` - the CLI front-end.
- `tests/` - cross-layer contract and policy tests.
- `assets/share/` - generated shell completions and man pages shipped by Nix and
  release packages.
- `support/` - checksum-pinned tool installers, release packaging and
  verification scripts, and out-of-band reversing helpers.
- `.github/workflows/` and `flake.nix` - hosted CI, release, security, and Nix
  package contracts.

## Common Contribution Types

| What you want to do | Where |
| --- | --- |
| Add a new camera | [adding-cameras.md](adding-cameras.md) |
| Confirm a `?` in the support table | Open an issue; see [support](../users/support.md) |
| Add or correct a film-simulation alias / variant | `fml/option.cue`; see [fml/options](../fml/options.md) |
| Add a new validation rule | `fml/camera.cue` or `fml/generation.cue`; see [fml/rules](../fml/rules.md) |
| Reverse a render profile | [reversing.md](reversing.md) |
| Extend the codegen language | `crates/codegen/`; see [internals](../internals/README.md) |
| Change CLI grammar, help, or process behavior | `src/cli/`, tests, [CLI reference](../users/reference/cli.md), and generated CLI assets |
| Change release packaging | `support/`, `.github/workflows/release.yml`, and [releasing](releasing.md) |

## Testing

- `cargo test --locked --all-features --all-targets --workspace` covers codegen,
  runtime, CLI grammar, process behavior, packaging assets, and the gated dev
  tooling. It uses fixtures and fakes; it does not drive a camera.
- A workspace build is the FML smoke test: the codegen crate parses the CUE JSON,
  runs semantic analyses, emits Rust, and compiles the result.
- CLI grammar or help changes must keep `assets/share/` synchronized. See the
  regeneration command in [releasing](releasing.md).
- Hardware behavior requires a recorded run against the exact model, firmware,
  and USB mode. Privacy-review verbose output before attaching it to a PR, and
  never promote a support-table or preflight claim from fixture evidence alone.

## Code Style

- Rust: `cargo fmt` with the pinned stable toolchain. Do not panic on
  user input, device responses, I/O, or external-command failures. Reserve
  `expect()` for invariants already guaranteed by the CUE schema.
- CUE: keep field ordering consistent with the existing files (`id` first, then
  `spec`, then `codegen?`). The schema itself enforces most invariants. If a CUE
  error is opaque, ask in the PR.

## Licensing

By contributing you agree your changes ship under the project's existing license
(see [LICENSE](../../LICENSE)).
