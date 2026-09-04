# Plan 019: Prove codegen determinism with an emit-twice test

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat f2a78d0..HEAD -- crates/codegen/src/lib.rs crates/codegen/Cargo.toml`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P2
- **Effort**: S
- **Risk**: LOW
- **Depends on**: none
- **Category**: tests
- **Planned at**: commit `f2a78d0`, 2026-09-03

## Why this matters

`AGENTS.md` makes determinism a hard requirement: "Generated output must be
deterministic. Preserve stable ordering, canonical module paths, and
formatting through `prettyplease`". No test verifies this today. The codegen
tests assert substrings and ordering inside generated text (for example
`crates/codegen/src/common/renders/camera.rs:875-893`), but nothing checks
that the same input always produces byte-identical output. A regression in a
`BTreeMap`→`HashMap` swap or a sort key would ship silently, and the first
signal would be a noisy release-asset diff. One emit-twice test closes this.

## Current state

- `crates/codegen/src/lib.rs` — the generation pipeline. Key facts:
  - `pub fn generate(json: &str, out_dir: &Path) -> anyhow::Result<()>` at
    line 20. It parses the FML JSON, generates into a staging directory, then
    `publish()` renames staging onto `out_dir` (lines 126-168).
  - `generate_into` writes exactly six files via `write()`:
    `options.rs`, `cameras.rs`, `simulations.rs`, `renders.rs`, `cli.rs`,
    `mod.rs` (lines 45-63, `write()` at 195-201).
  - Staging directory names embed a process id and an atomic counter
    (`.{name}.tmp-{pid}-{sequence}`, line 81). These names never reach the
    published output, so two `generate()` calls into two different
    directories are directly comparable.
- Existing test harness in the same file, `mod tests` (lines 208-272):
  - `TempDir` helper with a `Drop` that removes the directory
    (lines 218-238). Reuse it.
  - `failed_generation_preserves_the_last_complete_output` (lines 240-271)
    shows the pattern: build a JSON string, call `generate`, assert on the
    filesystem.
- Repo conventions that apply:
  - Compiler-backed Cargo commands on the managed Mac must run under the
    machine-wide `build-gate` with at most four build jobs, per `AGENTS.md`:
    `build-gate -- cargo test --locked -p codegen --jobs 4`. If `build-gate`
    is not installed, run the inner command directly.
  - Never pass `--unlocked`. The committed `Cargo.lock` must not change.
  - `clippy::panic`, `clippy::unwrap_used`, `clippy::todo`,
    `clippy::unimplemented` are denied workspace-wide (root `Cargo.toml`
    lines 15-40). Use `.expect("...")` for invariants, `anyhow` context for
    fallible operations.
  - Commits follow the conventional style seen in `git log` (for example
    `fix(codegen): reject setting ids that are not Rust identifiers`).

## Commands you will need

| Purpose | Command | Expected on success |
|---|---|---|
| Check cue is available | `which cue` | prints a path |
| Export the live schema | `cue export ./fml --out json > /dev/null` | exit 0 |
| Run codegen tests | `build-gate -- cargo test --locked -p codegen --jobs 4` | all pass, including the new test |
| Formatting | `cargo fmt --all --check` | exit 0 |
| Lint | `build-gate -- cargo clippy --locked -p codegen --all-targets --jobs 4 -- -D warnings` | exit 0 |

## Scope

**In scope** (the only files you should modify):
- `crates/codegen/src/lib.rs` (the `#[cfg(test)] mod tests` block only)

**Out of scope** (do NOT touch, even though they look related):
- Any file under `fml/` — the test reads it, it must not change.
- The `build.rs` of the `fujicli` crate — the rerun-if-changed wiring is
  correct already.
- Anything under Cargo's build `OUT_DIR`. Generated Rust is never committed
  or hand-edited.
- `Cargo.lock` — this plan adds no dependencies.

## Git workflow

- Branch: `advisor/019-codegen-determinism-test`
- One commit at the end: `test(codegen): pin byte-identical generation across two runs`
- Do NOT push or open a PR unless the operator instructed it.

## Steps

### Step 1: Add the emit-twice determinism test

In `crates/codegen/src/lib.rs`, inside the existing `#[cfg(test)] mod tests`
block, add a test named `generate_twice_from_live_fml_is_byte_identical`.
Design, in order:

1. Resolve the repository root: `PathBuf::from(env!("CARGO_MANIFEST_DIR"))`
   joined with `../../fml`, then canonicalize. `crates/codegen` sits two
   levels below the root, and `fml/` is the CUE schema directory.
2. Export the schema with CUE, mirroring how the production build does it
   (`build.rs` lines 17-23 of the `fujicli` crate): run
   `cue export ./fml --out json` with `current_dir` set to the repository
   root and capture stdout.
   **Graceful skip**: if spawning `cue` fails with `io::ErrorKind::NotFound`,
   print one line explaining the skip and return `Ok(())`. Rationale: only
   the `fujicli` build needs CUE, so `cargo test -p codegen` alone must stay
   runnable without it. Any other CUE failure (non-zero exit, invalid UTF-8)
   is a real error — propagate it with `anyhow` context. CUE failing on the
   committed schema means the repository is broken.
3. Create two `TempDir` instances and two distinct output subdirectories
   inside them, for example `first/generated` and `second/generated`.
4. Call `generate(&json, out_first)?` and then `generate(&json, out_second)?`
   using the same `String`. `generate` is deterministic against the
   filesystem: each call stages into a fresh temporary directory and renames.
5. Assert the file sets are identical and exactly the six expected names:
   `mod.rs`, `options.rs`, `cameras.rs`, `simulations.rs`, `renders.rs`,
   `cli.rs` (see `generate_into` lines 45-63). Use `std::fs::read_dir`, sort
   the names, and compare.
6. For each of the six files, read both copies with `fs::read` and assert
   byte equality. On mismatch, include the file name and the two byte
   lengths in the assert message so the failure is diagnosable.

Style: match the existing test module — `use super::generate;` is already
imported at line 216; reuse `TempDir`; keep every fallible operation
returning `anyhow::Result` from the test via `?`, with `.expect("...")` only
on the asserts' invariants.

**Verify**: `build-gate -- cargo test --locked -p codegen generate_twice --jobs 4`
→ `test result: ok`, with `generate_twice_from_live_fml_is_byte_identical`
listed as passed (or skipped with the notice when `cue` is absent).

### Step 2: Confirm the gate

**Verify**: `build-gate -- cargo test --locked -p codegen --jobs 4` → all
tests pass, no new warnings.
**Verify**: `cargo fmt --all --check` → exit 0.
**Verify**: `build-gate -- cargo clippy --locked -p codegen --all-targets --jobs 4 -- -D warnings` → exit 0.

## Test plan

- New test `generate_twice_from_live_fml_is_byte_identical` in
  `crates/codegen/src/lib.rs`:
  - happy path: two runs, six files, byte-identical;
  - regression this plan fixes: any nondeterministic emission (map
    iteration order, unstable sort, timestamp) makes the test fail with the
    offending file named;
  - edge case: `cue` absent → explicit skip, not a failure.
- Structural pattern: model the test after
  `failed_generation_preserves_the_last_complete_output` in the same file
  (same TempDir usage, same `anyhow::Result` test signature).
- Verification: `build-gate -- cargo test --locked -p codegen --jobs 4` →
  all pass, including 1 new test.

## Done criteria

Machine-checkable. ALL must hold:

- [ ] `build-gate -- cargo test --locked -p codegen --jobs 4` exits 0 and
      runs `generate_twice_from_live_fml_is_byte_identical` (pass or explicit
      skip when cue is absent)
- [ ] With cue available, the test fails if you temporarily reverse the byte
      comparison — run this once manually to prove the test can fail, then
      restore
- [ ] `cargo fmt --all --check` exits 0
- [ ] `build-gate -- cargo clippy --locked -p codegen --all-targets --jobs 4 -- -D warnings` exits 0
- [ ] `git diff --exit-code Cargo.lock` — the lockfile is unchanged
- [ ] No files outside the in-scope list are modified (`git status`)
- [ ] `plans/README.md` status row for 019 updated

## STOP conditions

Stop and report back (do not improvise) if:

- The code at `crates/codegen/src/lib.rs:20` no longer matches
  `pub fn generate(json: &str, out_dir: &Path) -> anyhow::Result<()>`.
- The published file set is not the six files listed in Current state.
- Making the test pass requires editing anything outside
  `crates/codegen/src/lib.rs` `mod tests`.
- The two runs differ on a file — that is a real nondeterminism bug. Do not
  "fix" it in the test; report the file and the first divergent offset so a
  maintainer can triage the emitter.

## Maintenance notes

- If `generate_into` starts writing additional module files, update the
  six-name assertion — it is deliberately an exact-set check, not a
  superset check, so unexpected extra outputs fail loudly.
- If the FML schema grows an entity type whose emission is unordered, this
  test is the tripwire that catches it at PR time, not at release time.
- Deferred out of scope: a committed byte-golden of generated Rust. The
  substring assertions in the emitter tests already pin content; emit-twice
  pins determinism without a maintenance-heavy golden file.
