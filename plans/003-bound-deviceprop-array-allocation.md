# Plan 003: Bound device-property array allocation by available bytes and make it fallible

> **Executor instructions**: Follow this plan step by step. Run every verification command and confirm the expected result before moving to the next step. If anything in the "STOP conditions" section occurs, stop and report — do not improvise. When done, update the status row for this plan in `plans/README.md`.
>
> **Drift check (run first)**: `git diff --stat 4c6fd8c..HEAD -- src/lib/ptp/descriptor.rs src/lib/ptp/codec.rs`
> If any in-scope file changed since this plan was written, compare the "Current state" excerpts against the live code before proceeding; on a mismatch, treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: S
- **Risk**: LOW
- **Depends on**: none
- **Category**: security
- **Planned at**: commit `4c6fd8c`, 2026-08-30

## Why this matters

`DevicePropDesc` array decoding trusts a device-supplied `u32` element count: it checks only a 4,194,304-element cap and then calls `Vec::with_capacity(count)` before confirming that any element bytes actually exist in the response buffer. A malicious or malfunctioning camera can declare a huge count in a tiny payload and trigger a large speculative allocation from a handful of wire bytes (~64 MiB for 16-byte `UInt128` elements at the cap, and `DevicePropValue` in-memory elements are larger than their wire size). `Vec::with_capacity` also aborts the process on allocation failure, unlike the recoverable-error pattern this same crate already uses for PTP arrays in `codec.rs`. This plan brings `descriptor.rs` up to the crate's own established standard.

## Current state

- `src/lib/ptp/descriptor.rs` — decodes `GetDevicePropDesc`/property values from a `Cursor<&[u8]>` over the already-received, size-bounded response payload. The gap:

```rust
// src/lib/ptp/descriptor.rs:350-366
fn read_array(
    reader: &mut Cursor<&[u8]>,
    element_type: DevicePropDataType,
) -> anyhow::Result<DevicePropValue> {
    const MAX_DEVICE_PROP_ARRAY_VALUES: usize = 4 * 1024 * 1024;

    let count = usize::try_from(read_u32(reader)?)?;
    ensure!(
        count <= MAX_DEVICE_PROP_ARRAY_VALUES,
        "PTP device property array exceeds safety limit"
    );
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        values.push(read_value(reader, element_type)?);
    }
    Ok(DevicePropValue::Array(values))
}
```

- `DevicePropValue` (`descriptor.rs:59-65`) stores scalars as `Int(i128)`/`UInt(u128)`, so each in-memory element is at least 16 bytes plus the enum tag regardless of wire size.
- The scalar wire sizes by `DevicePropDataType`: `Int8`/`UInt8` = 1, `Int16`/`UInt16` = 2, `Int32`/`UInt32` = 4, `Int64`/`UInt64` = 8, `Int128`/`UInt128` = 16. `read_array` is only ever called with these scalar element types (`descriptor.rs:335-344`).
- The crate's own exemplar of the correct pattern — mirror this:

```rust
// src/lib/ptp/codec.rs:98-118 (inside PtpArray::read_options)
let allocation_bytes = length.checked_mul(T::WIRE_SIZE).ok_or_else(|| { /* InvalidData */ })?;
if allocation_bytes > MAX_PTP_ARRAY_ALLOCATION_BYTES { /* InvalidData */ }
let mut values = Vec::new();
values.try_reserve_exact(length).map_err(|error| { /* OutOfMemory */ })?;
```

- The strongest available bound here is better than a fixed budget: `reader` is a `Cursor<&[u8]>` over the complete response, so the exact number of remaining bytes is known before allocating — `reader.get_ref().len() - usize::try_from(reader.position())?`. If `count * wire_size(element_type)` exceeds the remaining bytes, the array is malformed and can be rejected before any allocation.
- Existing tests live in `#[cfg(test)] mod tests` at `descriptor.rs:429` onward; they build byte fixtures inline and call `DevicePropDesc::decode(&bytes)` (see `descriptor_decodes_writable_scalar_without_form` at `descriptor.rs:432`). Model new tests on that pattern.
- Conventions: `anyhow::Result` with `ensure!`/contextual messages; no panics on device input; workspace lints deny `unwrap_used`/`panic`. `build-gate` wraps compiler-backed cargo commands on the maintainer's Mac; if unavailable, run the inner command directly with the same `--jobs` ceiling.

## Commands you will need

| Purpose | Command | Expected on success |
|---------|---------|---------------------|
| Format check | `cargo fmt --all --check` | exit 0 |
| Focused tests | `build-gate -- cargo test --locked -p fujicli --lib ptp:: --jobs 4` | all pass |
| Workspace build | `build-gate -- cargo build --locked --workspace --jobs 4` | exit 0 |
| Clippy | `build-gate -- cargo clippy --locked --all-features --all-targets --workspace --jobs 4 -- -D warnings` | exit 0 |

## Scope

**In scope** (the only files you should modify):

- `src/lib/ptp/descriptor.rs`

**Out of scope** (do NOT touch, even though they look related):

- `src/lib/ptp/codec.rs` — already correct; it is the exemplar, not a target.
- `read_string` in `descriptor.rs:368-379` — its `Vec::with_capacity` is bounded by a `u8` length (max 254 `u16` units); harmless, leave it.
- Any change to `MAX_DEVICE_PROP_ARRAY_VALUES` semantics for well-formed responses — valid arrays that fit their payload must keep decoding identically.

## Git workflow

- Branch: `advisor/003-bound-descriptor-array-allocation`
- Conventional commits (repo examples: `fix: harden PTP bulk framing`, `fix: classify temporary simulation reads`). Suggested: `fix: bound device property array allocation`.
- Do NOT push or open a PR unless the operator instructed it.

## Steps

### Step 1: Add a wire-size helper and pre-allocation bounds to `read_array`

In `src/lib/ptp/descriptor.rs`, extend `read_array` so that, after the existing element-count cap, it:

1. computes the element wire size from `element_type` (a small private `const fn wire_size(element_type: DevicePropDataType) -> Option<usize>` returning `Some(1|2|4|8|16)` for the ten scalar types and `None` otherwise — return an error via `ensure!` on `None` rather than panicking);
2. computes `required_bytes = count.checked_mul(wire_size)` and errors on overflow (message: `"PTP device property array size overflows"`);
3. computes remaining bytes as `reader.get_ref().len().saturating_sub(usize::try_from(reader.position())?)` and errors if `required_bytes > remaining` (message: `"PTP device property array is larger than its payload"`);
4. replaces `Vec::with_capacity(count)` with `Vec::new()` + `values.try_reserve_exact(count)` mapped to an `anyhow` error (message pattern: `"failed to reserve PTP device property array: {error}"`), matching the `codec.rs:112-118` exemplar.

The decode loop and success value stay unchanged.

**Verify**: `build-gate -- cargo build --locked --workspace --jobs 4` → exit 0.

### Step 2: Add rejection tests

In the `descriptor.rs` tests module, following the inline-fixture style of `descriptor_decodes_writable_scalar_without_form` (`descriptor.rs:432`), add:

- `array_count_exceeding_payload_is_rejected_before_allocation`: a `UInt128Array` descriptor whose count field declares the maximum permitted count (`4 * 1024 * 1024`) but whose buffer contains no element bytes → `DevicePropDesc::decode` returns `Err`, and the error string contains `"larger than its payload"`.
- `array_matching_its_payload_still_decodes`: a small valid array (e.g. `UInt16Array` with 2 elements and exactly 4 element bytes) decodes to the expected `DevicePropValue::Array` — proves the new bound does not over-reject.

Keep the existing tests (including `decode_rejects_ptp_array_count_above_safety_limit`-style count-cap coverage) passing unchanged.

**Verify**: `build-gate -- cargo test --locked -p fujicli --lib ptp:: --jobs 4` → all pass, including the 2 new tests.

## Test plan

- New tests: the two cases in Step 2, in `src/lib/ptp/descriptor.rs` tests module.
- Existing coverage to keep green: all current `descriptor.rs` and `codec.rs` tests under `cargo test --lib ptp::`.
- Verification: `build-gate -- cargo test --locked -p fujicli --lib ptp:: --jobs 4` → all pass.

## Done criteria

Machine-checkable. ALL must hold:

- [ ] `rg -n "with_capacity" src/lib/ptp/descriptor.rs` matches only the bounded `read_string` site (line ~374), not `read_array`
- [ ] `cargo fmt --all --check` exits 0
- [ ] `build-gate -- cargo clippy --locked --all-features --all-targets --workspace --jobs 4 -- -D warnings` exits 0
- [ ] `build-gate -- cargo test --locked --all-features --all-targets --workspace --jobs 4` exits 0; both new tests exist and pass
- [ ] `git status` shows no modified files outside the in-scope list
- [ ] `plans/README.md` status row updated

## STOP conditions

Stop and report back (do not improvise) if:

- `read_array` at `descriptor.rs:350-366` no longer matches the excerpt (drift since `4c6fd8c`).
- `read_array` turns out to be reachable with a non-scalar `element_type` (nested arrays) — the wire-size helper's assumption would be wrong; report rather than guessing a size.
- An existing test fails because it depended on decoding an array whose declared count exceeds its payload — that would mean real traffic relies on the lax behavior; report with the failing test name.

## Maintenance notes

- If a new `DevicePropDataType` variant is ever added, `wire_size` must be extended; the `ensure!` on `None` makes the omission fail loudly instead of mis-sizing.
- Reviewer focus: confirm the remaining-bytes computation uses the cursor position at the time of the check (after the count field was consumed), and that well-formed fixtures still decode byte-identically.
