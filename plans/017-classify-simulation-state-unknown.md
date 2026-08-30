# Plan 017: Give simulation selector-restore failures the state-unknown exit code

> **Executor instructions**: Follow this plan step by step. Run every verification command and confirm the expected result before moving on. Touch only the files listed as in scope. If any STOP condition occurs, stop and report — do not improvise. Commit your work in the worktree following the plan's git workflow section. SKIP updating `plans/README.md` — your reviewer maintains the index. Before reporting, re-run each verification command on your committed tree and quote the actual observed result; do not report a check as passing based on an earlier run.
>
> **Drift check (run first)**: `git diff --stat d4746a6..HEAD -- src/cli/simulation/mod.rs src/main.rs src/cli/common`

## Status

- **Priority**: P2
- **Effort**: S
- **Risk**: LOW
- **Depends on**: plan 014 (merged as `1080d78` — the `CameraStateUnknown` marker and exit code 3 exist)
- **Category**: bug
- **Planned at**: commit `d4746a6`, 2026-08-30

## Why this matters

Plan 014 gave "a state-changing operation was sent and its outcome is unconfirmed" its own exit code (3), so wrappers can tell it apart from ordinary retryable failures. It tagged the backup-restore and interrupt paths, but left the simulation paths at exit 1 on the assumption that classifying them would require changing a library error type. That assumption was wrong, as the plan-014 executor discovered: `SimulationTransactionError` already exposes everything needed. Today a simulation operation that leaves the camera's custom-setting selector in an unknown state exits with the same code as "no camera found" — precisely the ambiguity exit code 3 exists to remove.

Note this is **not only a write-path concern**. The selector-restore failures live on the *read* paths too: a `get_simulation` that succeeds but then fails to restore the previously selected slot leaves selector state unknown.

## Current state

The library side (already public, do NOT change it):

```rust
// src/lib/features/simulation/transaction.rs:28-31
pub enum SimulationFailureState {
    // ...
    CameraStateUnknown,
}

// src/lib/features/simulation/transaction.rs:50, :77, :123
pub struct SimulationTransactionError { /* ... */ }
    pub fn state(&self) -> SimulationFailureState
impl std::error::Error for SimulationTransactionError
```

The error is constructed with `state: SimulationFailureState::CameraStateUnknown` at `transaction.rs:553`, `:562`, `:639`, `:661`, `:672`, backing the messages at `transaction.rs:307,311,315` ("restoring the original slot selector also failed … selector state is unknown", and siblings).

The CLI marker and classification from plan 014 (reuse, do NOT redefine):

- `src/cli/common/camera_state.rs` — `pub struct CameraStateUnknown` (a data-free marker implementing `std::error::Error`).
- `src/main.rs` — classifies with `error.is::<CameraStateUnknown>()` on the outer `anyhow::Error` (this searches the whole chain regardless of nesting) and maps to `ExitCode::from(3)`.
- The attachment pattern plan 014 established: `original_error.context(CameraStateUnknown).context("unchanged human message")` — the marker goes on first, the human-readable context on top, so the displayed message is unchanged.

**Important**: `error.chain().any(|s| s.is::<T>())` does NOT work with the pinned `anyhow` — chain items are anyhow's internal wrappers. Use `is::<T>()`/`downcast_ref::<T>()` on the outer `anyhow::Error`. This was verified empirically during plan 014.

The call sites in `src/cli/simulation/mod.rs` where a `SimulationTransactionError` can surface (verify each yourself; line numbers are from `d4746a6`):

- `:117` — `session.get_simulation(slot)?` (in the get handler)
- `:151` — `session.update_simulation(slot, partial)?` inside `interrupt::critical_camera_write`
- `:171` — `session.get_simulation(slot)?` (in the export handler)
- `:199` — `session.set_simulation(slot, &*simulation)?` inside `interrupt::critical_camera_write`
- also check `session.get_simulations(&slots)?` at `:84` and any other call returning this error type

Conventions: `anyhow::Result` at boundaries; workspace lints deny `unwrap_used`/`panic`; the repo enables `clippy::pedantic` + `clippy::nursery` and runs clippy with `-D warnings` — lints like `redundant_clone`, `assigning_clones`, and `too_many_lines` will fail the build, and adding `#[allow(clippy::...)]` is not acceptable (no precedent in this codebase; extract a helper instead).

Environment: `export PATH="/Users/po4yka/.local/share/mise/installs/cue/0.17.1:$PATH"` before any cargo command; `build-gate` is on PATH; use `--jobs 3`.

## Commands you will need

| Purpose | Command | Expected on success |
|---------|---------|---------------------|
| Format check | `cargo fmt --all --check` | exit 0 |
| Focused tests | `build-gate -- cargo test --locked -p fujicli --lib simulation --jobs 3` | all pass |
| Clippy | `build-gate -- cargo clippy --locked --all-features --all-targets --workspace --jobs 3 -- -D warnings` | exit 0 |
| Full gate | `build-gate -- cargo test --locked --all-features --all-targets --workspace --jobs 3` | all pass |

## Scope

**In scope**:

- `src/cli/simulation/mod.rs` (tag the errors)
- `docs/users/usage.md` (only if its Exit Codes section needs a sentence noting simulation commands can also exit 3 — keep it minimal)

**Out of scope** (do NOT touch):

- `src/lib/**` — in particular `src/lib/features/simulation/transaction.rs`. It already exposes `state()`; no library change is needed or wanted.
- `src/main.rs` and `src/cli/common/camera_state.rs` — the marker and the classification already work; reuse them as-is.
- The other CLI handlers (backup, image, device) — already handled by plan 014.
- Any error message text — messages must stay byte-identical; existing tests assert on them.
- Any change to which simulation operations are permitted (simulation writes remain gated/disabled on X-T5 by FML; this plan only changes how a failure is *classified*).

## Git workflow

- Branch: `advisor/017-simulation-state-unknown`
- Conventional commit. Suggested: `fix: classify simulation selector failures as state unknown`.
- Do NOT push or open a PR.

## Steps

### Step 1: Add a single tagging helper

In `src/cli/simulation/mod.rs`, add one private helper that takes a `Result<T, SimulationTransactionError>` and returns `anyhow::Result<T>`, attaching the `CameraStateUnknown` marker **only** when `error.state() == SimulationFailureState::CameraStateUnknown`, and otherwise converting the error unchanged.

Keep it one helper used by every site — do not repeat the match at each call. Preserve the error's own `Display` output exactly (the marker goes underneath via `.context(CameraStateUnknown)`; do not add any new human-readable text).

**Verify**: `build-gate -- cargo build --locked --workspace --jobs 3` → exit 0.

### Step 2: Apply it at every site that can yield the error

Route each call listed in "Current state" through the helper. Two of them sit inside `interrupt::critical_camera_write(...)` closures that currently do `Ok(session.update_simulation(...)?)` — the tagging must happen **inside** the closure, on the raw `SimulationTransactionError`, before it becomes an `anyhow::Error`.

Confirm you covered every site: after editing, grep the file for calls into the session API and check each one's error type. A site that cannot produce a `SimulationTransactionError` must be left alone.

**Verify**: `build-gate -- cargo clippy --locked --all-features --all-targets --workspace --jobs 3 -- -D warnings` → exit 0.

### Step 3: Test the classification

Add unit tests in `src/cli/simulation/mod.rs` proving the mapping, without a device:

- a `SimulationTransactionError` whose `state()` is `CameraStateUnknown`, passed through the helper, produces an `anyhow::Error` for which `error.is::<CameraStateUnknown>()` is true;
- a `SimulationTransactionError` with any other state produces an error for which it is false;
- in both cases the resulting `error.to_string()` is unchanged from the original error's `Display`.

If constructing a `SimulationTransactionError` from outside the library is not possible with its current public API, that is a STOP condition — report it rather than adding a constructor to the library.

**Verify**: `build-gate -- cargo test --locked --all-features --all-targets --workspace --jobs 3` → all pass, including the new tests.

### Step 4: Documentation touch-up

If `docs/users/usage.md`'s Exit Codes section implies only backup commands can exit 3, add a short clause noting simulation commands can too when the selector's state is left unknown. Match the file's existing wrapping. If the section is already generic, change nothing and say so.

**Verify**: `cargo fmt --all --check` → exit 0.

## Test plan

- New: the three classification cases in Step 3.
- Existing to keep green, unmodified: the full workspace suite, especially `tests/xt5_simulation_domain.rs` (the fail-closed invariant) and `tests/cli_process.rs`.

## Done criteria

ALL must hold:

- [ ] `git diff d4746a6 -- src/lib/` is empty (no library change)
- [ ] Every `src/cli/simulation/mod.rs` call that can return a `SimulationTransactionError` goes through the helper (state this explicitly in your report, listing the sites you checked and their error types)
- [ ] `rg -n "chain\(\)" src/cli/simulation/mod.rs` returns nothing (classification uses `is::<T>()`/`downcast_ref`, not a chain walk)
- [ ] No `#[allow(clippy::` was added anywhere
- [ ] `cargo fmt --all --check` exits 0
- [ ] `build-gate -- cargo clippy --locked --all-features --all-targets --workspace --jobs 3 -- -D warnings` exits 0
- [ ] `build-gate -- cargo test --locked --all-features --all-targets --workspace --jobs 3` exits 0; the new tests exist and pass
- [ ] `git status` shows no modified files outside the in-scope list
- [ ] `plans/README.md` status row updated (SKIPPED per reviewer override)

## STOP conditions

Stop and report if:

- `src/cli/simulation/mod.rs` or the marker/classification no longer match the excerpts (drift since `d4746a6`).
- A `SimulationTransactionError` cannot be constructed in a test with the current public API — report; do not add a library constructor.
- Tagging changes any error's `Display` output, or breaks a test that asserts on a message.
- You find yourself classifying by message text rather than by `state()` — stop.
- Covering a site would require touching `src/lib/**` — stop and report which site and why.

## Maintenance notes

- The rule stays the same as plan 014's: the marker means a state-changing operation was already sent (or a selector was left switched) and the outcome is unconfirmed. Read operations that fail cleanly, before touching the selector, stay at exit 1.
- If `SimulationFailureState` gains a variant, this helper's match must be revisited — prefer an exhaustive match without a wildcard arm so the compiler points at it.
