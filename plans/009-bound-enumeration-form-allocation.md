# Plan 009: Bound the Enumeration-form allocation in the PTP descriptor decoder

> **Executor instructions**: Follow this plan step by step. Run every verification command and confirm the expected result before moving on. Touch only the file listed as in scope. If any STOP condition occurs, stop and report — do not improvise. Commit your work in the worktree following the plan's git workflow section. SKIP updating `plans/README.md` — your reviewer maintains the index. Before reporting, audit every claim against an actual tool result.
>
> **Drift check (run first)**: `git diff --stat 46f2e5e..HEAD -- src/lib/ptp/descriptor.rs`

## Status

- **Priority**: P2
- **Effort**: S
- **Risk**: LOW
- **Depends on**: none (mirrors the fix already merged as plan 003)
- **Category**: security
- **Planned at**: commit `46f2e5e`, 2026-08-30

## Why this matters

Plan 003 fixed `read_array` so a device-supplied element count can no longer drive a speculative allocation before the corresponding bytes are known to exist. The Enumeration-form branch of the same decoder still has the original pattern: it reads a `u16` count and immediately calls `Vec::with_capacity(count)`. The ceiling is far smaller (65,535 elements, so a few MB rather than tens), which is why it was not part of plan 003 — but it is the same allocate-before-validate shape in the same file, and `Vec::with_capacity` aborts the process on allocation failure instead of returning a recoverable error. This makes the decoder internally consistent.

## Current state

`src/lib/ptp/descriptor.rs` decodes `GetDevicePropDesc` responses from a `Cursor<&[u8]>` over the already-received payload. The remaining gap is in `DevicePropDesc::decode`'s form match:

```rust
// src/lib/ptp/descriptor.rs:119-126 (inside the `2 =>` Enumeration arm)
            2 => {
                let count = usize::from(read_u16(&mut reader)?);
                let mut values = Vec::with_capacity(count);
                for _ in 0..count {
                    values.push(read_value(&mut reader, data_type)?);
                }
                DevicePropForm::Enumeration(values)
            }
```

The exemplar to mirror is the already-fixed `read_array` in the same file (landed by plan 003, commit `7ddb02f`): it computes the element wire size, multiplies with `checked_mul`, compares against the cursor's remaining bytes, and uses `try_reserve_exact` instead of `with_capacity`. Read that function before editing — it defines the pattern, including the private `wire_size(element_type) -> Option<usize>` helper it already added, which this plan reuses as-is.

Key differences to respect here:

- The count is a `u16` (max 65,535), read via `read_u16`, not a `u32`.
- The element type is the descriptor's own `data_type` (already in scope in this match arm), not a separate parameter.
- `wire_size` returns `None` for array/string types; the existing `read_array` errors in that case. Do the same here rather than panicking.

Conventions: `anyhow::Result` with `ensure!`/`bail!` and contextual messages; no panics on device input; workspace lints deny `unwrap_used`/`panic`. Tests live in the `#[cfg(test)] mod tests` at the bottom of the same file, building inline byte fixtures and calling `DevicePropDesc::decode(&bytes)` — see `descriptor_decodes_writable_scalar_without_form` and the enumeration tests already present.

Environment: `cue` must be on PATH — `export PATH="/Users/po4yka/.local/share/mise/installs/cue/0.17.1:$PATH"`. `build-gate` is on PATH; the machine-wide cargo wrapper caps jobs at 3, so use `--jobs 3`.

## Commands you will need

| Purpose | Command | Expected on success |
|---------|---------|---------------------|
| Format check | `cargo fmt --all --check` | exit 0 |
| Focused tests | `build-gate -- cargo test --locked -p fujicli --lib ptp:: --jobs 3` | all pass |
| Clippy | `build-gate -- cargo clippy --locked --all-features --all-targets --workspace --jobs 3 -- -D warnings` | exit 0 |
| Full gate | `build-gate -- cargo test --locked --all-features --all-targets --workspace --jobs 3` | all pass |

## Scope

**In scope** (the only file you should modify):

- `src/lib/ptp/descriptor.rs`

**Out of scope** (do NOT touch):

- `read_array` and the `wire_size` helper — already correct; reuse `wire_size`, do not restructure it.
- `read_string`'s `Vec::with_capacity` — bounded by a `u8` length (max 254 units); leave it.
- `src/lib/ptp/codec.rs` — unrelated.
- Any change to what a well-formed descriptor decodes to: valid enumerations that fit their payload must decode byte-identically.

## Git workflow

- Branch: `advisor/009-bound-enumeration-allocation`
- Conventional commit, imperative, under 72 chars. Suggested: `fix: bound enumeration form allocation`.
- Do NOT push or open a PR.

## Steps

### Step 1: Apply the plan-003 pattern to the Enumeration arm

In the `2 =>` arm, after reading the count and before allocating:

1. resolve the element wire size with the existing `wire_size(data_type)` helper; on `None`, return an error (message: `"PTP device property enumeration element type has no fixed wire size"`);
2. compute `required_bytes = count.checked_mul(element_size)`, erroring on overflow (message: `"PTP device property enumeration size overflows"`);
3. compute remaining bytes as `reader.get_ref().len().saturating_sub(usize::try_from(reader.position())?)` and error if `required_bytes > remaining` (message: `"PTP device property enumeration is larger than its payload"`);
4. replace `Vec::with_capacity(count)` with `Vec::new()` + `values.try_reserve_exact(count)` mapped to an `anyhow` error (message pattern: `"failed to reserve PTP device property enumeration: {error}"`).

The decode loop and the `DevicePropForm::Enumeration(values)` result stay unchanged. If the arm's body grows awkward inline, extracting it into a private `read_enumeration(&mut reader, data_type)` function beside `read_array` is acceptable and preferred — mirror `read_array`'s signature style.

**Verify**: `build-gate -- cargo build --locked --workspace --jobs 3` → exit 0.

### Step 2: Add rejection and acceptance tests

In the file's tests module, following the inline-fixture style already there:

- `enumeration_count_exceeding_payload_is_rejected_before_allocation`: a descriptor with form flag `2` whose enumeration count declares the maximum `u16` (65,535) but supplies no element bytes → `DevicePropDesc::decode` returns `Err` whose message contains `"larger than its payload"`.
- `enumeration_matching_its_payload_still_decodes`: a small valid enumeration (e.g. `UInt16` data type, 2 elements, exactly 4 element bytes) decodes to the expected `DevicePropForm::Enumeration` — proves the new bound does not over-reject.

Every existing test in the module must keep passing unchanged.

**Verify**: `build-gate -- cargo test --locked -p fujicli --lib ptp:: --jobs 3` → all pass, including the 2 new tests.

## Test plan

- New tests: the two cases in Step 2, in `src/lib/ptp/descriptor.rs`'s tests module.
- Existing coverage to keep green: all `ptp::` unit tests.
- Verification: the focused test command above, then the full gate.

## Done criteria

ALL must hold:

- [ ] `rg -n "with_capacity" src/lib/ptp/descriptor.rs` matches only the bounded `read_string` site (nothing in the Enumeration path and nothing in `read_array`)
- [ ] `cargo fmt --all --check` exits 0
- [ ] `build-gate -- cargo clippy --locked --all-features --all-targets --workspace --jobs 3 -- -D warnings` exits 0
- [ ] `build-gate -- cargo test --locked --all-features --all-targets --workspace --jobs 3` exits 0; both new tests exist and pass
- [ ] `git status` shows only `src/lib/ptp/descriptor.rs` modified
- [ ] `plans/README.md` status row updated (SKIPPED per reviewer override)

## STOP conditions

Stop and report if:

- The Enumeration arm no longer matches the excerpt (drift since `46f2e5e`).
- `wire_size` does not exist in the file, or does not cover the scalar types — that would mean plan 003's fix is not present as expected; report rather than reimplementing it.
- An existing test fails because it decoded an enumeration whose declared count exceeds its payload — report the test name instead of relaxing the bound.

## Maintenance notes

- After this lands, both count-driven allocation sites in the descriptor decoder share one pattern; a reviewer should reject any new `Vec::with_capacity` driven by device-supplied counts in this file.
- If a new `DevicePropDataType` variant is added, `wire_size` must be extended; the error on `None` makes that omission fail loudly.
