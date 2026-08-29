# Contributing

Send contributions on [GitHub](https://github.com/po4yka/fuji-cli).

## Workflow

No strict process; just be sane.

- Fork on GitHub and open a PR.
- A short note in the description (what, why) helps.
- Reproduce the applicable [continuous-integration checks](ci.md) locally.
- Run the project formatter if you have Nix (`nix fmt`); otherwise `cargo fmt`
  covers Rust.

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
- `support/` - out-of-band scripts used during reversing.

## Common Contribution Types

| What you want to do                              | Where                                                                      |
| ------------------------------------------------ | -------------------------------------------------------------------------- |
| Add a new camera                                 | [adding-cameras.md](adding-cameras.md)                                     |
| Confirm a `?` in the support table               | Open an issue; see [support](../users/support.md)                          |
| Add or correct a film-simulation alias / variant | `fml/option.cue`; see [fml/options](../fml/options.md)                     |
| Add a new validation rule                        | `fml/camera.cue` or `fml/generation.cue`; see [fml/rules](../fml/rules.md) |
| Reverse a render profile                         | [reversing.md](reversing.md)                                               |
| Extend the codegen language                      | `crates/codegen/`; see [internals](../internals/README.md)                 |

## Testing

- `cargo test --workspace` runs the codegen unit tests (DNF, alias, presence
  DAG, repair output, predicate compiler).
- The build itself is a smoke test for any FML change: the codegen crate parses
  the JSON, runs the analyses, and the resulting Rust has to compile.
- There is no integration test that drives a real camera; for now, manual `-vvv`
  runs against a physical X-T5 are the gold standard. If you confirm a feature
  works on another body, privacy-review the redacted `-vvv` diagnostics before
  attaching them to the PR.

## Code Style

- Rust: `cargo fmt` (uses [rustfmt.toml](../../rustfmt.toml)). No panicking
  outside `expect()` calls on invariants the CUE schema enforces.
- CUE: keep field ordering consistent with the existing files (`id` first, then
  `spec`, then `codegen?`). The schema itself enforces most invariants. If a CUE
  error is opaque, ask in the PR.

## Licensing

By contributing you agree your changes ship under the project's existing license
(see [LICENSE](../../LICENSE)).
