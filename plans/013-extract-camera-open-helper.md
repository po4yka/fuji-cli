# Plan 013: Extract the repeated native-camera open helper in the CLI

> **Executor instructions**: Follow this plan step by step. Run every verification command and confirm the expected result before moving on. Touch only the files listed as in scope. If any STOP condition occurs, stop and report — do not improvise. Commit your work in the worktree following the plan's git workflow section. SKIP updating `plans/README.md` — your reviewer maintains the index. Before reporting, audit every claim against an actual tool result.
>
> **Drift check (run first)**: `git diff --stat 46f2e5e..HEAD -- src/cli`

## Status

- **Priority**: P3
- **Effort**: S
- **Risk**: LOW
- **Depends on**: none
- **Category**: tech-debt
- **Planned at**: commit `46f2e5e`, 2026-08-30

## Why this matters

Ten CLI handlers open the camera with the identical three-argument incantation `usb::get_camera(device, emulate, EmulationAcknowledgement::NotProvided)?`. The third argument is the load-bearing one: it encodes "this command does not accept an emulation acknowledgement", which is what keeps emulated devices out of state-changing paths. Repeating it ten times means a future change to that policy (a new `EmulationAcknowledgement` variant, or a command that legitimately does provide one) must be applied by hand at ten call sites, and a missed site fails open rather than closed. One named helper makes the policy visible and changeable in one place.

## Current state

`src/cli/common/usb.rs` exposes:

```rust
// src/cli/common/usb.rs:205-209
pub fn get_camera(
    device: Option<Location>,
    emulate: Option<Identity>,
    acknowledgement: EmulationAcknowledgement,
) -> anyhow::Result<Camera> {
```

The ten identical call sites, all passing `EmulationAcknowledgement::NotProvided`:

- `src/cli/device/mod.rs:58`
- `src/cli/backup/mod.rs:191`, `src/cli/backup/mod.rs:298`
- `src/cli/simulation/mod.rs:80`, `:115`, `:145`, `:169`, `:194`
- `src/cli/image/mod.rs:140`, `:192`

Each is preceded in its handler by a destructure of `GlobalOptions` (`src/cli/mod.rs:29-41`: `json`, `verbose`, `device`, `emulate`, …). The destructured field subsets differ per handler, so **only the `get_camera` call is being deduplicated here, not the destructuring**.

Conventions: `anyhow::Result` at boundaries; `src/cli/common/` holds shared CLI plumbing (`usb.rs`, `file.rs`, `interrupt.rs`); workspace lints deny `unwrap_used`/`panic`. `tests/cli_process.rs` drives the built binary and asserts argument-validation short-circuits — those tests must stay green and unchanged.

Environment: `export PATH="/Users/po4yka/.local/share/mise/installs/cue/0.17.1:$PATH"` before any cargo command; `build-gate` is on PATH; use `--jobs 3`.

## Commands you will need

| Purpose | Command | Expected on success |
|---------|---------|---------------------|
| Format check | `cargo fmt --all --check` | exit 0 |
| CLI tests | `build-gate -- cargo test --locked --test cli_process --jobs 3` | all pass |
| Clippy | `build-gate -- cargo clippy --locked --all-features --all-targets --workspace --jobs 3 -- -D warnings` | exit 0 |
| Full gate | `build-gate -- cargo test --locked --all-features --all-targets --workspace --jobs 3` | all pass |

## Scope

**In scope**:

- `src/cli/common/usb.rs` (add the helper)
- `src/cli/device/mod.rs`, `src/cli/backup/mod.rs`, `src/cli/simulation/mod.rs`, `src/cli/image/mod.rs` (replace the call sites)

**Out of scope** (do NOT touch):

- `get_camera` itself — its signature and body stay exactly as they are; the helper wraps it.
- `reconnect_camera_by_serial`, `get_all_cameras`, `get_usb_device_by_location` — untouched.
- The `GlobalOptions` destructuring in each handler — field subsets differ per handler; leave them alone.
- `src/lib/**`, `tests/**` — no library or test changes; behavior must be identical.
- Any change to which commands accept emulation, or to `EmulationAcknowledgement`'s variants.

## Git workflow

- Branch: `advisor/013-camera-open-helper`
- Conventional commit. Suggested: `refactor: extract native camera open helper`.
- Do NOT push or open a PR.

## Steps

### Step 1: Add the helper

In `src/cli/common/usb.rs`, beside `get_camera`, add:

```rust
/// Opens the camera for a command that never supplies an emulation
/// acknowledgement — every CLI command except the ones that explicitly
/// negotiate emulation. Centralises the `EmulationAcknowledgement::NotProvided`
/// policy so it can be changed in one place rather than at each call site.
pub fn get_native_camera(
    device: Option<Location>,
    emulate: Option<Identity>,
) -> anyhow::Result<Camera> {
    get_camera(device, emulate, EmulationAcknowledgement::NotProvided)
}
```

**Verify**: `build-gate -- cargo build --locked --workspace --jobs 3` → exit 0.

### Step 2: Replace all ten call sites

Replace each `usb::get_camera(device, emulate, EmulationAcknowledgement::NotProvided)?` with `usb::get_native_camera(device, emulate)?` at the ten locations listed in "Current state". Remove the now-unused `EmulationAcknowledgement` import from any handler module where it has no other use (clippy will flag unused imports under `-D warnings`; do not add `#[allow]` to silence it).

Do not change anything else in those handlers — not the destructuring, not the surrounding control flow.

**Verify**: `rg -n "get_camera\(" src/cli` shows matches only inside `src/cli/common/usb.rs` (the helper and the original function); `build-gate -- cargo clippy --locked --all-features --all-targets --workspace --jobs 3 -- -D warnings` → exit 0.

### Step 3: Confirm behavior is unchanged

No new tests: this is a pure mechanical extraction with no behavior change, and the existing `tests/cli_process.rs` argument-validation tests already exercise these handlers up to the point of device access.

**Verify**: `build-gate -- cargo test --locked --all-features --all-targets --workspace --jobs 3` → all pass, with no test modified.

## Test plan

- No new tests (mechanical refactor, no behavior change).
- Regression safety: the full workspace suite plus `tests/cli_process.rs` unchanged and green; clippy `-D warnings` catches any leftover unused import.

## Done criteria

ALL must hold:

- [ ] `rg -c "EmulationAcknowledgement::NotProvided" src/cli` reports exactly 1 (only inside the new helper)
- [ ] `rg -n "usb::get_camera\(" src/cli` returns no matches outside `src/cli/common/usb.rs`
- [ ] `cargo fmt --all --check` exits 0
- [ ] `build-gate -- cargo clippy --locked --all-features --all-targets --workspace --jobs 3 -- -D warnings` exits 0
- [ ] `build-gate -- cargo test --locked --all-features --all-targets --workspace --jobs 3` exits 0, with `git diff 46f2e5e -- tests/` empty
- [ ] `git status` shows no modified files outside the in-scope list
- [ ] `plans/README.md` status row updated (SKIPPED per reviewer override)

## STOP conditions

Stop and report if:

- Any call site passes something other than `EmulationAcknowledgement::NotProvided` — that site is NOT a candidate; leave it on `get_camera` and report which one.
- Replacing a call site would require changing that handler's `GlobalOptions` destructuring or control flow — report instead of restructuring.
- A test fails after the replacement — the extraction should be behavior-identical; report the failure rather than adjusting the test.

## Maintenance notes

- A command that legitimately negotiates emulation must call `get_camera` directly with its own acknowledgement; the helper exists precisely to make that an explicit, visible exception.
- If `EmulationAcknowledgement` ever gains a variant, this helper is the one place the default policy is decided.
