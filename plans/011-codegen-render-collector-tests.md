# Plan 011: Unit-test the codegen render CLI collectors

> **Executor instructions**: Follow this plan step by step. Run every verification command and confirm the expected result before moving on. Touch only the file listed as in scope. If any STOP condition occurs, stop and report — do not improvise. Commit your work in the worktree following the plan's git workflow section. SKIP updating `plans/README.md` — your reviewer maintains the index. Before reporting, audit every claim against an actual tool result.
>
> **Drift check (run first)**: `git diff --stat 46f2e5e..HEAD -- crates/codegen/src/cli/render.rs`

## Status

- **Priority**: P3
- **Effort**: S
- **Risk**: LOW
- **Depends on**: none
- **Category**: tests
- **Planned at**: commit `46f2e5e`, 2026-08-30

## Why this matters

`crates/codegen/src/cli/render.rs` decides which render options become fields of the generated `RenderArgs` CLI struct. It walks every camera's render field list and dedupes into `BTreeSet`s. The file has zero tests, unlike its siblings (`cli/simulation.rs` has one; `common/renders/camera.rs` has six). The project's stated smoke test is "the generated Rust has to compile" — but a collector that silently *drops* a field still produces compiling code, just with a missing CLI argument. That failure mode is invisible until someone notices a flag is gone. These are pure functions over in-memory fixtures, so covering them is cheap.

## Current state

`crates/codegen/src/cli/render.rs` (~129 lines) contains two private collectors feeding `build_entries`/`generate_struct`:

```rust
// crates/codegen/src/cli/render.rs:34-53
fn collect_render_option_ids(cameras: &BTreeMap<String, Camera>) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for camera in cameras.values() {
        let Some(render) = camera
            .spec
            .features
            .as_ref()
            .and_then(|f| f.render.as_ref())
        else {
            continue;
        };
        for field in &render.fields {
            if let Field::Ref(r) = field {
                out.insert(r.r#ref.clone());
            }
        }
    }
    out
}
```

`collect_render_inline_ids` (`render.rs:55-70`) is the mirror image: same walk, but matches `Field::Inline(i)` and inserts `i.id.clone()`.

Behaviors worth pinning (they are the contract, and none is covered today):

- cameras with no `features`, or with `features` but no `render`, are skipped rather than panicking;
- the two collectors partition the field list — a `Ref` field never lands in the inline set and vice versa;
- ids repeated across multiple cameras appear exactly once (the `BTreeSet` dedup — the case compile-time smoke testing is least likely to catch);
- ordering is the `BTreeSet`'s deterministic sort, which is what makes generated output stable.

Before writing tests, read `crates/codegen/src/cli/simulation.rs`'s test module: it is the sibling with the closest shape and shows how this crate builds `Camera`/field fixtures in memory. Match that construction style rather than inventing your own helper shape. Also confirm the exact type names and field spellings (`Field::Ref`/`Field::Inline`, `r#ref`, `id`, `spec.features.render.fields`) from the live source — the excerpt above is a guide, not a substitute for reading it.

Conventions: tests live in a `#[cfg(test)] mod tests` at the bottom of the same file; workspace lints deny `unwrap_used`/`panic` in production code but `expect` is normal inside tests; the codegen crate is pure (no I/O, no device).

Environment: `export PATH="/Users/po4yka/.local/share/mise/installs/cue/0.17.1:$PATH"` before any cargo command; `build-gate` is on PATH; use `--jobs 3`.

## Commands you will need

| Purpose | Command | Expected on success |
|---------|---------|---------------------|
| Format check | `cargo fmt --all --check` | exit 0 |
| Focused tests | `build-gate -- cargo test --locked -p codegen --jobs 3` | all pass |
| Clippy | `build-gate -- cargo clippy --locked --all-features --all-targets --workspace --jobs 3 -- -D warnings` | exit 0 |
| Full gate | `build-gate -- cargo test --locked --all-features --all-targets --workspace --jobs 3` | all pass |

## Scope

**In scope** (the only file you should modify):

- `crates/codegen/src/cli/render.rs` (tests module only — no change above `#[cfg(test)]`)

**Out of scope** (do NOT touch):

- The collectors' logic, `build_entries`, `generate_struct`, or any emitted-code shape. If a test reveals a real bug, that is a STOP condition, not a licence to fix it here.
- `crates/codegen/src/cli/simulation.rs` and the `common/` emitters — read them for style, do not edit.
- `fml/**` — no schema changes; fixtures are constructed in memory.

## Git workflow

- Branch: `advisor/011-codegen-render-tests`
- Conventional commit (repo example: `test: make API boundary checks build-layout independent`). Suggested: `test: cover render CLI id collectors`.
- Do NOT push or open a PR.

## Steps

### Step 1: Add fixture-based collector tests

In a new `#[cfg(test)] mod tests` at the bottom of `crates/codegen/src/cli/render.rs`, add tests covering:

1. `collects_ref_ids_and_skips_inline_fields` — one camera whose render fields mix `Ref` and `Inline`; assert `collect_render_option_ids` returns exactly the `Ref` ids and none of the inline ones.
2. `collects_inline_ids_and_skips_ref_fields` — the mirror assertion for `collect_render_inline_ids`.
3. `dedupes_ids_shared_across_cameras` — two cameras that both declare the same `Ref` id (and the same inline id); assert each collector returns that id exactly once, i.e. the returned set has the expected length. This is the case a compile-only smoke test cannot catch.
4. `skips_cameras_without_a_render_feature` — a map containing one camera with `features: None` and one with `features` present but `render: None`, plus one normal camera; assert only the normal camera's ids come back and nothing panics.

Build fixtures with small private helpers in the test module (e.g. `fn camera_with_fields(fields: Vec<Field>) -> Camera`), following the fixture style used in `cli/simulation.rs`'s tests. Assert on whole collections (`BTreeSet::from([...])` equality) rather than probing individual membership — the set identity is the contract.

**Verify**: `build-gate -- cargo test --locked -p codegen --jobs 3` → all pass, with the 4 new tests listed by name in the output.

### Step 2: Format and lint

**Verify**: `cargo fmt --all --check` → exit 0; `build-gate -- cargo clippy --locked --all-features --all-targets --workspace --jobs 3 -- -D warnings` → exit 0.

## Test plan

This plan is the test plan: the 4 cases in Step 1, modelled on `crates/codegen/src/cli/simulation.rs`'s test module. Verification commands above.

## Done criteria

ALL must hold:

- [ ] `rg -c "#\[test\]" crates/codegen/src/cli/render.rs` reports at least 4
- [ ] `build-gate -- cargo test --locked -p codegen --jobs 3` exits 0
- [ ] Nothing above the `#[cfg(test)]` line changed (`git diff 46f2e5e -- crates/codegen/src/cli/render.rs` shows additions only in the test module)
- [ ] `cargo fmt --all --check` exits 0
- [ ] `build-gate -- cargo clippy --locked --all-features --all-targets --workspace --jobs 3 -- -D warnings` exits 0
- [ ] `build-gate -- cargo test --locked --all-features --all-targets --workspace --jobs 3` exits 0
- [ ] `git status` shows only `crates/codegen/src/cli/render.rs` modified
- [ ] `plans/README.md` status row updated (SKIPPED per reviewer override)

## STOP conditions

Stop and report if:

- A new test FAILS against the current collectors — that means a real dedup/filter bug exists; report the case and observed output instead of adjusting the assertion or the production code.
- The types (`Camera`, `Field::Ref`, `Field::Inline`, the `spec.features.render.fields` path) do not match the excerpt — the file drifted; compare against the live source and report.
- Constructing a `Camera` fixture in memory turns out to require deserializing real FML JSON — report how `cli/simulation.rs`'s tests do it instead of reaching for `fml/` files.

## Maintenance notes

- If a third field kind is ever added to `Field`, both collectors and these tests need a case for it; the partition assertions will fail loudly, which is the point.
- These tests pin dedup and skip behavior, not emitted text; if the generated `RenderArgs` shape changes, the `common/renders` tests are the ones that should notice.
