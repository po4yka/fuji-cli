# Plan 020: Add cargo-fuzz targets for the PTP wire parsers

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat f2a78d0..HEAD -- src/lib/ptp/descriptor.rs src/lib/ptp/codec.rs src/lib/ptp/structs.rs src/lib/ptp/container.rs src/lib/ptp/mod.rs Cargo.toml`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P2
- **Effort**: M
- **Risk**: MED (new toolchain interaction: libFuzzer + the pinned nightly)
- **Depends on**: none
- **Category**: tests
- **Planned at**: commit `f2a78d0`, 2026-09-03

## Why this matters

The primary untrusted input of this tool is bytes that a camera sends over
USB. The codec and descriptor layers already carry 43 hand-written tests,
including a deterministic "arbitrary bytes never panic" sampler
(`src/lib/ptp/codec.rs:522-536`), but a fixed sampler explores only a thin
slice of the input space. A hang or panic on an unforeseen device response
happens at the worst possible moment: mid-session on physical hardware,
where the codebase's own failure taxonomy calls the outcome "camera state
unknown". Coverage-guided fuzzing is the standard answer, and it is entirely
absent: there is no `fuzz/` directory and no fuzz dependency anywhere.

## Current state

All fuzz entry points live in the `fujicli` library crate
(`/Users/npochaev/GitHub/fuji-cli/src/lib/ptp/`):

- `src/lib/ptp/codec.rs`
  - `pub fn decode_exact<T>(bytes: &[u8]) -> BinResult<T>` at lines 347-360 —
    decodes a little-endian binrw value and rejects trailing bytes.
  - `pub fn encode<T>(value: &T) -> BinResult<Vec<u8>>` at lines 338-345.
  - `PtpString` (lines 37-38, 236-268), `PtpExactString` (40-41, 176-197),
    `PtpArray<T>` (43-44, 74-139) are all `pub`, with allocation budgets
    enforced inside `PtpArray::read_options` (count cap line 86, byte budget
    line 104, remaining-payload check line 119).
- `src/lib/ptp/structs.rs`
  - `pub struct DeviceInfo` (lines 3-40) and `pub struct ObjectInfo`
    (lines 255-285) — binrw-derived, with `read_ptp_string` /
    `read_ptp_array` custom parsers. `ObjectInfo` derives `Default`
    (line 255).
- `src/lib/ptp/container.rs`
  - `pub struct ContainerInfo` (lines 6-45) with `payload_len()` performing
    `checked_sub` on the wire length (lines 39-44).
- `src/lib/ptp/descriptor.rs`
  - `pub(crate) struct DevicePropDesc` with
    `pub(crate) fn decode(bytes: &[u8]) -> anyhow::Result<Self>` at lines
    100-136. This is a hand-rolled parser over fully untrusted bytes —
    the single best fuzz target — but it is **not** reachable from outside
    the crate today. A feature-gated facade is part of this plan.

Toolchain and environment facts:

- `rust-toolchain.toml` pins `nightly-2026-07-01` — libFuzzer needs nightly,
  so no toolchain change is required.
- Building the `fujicli` crate requires a C toolchain, `libusb-1.0` headers,
  and (for the workspace build) CUE on `PATH`. The fuzz crate compiles the
  `fujicli` library, so the same prerequisites apply.
- Repo governance from `AGENTS.md`: adding a dependency requires an explicit
  need and review. Justification for this plan: `libfuzzer-sys` is fuzz-only
  tooling that lives in a separate crate with its own lockfile; it never
  enters the production binary or the committed workspace `Cargo.lock`.
  Compiler-backed Cargo commands on the managed Mac run under `build-gate`
  with at most four jobs; the same ceiling applies to fuzz builds.
- Workspace lints deny `panic`, `unwrap_used`, `todo`, `unimplemented`
  (root `Cargo.toml` lines 15-40). `#![forbid(unsafe_code)]` is set in every
  crate; the fuzz harness code must keep it.

## Commands you will need

| Purpose | Command | Expected on success |
|---|---|---|
| Install cargo-fuzz | `cargo install cargo-fuzz --locked` | exit 0 |
| Compile all targets | `build-gate -- cargo fuzz build --jobs 4` | exit 0 |
| Smoke-run one target | `build-gate -- cargo fuzz run ptp_container_info -- -max_total_time=30` | runs 30s, exits 0, no crashes |
| Regression check | `build-gate -- cargo test --locked -p fujicli --lib ptp:: --jobs 4` | all pass |
| Lockfile untouched | `git diff --exit-code Cargo.lock` | exit 0 |

## Scope

**In scope** (the only files you should create or modify):
- `fuzz/` (new cargo-fuzz crate: `Cargo.toml`, `.gitignore`,
  `fuzz_targets/*.rs`)
- `Cargo.toml` of the root crate — add the feature `fuzzing = []` only
- `src/lib/ptp/mod.rs` — one feature-gated `pub use` line only
- Root `.gitignore` — one entry for `fuzz/artifacts` if the fuzz-local
  `.gitignore` does not cover it

**Out of scope** (do NOT touch, even though they look related):
- The workspace `Cargo.lock` — the fuzz crate is a standalone workspace with
  its own lockfile. If your change would touch the root `Cargo.lock`, you
  have wired something wrong: STOP.
- CI workflows — running fuzzing in CI is a deliberate follow-up, not part
  of this plan.
- The descriptor parser logic itself. This plan adds coverage, not fixes. If
  a fuzzer finds a bug, that is a finding to report, not to patch here.
- `fml/` and generated code.

## Git workflow

- Branch: `advisor/020-ptp-parser-fuzz-targets`
- Commit style (conventional, per `git log`): `test(ptp): add cargo-fuzz targets for wire parsers`
- Do NOT push or open a PR unless the operator instructed it.

## Steps

### Step 1: Scaffold the fuzz crate

Create `fuzz/Cargo.toml`:

```toml
[package]
name = "fujicli-fuzz"
version = "0.0.0"
publish = false
edition = "2024"

[package.metadata]
cargo-fuzz = true

[dependencies]
libfuzzer-sys = "0.4"

[dependencies.fujicli]
path = ".."
features = ["fuzzing"]

# Keep the fuzz crate out of the parent workspace.
[workspace]

[[bin]]
name = "ptp_descriptor"
path = "fuzz_targets/ptp_descriptor.rs"
test = false
doc = false
bench = false

[[bin]]
name = "ptp_object_info"
path = "fuzz_targets/ptp_object_info.rs"
test = false
doc = false
bench = false

[[bin]]
name = "ptp_device_info"
path = "fuzz_targets/ptp_device_info.rs"
test = false
doc = false
bench = false

[[bin]]
name = "ptp_container_info"
path = "fuzz_targets/ptp_container_info.rs"
test = false
doc = false
bench = false
```

Create `fuzz/.gitignore` with `artifacts/`, `coverage/`, and `target/`.
Create the empty `fuzz_targets/` directory; the four target files follow.

**Verify**: `cargo metadata --no-deps --format-version 1 > /dev/null` from
the repository root → exit 0 (the parent workspace did not absorb `fuzz/`).

### Step 2: Add the feature-gated descriptor facade

1. In the root `Cargo.toml`, extend the `[features]` section (lines 61-64)
   with `fuzzing = []`. Do not add it to `default`.
2. In `src/lib/ptp/mod.rs`, next to the existing `mod descriptor;`
   declaration, add exactly one gated re-export:

   ```rust
   #[cfg(feature = "fuzzing")]
   pub use descriptor::DevicePropDesc;
   ```

   Read the surrounding module declarations first and place the line to
   match the file's ordering conventions. The feature is not in any
   distributable build path (distributables use default features), so the
   public surface of shipped binaries is unchanged.

**Verify**: `build-gate -- cargo check --locked --workspace --jobs 4` → exit
0 with the feature off.
**Verify**: `git diff --exit-code Cargo.lock` → exit 0.

### Step 3: Write the four targets

All targets follow one shape: `#![no_main]`, `use libfuzzer_sys::fuzz_target`,
one call per entry point, no panics, no unwraps. Match this template:

```rust
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // A camera answer must never panic, hang, or abort the process.
    let _ = fujicli::ptp::DevicePropDesc::decode(data);
});
```

- `ptp_descriptor.rs` — as above. This is the hand-rolled parser; keep it
  as the first target.
- `ptp_object_info.rs` —
  `decode_exact::<ObjectInfo>(data)`. On `Ok(value)`, encode with
  `encode(&value)` and require `Ok`; if re-encoding succeeded, decode the
  re-encoded bytes and require the second decode to equal the first. This
  is the round-trip oracle; wire identity of the bytes themselves is
  asserted by unit tests, not here.
- `ptp_device_info.rs` — same shape as `ptp_object_info.rs` with
  `DeviceInfo`.
- `ptp_container_info.rs` — `decode_exact::<ContainerInfo>(data)`. On
  `Ok(info)`, `info.payload_len()` must be `Ok` whenever
  `info.total_len >= ContainerInfo::SIZE` and `Err` otherwise; assert that
  correspondence explicitly instead of dropping the result.

Use fully qualified paths (`fujicli::ptp::codec::decode_exact` and so on) so
each target file is readable without imports.

**Verify**: `build-gate -- cargo fuzz build --jobs 4` → exit 0. If libFuzzer
fails to compile against the pinned nightly, see STOP conditions.

### Step 4: Smoke-run and document

1. Run one target briefly:
   `build-gate -- cargo fuzz run ptp_container_info -- -max_total_time=30`
   → exits 0, no crash artifacts. Capture the command in the fuzz README.
2. Create `fuzz/README.md` (short, simple English): purpose, the four
   targets, the run command with `-max_total_time`, where artifacts land,
   and the rule that a found crash becomes a regression test in the owning
   module plus a fix in a separate commit. Keep it under 60 lines.
3. Add `fuzz/artifacts/` to `.gitignore` handling as described in Scope.

**Verify**: `git status --porcelain fuzz/` shows only intended files;
artifacts are ignored.

### Step 5: Confirm the workspace gate

**Verify**: `build-gate -- cargo test --locked -p fujicli --lib ptp:: --jobs 4`
→ all pass.
**Verify**: `cargo fmt --all --check` → exit 0.
**Verify**: `build-gate -- cargo clippy --locked --all-features --all-targets --workspace --jobs 4 -- -D warnings`
→ exit 0 (this also compiles the feature-gated facade under `--all-features`).

## Test plan

- No new unit tests in the workspace crates: the fuzz targets ARE the test
  surface. The oracle logic lives in the targets (decode-must-not-panic;
  round-trip equality; payload_len correspondence).
- Existing tests that must keep passing:
  `build-gate -- cargo test --locked -p fujicli --lib ptp:: --jobs 4`.
- Manual fuzz evidence to record in the handoff: target name, wall time,
  number of executions printed by libFuzzer, and "no crashes". Keep the
  artifact directory out of any commit.

## Done criteria

Machine-checkable. ALL must hold:

- [ ] `build-gate -- cargo fuzz build --jobs 4` exits 0
- [ ] `build-gate -- cargo fuzz run ptp_container_info -- -max_total_time=30`
      exits 0 with no crash artifacts
- [ ] `build-gate -- cargo test --locked -p fujicli --lib ptp:: --jobs 4`
      exits 0
- [ ] `git diff --exit-code Cargo.lock` — the workspace lockfile is unchanged
- [ ] `git diff` shows no changes to the descriptor, codec, structs, or
      container parser logic (only the one gated `pub use` line and the
      feature entry)
- [ ] `cargo fmt --all --check` and the workspace clippy command exit 0
- [ ] `plans/README.md` status row for 020 updated

## STOP conditions

Stop and report back (do not improvise) if:

- `libfuzzer-sys` does not compile against `nightly-2026-07-01` after trying
  the latest 0.4.x patch release. Report the exact compiler error; do not
  bump the toolchain or switch to another fuzzing engine.
- Making a target compile requires changing any parser's visibility beyond
  the single gated `pub use` line, or requires touching the root
  `Cargo.lock`.
- A fuzz run finds a crash, hang, or OOM. Preserve the artifact path, write
  down the target and the reproducing input hash, and report. Fixing the
  parser is a separate decision, not part of this plan.
- The round-trip oracle in `ptp_object_info.rs` or `ptp_device_info.rs`
  fires (decode → encode → decode inequality). That is a real finding —
  report it with the artifact; do not weaken the oracle to make it pass.

## Maintenance notes

- New wire types that parse device bytes should get a fuzz target the same
  release they land. The README lists this as the standing rule.
- CI integration (a time-boxed fuzz job, `continue-on-error`, artifact
  upload) is the natural follow-up; it was deliberately deferred so this
  plan stays local-only.
- If the `fujicli` crate ever removes the `fuzzing` feature, the fuzz crate
  breaks loudly at build time — that is intended.
