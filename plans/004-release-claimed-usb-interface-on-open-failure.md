# Plan 004: Guarantee USB interface release on every `Camera::open_with` failure path

> **Executor instructions**: Follow this plan step by step. Run every verification command and confirm the expected result before moving to the next step. If anything in the "STOP conditions" section occurs, stop and report — do not improvise. When done, update the status row for this plan in `plans/README.md`.
>
> **Drift check (run first)**: `git diff --stat 4c6fd8c..HEAD -- src/lib/camera.rs src/lib/ptp/mod.rs`
> If any in-scope file changed since this plan was written, compare the "Current state" excerpts against the live code before proceeding; on a mismatch, treat it as a STOP condition.

## Status

- **Priority**: P2
- **Effort**: S
- **Risk**: LOW
- **Depends on**: none
- **Category**: bug
- **Planned at**: commit `4c6fd8c`, 2026-08-30

## Why this matters

`Camera::open_with` claims the USB interface, then performs several fallible steps before ownership reaches `Ptp` — the only type whose `Drop` releases the interface. If any of those steps fails, the `rusb::DeviceHandle` is dropped with the interface still claimed, relying on undocumented per-platform `libusb_close` behavior. On backends that don't auto-release, a subsequent open of the same camera in the same process fails with "resource busy". The fix closes the gap so cleanup is guaranteed by construction, matching the crate's otherwise strict RAII posture.

## Current state

- `src/lib/camera.rs` — `Camera::open_with`; the claim-to-ownership window:

```rust
// src/lib/camera.rs:273-321 (abridged)
let handle = device.open()?;
handle.claim_interface(binding.interface)?;          // <- claimed here
debug!("Claimed interface");
handle.set_alternate_setting(binding.interface, binding.setting)?;   // fallible
// ...
let chunk_policy = ptp::ChunkPolicy::for_transport(/* ... */)?;      // fallible, does NOT use handle
validate_bulk_read_geometry(/* read.initial_bytes ... */)?;          // fallible, does NOT use handle
validate_bulk_read_geometry(/* read.ceiling_bytes ... */)?;          // fallible, does NOT use handle
// ... debug! log ...
let mut ptp = Ptp::new(bus, address, binding.interface, binding.bulk_in, binding.bulk_out, handle, chunk_policy)?;  // fallible
let session_control_permit = ptp.open_session(SESSION)?;             // fallible, but Ptp now owns cleanup
```

- `src/lib/ptp/mod.rs:378-404` — `Ptp::new` is fallible because `BulkReadState::new(initial_read_chunk)?` is evaluated inside the `Self { .. }` struct literal; if it errors, the `handle` parameter is dropped without `Ptp`'s `Drop` ever running.
- `src/lib/ptp/mod.rs:1556-1562` — the only explicit release:

```rust
impl Drop for Ptp {
    fn drop(&mut self) {
        if let Err(e) = self.handle.release_interface(self.interface) {
            error!("Failed to release USB interface: {e}");
        }
    }
}
```

- Conventions: RAII cleanup with logged-but-swallowed errors in `Drop` (exemplar above); `anyhow::Result` at boundaries; workspace lints deny `unwrap_used`/`panic`. `build-gate` wraps compiler-backed cargo commands on the maintainer's Mac; if unavailable, run the inner command directly with the same `--jobs` ceiling.
- Note: `ChunkPolicy::for_transport` and both `validate_bulk_read_geometry` calls consume only `binding` data and `speed` (`device.speed()` — readable before `device.open()`), so they can be computed before the claim. `BulkReadState::new` takes `chunk_policy.read.initial_bytes`, also computable before the claim.

## Commands you will need

| Purpose | Command | Expected on success |
|---------|---------|---------------------|
| Format check | `cargo fmt --all --check` | exit 0 |
| Focused tests | `build-gate -- cargo test --locked -p fujicli --lib ptp:: --jobs 4` | all pass |
| Workspace build | `build-gate -- cargo build --locked --workspace --jobs 4` | exit 0 |
| Clippy | `build-gate -- cargo clippy --locked --all-features --all-targets --workspace --jobs 4 -- -D warnings` | exit 0 |

## Scope

**In scope** (the only files you should modify):

- `src/lib/camera.rs`
- `src/lib/ptp/mod.rs` (only `Ptp::new`'s construction order / signature if needed, and only as described below)

**Out of scope** (do NOT touch, even though they look related):

- `Ptp`'s `Drop` impl and the transaction/session logic in `ptp/mod.rs` — the existing steady-state cleanup is correct.
- `src/cli/**` — no CLI contract changes.
- Any behavioral change on the success path (ordering of USB control operations `claim_interface` → `set_alternate_setting` must be preserved relative to each other).

## Steps

### Step 1: Reorder pure computations before the claim

In `Camera::open_with`, move the `ChunkPolicy::for_transport` call and both `validate_bulk_read_geometry` calls so they execute BEFORE `device.open()` / `claim_interface`. Read `device.speed()` before opening (rusb exposes `speed()` on `Device`, not the handle — it is already called on `device` today). Also hoist the fallible part of `Ptp::new`: construct `BulkReadState::new(chunk_policy.read.initial_bytes)?` before the claim, and change `Ptp::new` to accept the ready `BulkReadState` (making `Ptp::new` infallible), OR keep `Ptp::new`'s signature and instead validate `initial_read_chunk` before claiming — choose the first option unless it ripples beyond `camera.rs` call sites (`rg -n "Ptp::new" src/` — at `4c6fd8c` the only caller is `camera.rs`).

After this step the only fallible operations between `claim_interface` and `Ptp` construction are `set_alternate_setting` and the infallible-by-now `Ptp::new`.

**Verify**: `build-gate -- cargo build --locked --workspace --jobs 4` → exit 0.

### Step 2: Guard the remaining window

Add a small private RAII guard in `src/lib/camera.rs`:

```rust
struct ClaimedInterface {
    handle: Option<rusb::DeviceHandle<rusb::GlobalContext>>,
    interface: u8,
}

impl ClaimedInterface {
    fn claim(handle: rusb::DeviceHandle<rusb::GlobalContext>, interface: u8) -> anyhow::Result<Self> { /* claim_interface, then wrap */ }
    fn into_handle(mut self) -> rusb::DeviceHandle<rusb::GlobalContext> { /* take handle, mem::forget-free: rely on Option */ }
}

impl Drop for ClaimedInterface {
    fn drop(&mut self) {
        if let Some(handle) = &self.handle {
            if let Err(error) = handle.release_interface(self.interface) {
                log::error!("Failed to release USB interface after open failure: {error}");
            }
        }
    }
}
```

Use it in `open_with`: `let claimed = ClaimedInterface::claim(handle, binding.interface)?;`, call `set_alternate_setting` through `claimed.handle` (borrow via a small accessor), and pass `claimed.into_handle()` to `Ptp::new` as the last fallible-free step. `into_handle` takes the handle out of the `Option`, so the guard's `Drop` becomes a no-op once ownership transfers to `Ptp` — no double release. Match the `Drop`-logs-error convention shown in the `Ptp` exemplar.

**Verify**: `build-gate -- cargo build --locked --workspace --jobs 4` → exit 0, and `build-gate -- cargo clippy --locked --all-features --all-targets --workspace --jobs 4 -- -D warnings` → exit 0.

### Step 3: Run the focused and full test suites

No behavioral change is expected on the success path; existing PTP and camera tests must pass unchanged.

**Verify**: `build-gate -- cargo test --locked --all-features --all-targets --workspace --jobs 4` → all pass.

## Test plan

- No new automated test: `rusb::DeviceHandle` cannot be constructed without hardware, and the guard's logic is a two-branch `Option` dance fully exercised by the type system. Document the guard's contract in its doc comment instead.
- Regression safety comes from the full workspace suite plus clippy (`-D warnings` catches an unused/unmoved handle).
- If the maintainer has an X-T5 available, a manual smoke check is: `fujicli device info` twice in a row succeeds (open → close → reopen), and remains out of executor scope.

## Done criteria

Machine-checkable. ALL must hold:

- [ ] In `src/lib/camera.rs`, no fallible `?` expression that can fail after `claim_interface` executes outside the guard's protection (inspect `open_with` top-to-bottom)
- [ ] `rg -n "BulkReadState::new" src/lib/ptp/mod.rs src/lib/camera.rs` shows it is no longer evaluated inside `Ptp`'s `Self { .. }` literal
- [ ] `cargo fmt --all --check` exits 0
- [ ] `build-gate -- cargo clippy --locked --all-features --all-targets --workspace --jobs 4 -- -D warnings` exits 0
- [ ] `build-gate -- cargo test --locked --all-features --all-targets --workspace --jobs 4` exits 0
- [ ] `git status` shows no modified files outside the in-scope list
- [ ] `plans/README.md` status row updated

## STOP conditions

Stop and report back (do not improvise) if:

- `open_with` no longer matches the excerpt (drift since `4c6fd8c`).
- `Ptp::new` has callers outside `src/lib/camera.rs` (check `rg -n "Ptp::new" src crates`) — the signature change then needs coordination; report the call sites.
- Reordering `ChunkPolicy::for_transport` before `device.open()` turns out to be impossible because some input actually requires the open handle — report which input, and fall back to guarding the whole window instead of reordering.
- `set_alternate_setting` cannot be called through a borrow of the guarded handle without fighting the borrow checker into `unsafe` or `clone` — report; do not add `unsafe` (the crate is `#![forbid(unsafe_code)]`).

## Maintenance notes

- Any future fallible step added to `open_with` between claim and `Ptp` construction must go through the guard; reviewer should reject bare `handle.…?` calls in that window.
- Deliberately deferred: a transport seam that would make this window unit-testable with a fake handle — that is the larger refactor tracked as the `preflight::run()` fake-transport finding (not planned in this batch).
