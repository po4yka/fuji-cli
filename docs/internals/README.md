# Architecture

The project is split into a **build-time** pipeline that turns FML into Rust and
a **runtime** that consumes the generated code. The runtime is small, generic,
and dispatches everything through traits the codegen implements.

- [Codegen](codegen.md) - what each emitter produces (option types, camera
  registry, simulation/render structs, CLI args).
- [Analyses](analyses.md) - the semantic passes between AST and emission: DNF
  normalization, alias substitution, the presence DAG, the repair walk, and
  inverse transformations.
- [Runtime](runtime.md) - how the generated modules plug into the trait-based
  runtime (`CameraBase`, `Simulation`, `CameraRenderManager`,
  `SimulationSetting`, `ConversionProfileField`).
- [PTP codec migration](binrw-ptp-migration-research.md) - rationale,
  implemented `binrw` architecture, wire-contract mapping, and residual risks.
- [Fuji PTP ecosystem research](fuji-ptp-ecosystem-research.md) - prior art
  (libgphoto2, libfuji), macOS claiming, Rust USB crate landscape, and the
  prioritized improvement candidates derived from them.
- [X-T5 device audit](x-t5-device-audit-2026-08-31.md) - the read-only
  physical-device audit: per-USB-mode surface, the descriptor asymmetry,
  virtual object stores, the property inventory, and open questions.
- [X-T5 device run and macOS transport findings](x-t5-device-audit-2026-09-04.md) -
  a follow-up read-only device run plus host-side analysis of X RAW Studio's
  macOS transport (ImageCaptureCore/`ptpcamerad`), the `discover surface`
  artifact's privacy caveat, and a read-only design for the still/movie
  `0xD18C` namespace question.
- [X-T5 firmware 4.31 static analysis](x-t5-firmware-4.31-static-analysis-2026-09-03.md) -
  the `FWUP0030.DAT` container format, the section LZSS compression, and the
  static PTP surface found in the image: DeviceInfo lists, the 61- and
  264-code property lists, and the per-property descriptor table.

## Build Pipeline

[build.rs](../../build.rs) is the entrypoint. It:

1. Tells cargo to rerun if `fml/` or `crates/codegen/` change.
2. Shells out to `cue export ./fml --out json`. If `cue` isn't on `PATH`, the
   error tells the user how to install it.
3. Passes the JSON to `codegen::generate(json, &generated)` where `generated`
   is under Cargo's build `OUT_DIR`.

`codegen::generate` (see [lib.rs](../../crates/codegen/src/lib.rs)):

```text
options     -> $OUT_DIR/generated/options.rs      (typed option values and codecs)
cameras     -> $OUT_DIR/generated/cameras.rs      (one ZST + registry entry per camera)
simulations -> $OUT_DIR/generated/simulations.rs  (SimulationBase + per-camera structs)
renders     -> $OUT_DIR/generated/renders.rs      (RenderBase + per-camera profiles)
cli         -> $OUT_DIR/generated/cli.rs          (SimulationArgs + RenderArgs + PROP_CODES)
mod         -> $OUT_DIR/generated/mod.rs          (module roots)
```

Output is formatted through `prettyplease` before being written, so any
diagnostic dump of the file is human-readable.

Generation happens in a sibling staging directory. Only a complete set of
formatted modules is published into `OUT_DIR`, so an emitter failure preserves
the last complete output. `cargo build` on a fresh checkout always regenerates
from `fml/`; generated files are artifacts, not source files to edit or commit.

## Why the Analyses Live in Their Own Module

`schema/` is the layer that does anything interesting. The emitters in `common/`
are mostly mechanical: turn a typed AST node into a TokenStream. The cleverness

- DNF normalization, alias substitution, the presence DAG, repair synthesis,
  inverse detection - lives in `schema/` and is unit-tested independently of the
  emitters.

This separation means changing the _language_ (add a new predicate kind, a new
transformation shape) only touches `ast/` + `schema/`, not the emitters.
Changing the _output_ (new trait, new derive) only touches `common/`, not the
analyses.

Read the next two pages in order: [codegen](codegen.md) for the mechanical
layer, then [analyses](analyses.md) for the clever bits.
