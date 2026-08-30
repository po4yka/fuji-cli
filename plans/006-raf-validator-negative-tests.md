# Plan 006: Cover every rejection branch of the X-T5 RAF validator with tests

> **Executor instructions**: Follow this plan step by step. Run every verification command and confirm the expected result before moving to the next step. If anything in the "STOP conditions" section occurs, stop and report — do not improvise. When done, update the status row for this plan in `plans/README.md`.
>
> **Drift check (run first)**: `git diff --stat 4c6fd8c..HEAD -- src/lib/features/render/raf.rs`
> If the file changed since this plan was written, compare the "Current state" excerpts against the live code before proceeding; on a mismatch, treat it as a STOP condition.

## Status

- **Priority**: P2
- **Effort**: S
- **Risk**: LOW
- **Depends on**: none
- **Category**: tests
- **Planned at**: commit `4c6fd8c`, 2026-08-30

## Why this matters

`validate_xt5_raf` is the first gate on user-supplied `.raf` files before RAW-conversion camera writes. Its test module covers the signature, wrong-model, one out-of-bounds region, and missing-RAW-region cases, but four rejection branches are untested: the incomplete offset/length pair, the header-overlap check, the `u32` end-offset overflow, and the truncated-directory error in `read_be_u32`. These are exactly the branches that exist to reject malformed untrusted input; a logic inversion there would currently pass the suite. This plan is test-only — no production code changes.

## Current state

- `src/lib/features/render/raf.rs` — the whole file is ~135 lines; validator plus its tests module. The untested branches:

```rust
// raf.rs:44-59 (inside validate_region)
    anyhow::ensure!(
        offset != 0 && length != 0,
        "RAF {name} region has an incomplete offset/length pair"
    );
    anyhow::ensure!(
        usize::try_from(offset)? >= RAF_MIN_HEADER_BYTES,
        "RAF {name} region overlaps the RAF header"
    );
    let end = offset
        .checked_add(length)
        .ok_or_else(|| anyhow::anyhow!("RAF {name} region overflows"))?;
```

```rust
// raf.rs:63-69
fn read_be_u32(data: &[u8], offset: usize) -> anyhow::Result<u32> {
    let bytes = data
        .get(offset..offset + 4)
        .ok_or_else(|| anyhow::anyhow!("RAF offset directory is truncated"))?;
    // ...
}
```

- Important structural fact: `validate_xt5_raf` checks `data.len() >= RAF_MIN_HEADER_BYTES` (0x6c = 108) at `raf.rs:22-25` BEFORE calling `validate_region` for offsets 0x54/0x5c/0x64, so the "RAF offset directory is truncated" branch of `read_be_u32` is unreachable through the public entry point — it is defensive. It must therefore be tested by calling the private helper directly from the in-file tests module (which already has access), not through `validate_xt5_raf`.
- Existing test fixtures and style to reuse (same file, `raf.rs:75-93`):

```rust
fn minimal_raf() -> Vec<u8> {
    let mut data = vec![0; 108];
    data[..RAF_SIGNATURE.len()].copy_from_slice(RAF_SIGNATURE);
    data[0x1c..0x20].copy_from_slice(b"X-T5");
    data
}
```

  Region fields: preview at 0x54 (offset) / 0x58 (length), metadata at 0x5c/0x60, RAW image at 0x64/0x68, all big-endian `u32`. Tests assert with `expect_err(...)` + `error.to_string().contains(...)` — match that style.
- `build-gate` wraps compiler-backed cargo commands on the maintainer's Mac; if unavailable, run the inner command directly with the same `--jobs` ceiling.

## Commands you will need

| Purpose | Command | Expected on success |
|---------|---------|---------------------|
| Format check | `cargo fmt --all --check` | exit 0 |
| Focused tests | `build-gate -- cargo test --locked -p fujicli --lib features::render::raf --jobs 4` | all pass |
| Clippy | `build-gate -- cargo clippy --locked --all-features --all-targets --workspace --jobs 4 -- -D warnings` | exit 0 |

## Scope

**In scope** (the only file you should modify):

- `src/lib/features/render/raf.rs` (tests module only — no change above `#[cfg(test)]`)

**Out of scope**:

- Any change to `validate_xt5_raf`/`validate_region`/`read_be_u32` logic. If a new test reveals a real bug in a branch, that is a STOP condition, not a license to fix it here.
- `src/cli/image/mod.rs` and the render manager — not touched by test-only work.

## Git workflow

- Branch: `advisor/006-raf-negative-tests`
- Conventional commits (repo example: `test: make API boundary checks build-layout independent`). Suggested: `test: cover RAF validator rejection branches`.
- Do NOT push or open a PR unless the operator instructed it.

## Steps

### Step 1: Add the four missing rejection tests

In the existing tests module of `raf.rs`, add:

1. `rejects_raf_region_with_an_incomplete_offset_length_pair` — start from `minimal_raf()`, set preview offset (0x54..0x58) to `108u32` and preview length (0x58..0x5c) to `0u32`; expect err containing `"incomplete offset/length pair"`. (Offset nonzero, length zero — hits `raf.rs:44-47`.)
2. `rejects_raf_region_overlapping_the_header` — start from `minimal_valid_raf()` (so the required RAW region is present), set preview offset to `4u32` and preview length to `8u32`; expect err containing `"overlaps the RAF header"`.
3. `rejects_raf_region_whose_end_overflows` — start from `minimal_valid_raf()`, set preview offset to `u32::MAX` and preview length to `1u32`; expect err containing `"overflows"`. (Overflow check at `raf.rs:53-55` fires before the out-of-bounds check.)
4. `read_be_u32_rejects_a_truncated_directory` — call `super::read_be_u32(&[0u8; 2], 0)` directly; expect err containing `"truncated"`. Extend the tests module's `use super::{...}` import accordingly.

Order the byte writes exactly like the existing tests (`data[0x54..0x58].copy_from_slice(&VALUE.to_be_bytes())`).

**Verify**: `build-gate -- cargo test --locked -p fujicli --lib features::render::raf --jobs 4` → all pass, 4 new tests listed.

### Step 2: Format and lint

**Verify**: `cargo fmt --all --check` → exit 0; `build-gate -- cargo clippy --locked --all-features --all-targets --workspace --jobs 4 -- -D warnings` → exit 0.

## Test plan

This plan IS the test plan: 4 new cases enumerated in Step 1, modeled on `rejects_raf_with_an_out_of_bounds_payload_region` (`raf.rs:116-124`). Verification command above.

## Done criteria

- [ ] `rg -c "#\[test\]" src/lib/features/render/raf.rs` reports 9 (5 existing + 4 new)
- [ ] `build-gate -- cargo test --locked -p fujicli --lib features::render::raf --jobs 4` exits 0
- [ ] `cargo fmt --all --check` exits 0
- [ ] `git status` shows only `src/lib/features/render/raf.rs` modified
- [ ] `plans/README.md` status row updated

## STOP conditions

Stop and report back (do not improvise) if:

- A new test FAILS against the current production code — that means one of the rejection branches is actually broken; report the branch and the observed error instead of adjusting the assertion or the production code.
- The file's structure no longer matches the excerpts (drift since `4c6fd8c`).

## Maintenance notes

- If RAF support for additional models ever lands, these tests should be generalized to the shared validator rather than duplicated per model.
- The unreachable-through-public-API status of the `read_be_u32` truncation branch is now documented by test 4; if `RAF_MIN_HEADER_BYTES` is ever lowered or region offsets move past 0x6c, that branch becomes load-bearing and the public-path variant should be added.
