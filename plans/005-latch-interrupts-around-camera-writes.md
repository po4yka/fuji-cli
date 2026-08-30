# Plan 005: Latch Ctrl-C around irreversible camera writes and report do-not-retry state

> **Executor instructions**: Follow this plan step by step. Run every verification command and confirm the expected result before moving to the next step. If anything in the "STOP conditions" section occurs, stop and report — do not improvise. When done, update the status row for this plan in `plans/README.md`.
>
> **Drift check (run first)**: `git diff --stat 4c6fd8c..HEAD -- src/main.rs src/cli Cargo.toml Cargo.lock`
> If any in-scope file changed since this plan was written, compare the "Current state" excerpts against the live code before proceeding; on a mismatch, treat it as a STOP condition.

## Status

- **Priority**: P2
- **Effort**: M
- **Risk**: LOW
- **Depends on**: none
- **Category**: bug
- **Planned at**: commit `4c6fd8c`, 2026-08-30

## Why this matters

The restore/write paths are engineered so an interrupted write is detectable and clearly flagged: a recovery backup is exported first, the CLI prints "PTP restore was accepted … waiting for a fresh camera session to verify" and the docs mandate "the restore must not be retried automatically" (`src/cli/backup/mod.rs:330-334`, `docs/users/usage.md`). But the process installs no signal handler at all (`rg -n "ctrlc|signal|SIGINT" src Cargo.toml` → no matches at `4c6fd8c`), so a plain Ctrl-C during the blocking USB bulk write kills the process via the OS default disposition — no warning, no do-not-retry guidance, no chance for the carefully built messaging to run. This plan latches SIGINT during the few identified camera-mutation windows so the write completes (or fails on its own terms) and the user always sees the correct guidance.

## Current state

- `src/main.rs:14-36` — `run()` parses args, inits logging, dispatches; `main()` only translates `BrokenPipe` into success. No signal handling anywhere.
- The camera-mutation call sites to protect (each is a blocking PTP bulk operation that changes camera state):
  - `src/cli/backup/mod.rs:315-326` — `session.restore(backup, …)` inside the `restore_after_recovery_saved` closure.
  - `src/cli/simulation/mod.rs:148` — `.update_simulation(slot, partial)?`.
  - `src/cli/simulation/mod.rs:196` — `.set_simulation(slot, &*simulation)?`.
  - `src/cli/image/mod.rs:153` — `session.render(&image, base, draft)?` (uploads the RAF and triggers in-camera processing; `image/mod.rs:161` then cleans up the rendered object).
- `src/cli/common/` currently holds `usb.rs` and `file.rs` — shared CLI plumbing; the new module belongs beside them.
- Dependency policy (from `AGENTS.md`): adding a production dependency requires explicit need plus review of maintenance, security, license, and platform impact; `Cargo.lock` changes must be deliberate and use `--locked` afterward. The standard library has no portable signal API, so a crate is genuinely required here. Use `ctrlc` (version `3.x`, MIT/Apache-2.0, tiny, cross-platform, no default features needed) — it is the smallest maintained crate that installs a SIGINT/SIGTERM/console-ctrl handler. Do not pull in `signal-hook`'s larger surface for this.
- Conventions: `anyhow::Result` at boundaries; warnings via `log::warn!`; workspace lints deny `unwrap_used`/`panic`; `#![forbid(unsafe_code)]`. `build-gate` wraps compiler-backed cargo commands on the maintainer's Mac; if unavailable, run the inner command directly with the same `--jobs` ceiling.
- The wording to reuse for the unknown-state guidance already exists: `docs/contributors/reversing.md:122-126` mandates printing `DO NOT RETRY AUTOMATICALLY` when camera state is ambiguous; `backup/mod.rs:334` uses "camera state is unknown and the restore must not be retried automatically".

## Commands you will need

| Purpose | Command | Expected on success |
|---------|---------|---------------------|
| Format check | `cargo fmt --all --check` | exit 0 |
| Workspace build | `build-gate -- cargo build --locked --workspace --jobs 4` | exit 0 |
| Focused CLI tests | `build-gate -- cargo test --locked --test cli_process --jobs 4` | all pass |
| Full gate | `build-gate -- cargo test --locked --all-features --all-targets --workspace --jobs 4` | all pass |
| Clippy | `build-gate -- cargo clippy --locked --all-features --all-targets --workspace --jobs 4 -- -D warnings` | exit 0 |

Note: adding the dependency changes `Cargo.lock`; run the first build without `--locked` once (`build-gate -- cargo build --workspace --jobs 4`) to update the lockfile, review the lock diff (it should add only `ctrlc` and its small platform deps, e.g. `nix` on Unix), then use `--locked` everywhere after.

## Scope

**In scope** (the only files you should modify):

- `Cargo.toml`, `Cargo.lock` (add `ctrlc` only)
- `src/main.rs` (install the handler once)
- `src/cli/common/interrupt.rs` (create), `src/cli/common/mod.rs` (register the module)
- `src/cli/backup/mod.rs`, `src/cli/simulation/mod.rs`, `src/cli/image/mod.rs` (wrap the four call sites)

**Out of scope** (do NOT touch, even though they look related):

- `src/lib/**` — the library stays signal-agnostic; latching is a CLI-process concern.
- Read-only commands (`device info/list`, `backup export`, simulation reads) — no guard needed; do not wrap them.
- Any attempt to cancel an in-flight USB transfer — explicitly NOT the goal; the write must run to completion.
- Windows-specific console niceties beyond what `ctrlc` provides by default.

## Git workflow

- Branch: `advisor/005-interrupt-latch`
- Conventional commits (repo examples: `fix: verify camera state changes semantically`). Suggested: `fix: latch interrupts around camera mutations` plus a separate `chore:`-style commit is NOT needed — keep one commit including the lockfile change, with rationale in the body.
- Do NOT push or open a PR unless the operator instructed it.

## Steps

### Step 1: Add the dependency

In `Cargo.toml` `[dependencies]`, add `ctrlc = "3.5"`. Build once without `--locked` to update `Cargo.lock`; inspect the lock diff and confirm only `ctrlc` and its platform deps were added.

**Verify**: `build-gate -- cargo build --workspace --jobs 4` → exit 0; `git diff Cargo.lock` shows only the new crate subtree.

### Step 2: Create the latch module

Create `src/cli/common/interrupt.rs` with two atomics and two functions:

```rust
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

static IN_CRITICAL_WRITE: AtomicBool = AtomicBool::new(false);
static INTERRUPTS_SEEN: AtomicU8 = AtomicU8::new(0);

/// Install the process-wide handler. Call exactly once, before dispatch.
pub fn install() -> anyhow::Result<()> {
    ctrlc::set_handler(|| {
        if !IN_CRITICAL_WRITE.load(Ordering::SeqCst) {
            // Not inside a camera write: behave like the default disposition.
            eprintln!("interrupted");
            std::process::exit(130);
        }
        let seen = INTERRUPTS_SEEN.fetch_add(1, Ordering::SeqCst);
        if seen == 0 {
            eprintln!("interrupt received during a camera write; finishing the current PTP operation first (press Ctrl-C again to force-quit; camera state will then be unknown)");
        } else {
            eprintln!("forced quit during a camera write; camera state is unknown. DO NOT RETRY AUTOMATICALLY");
            std::process::exit(130);
        }
    })?;
    Ok(())
}

/// Run `operation` with interrupts latched. If an interrupt arrived while it
/// ran, log the do-not-retry guidance and return an error after the fact.
pub fn critical_camera_write<T>(
    description: &str,
    operation: impl FnOnce() -> anyhow::Result<T>,
) -> anyhow::Result<T> { /* set IN_CRITICAL_WRITE, run, clear flag (also on error via a small Drop guard), then:
    if INTERRUPTS_SEEN was raised, log::warn! the standard guidance naming `description`,
    reset INTERRUPTS_SEEN, and bail!("{description} completed, but an interrupt was requested; stopping before any further camera work") on success-with-interrupt,
    returning the operation's own Err unchanged when it failed */ }
```

Details that are load-bearing: the flag must be cleared by a `Drop` guard so a panic-free early `?` return inside `operation` cannot leave `IN_CRITICAL_WRITE` set; on success-with-pending-interrupt the function returns an error AFTER the write result is fully reported via `log::warn!`, so the caller's normal error path prints the message and exits non-zero without retrying. Note the handler runs on its own thread — `std::process::exit(130)` there may skip buffered log flushing; the plain `eprintln!` calls are deliberate for that reason.

Register the module in `src/cli/common/mod.rs` following how `usb`/`file` are declared there.

**Verify**: `build-gate -- cargo build --locked --workspace --jobs 4` → exit 0.

### Step 3: Install the handler and wrap the four call sites

- In `src/main.rs` `run()`, call the install function right after `log::init(...)` (before dispatch). A failure to install is a hard error (`?`).
- Wrap each mutation call:
  - `src/cli/backup/mod.rs` — inside the closure at lines 319-325, wrap the `session.restore(...)` call: `interrupt::critical_camera_write("backup restore", || session.restore(backup, target_serial_sha256.as_deref()))`.
  - `src/cli/simulation/mod.rs:148` and `:196` — wrap `update_simulation` / `set_simulation` the same way with descriptions `"simulation update"` / `"simulation write"`.
  - `src/cli/image/mod.rs:153` — wrap `session.render(&image, base, draft)` with description `"RAW render upload"`. Do NOT wrap the cleanup at `image/mod.rs:161` — cleanup must still run after an interrupt.

Mind the closure captures: `session` is used mutably; if the borrow checker rejects the closure form at a site, restructure that one call as set-flag/call/check-flag inline using two small public helpers (`enter_critical_write()` returning a guard + `take_interrupt_request()`) instead of forcing the closure — keep the same semantics.

**Verify**: `build-gate -- cargo clippy --locked --all-features --all-targets --workspace --jobs 4 -- -D warnings` → exit 0.

### Step 4: Unit-test the latch logic

In `src/cli/common/interrupt.rs` tests module: exercise `critical_camera_write` directly (no real signals) by exposing a `#[cfg(test)]` helper that sets `INTERRUPTS_SEEN`, then assert: (a) operation result passes through untouched when no interrupt was recorded; (b) success-with-interrupt returns `Err` containing the description; (c) the in-critical flag is clear after both success and error returns. Mark the tests to run serially if they share the statics (they do — use one test function with sequential phases, the repo's tests are already single-process per binary).

**Verify**: `build-gate -- cargo test --locked --all-features --all-targets --workspace --jobs 4` → all pass, including the new tests; `build-gate -- cargo test --locked --test cli_process --jobs 4` → all pass (no behavior change for the argument-validation paths it covers).

## Test plan

- New: the latch state-machine tests from Step 4 (three phases listed there).
- Existing to keep green: `tests/cli_process.rs` (arg-validation short circuits) and the full workspace suite.
- Manual (maintainer, out of executor scope): Ctrl-C during `fujicli backup import` against a physical X-T5 prints the latch message, lets the restore call finish, then prints do-not-retry guidance and exits non-zero.

## Done criteria

Machine-checkable. ALL must hold:

- [ ] `rg -n "ctrlc" Cargo.toml src/main.rs` shows the dependency and exactly one `install` call site
- [ ] `rg -n "critical_camera_write|enter_critical_write" src/cli` covers all four mutation sites (backup restore, update_simulation, set_simulation, render) and no read-only site
- [ ] `cargo fmt --all --check` exits 0
- [ ] `build-gate -- cargo clippy --locked --all-features --all-targets --workspace --jobs 4 -- -D warnings` exits 0
- [ ] `build-gate -- cargo test --locked --all-features --all-targets --workspace --jobs 4` exits 0; latch tests exist and pass
- [ ] `git status` shows no modified files outside the in-scope list
- [ ] `plans/README.md` status row updated

## STOP conditions

Stop and report back (do not improvise) if:

- The four call sites don't match the cited lines (drift since `4c6fd8c`).
- `ctrlc 3.x` fails `cargo deny` (license/advisory) — report the deny output; do not switch crates on your own.
- Wrapping `session.restore` inside `restore_after_recovery_saved`'s closure requires changing that helper's signature in `src/lib` — the library is out of scope; use the guard-style helpers at that site instead, and if that also fails, report.
- You are tempted to make the handler cancel or time out the USB transfer — that is explicitly out of scope; stop and report if completion-latching seems insufficient for some site.

## Maintenance notes

- Every future state-changing CLI command (e.g. if simulation import lands after the 0xD18C probe) must wrap its mutation in the same latch; reviewers should grep for new `session.` mutation calls without it.
- The handler's `eprintln!`-then-`exit(130)` on double Ctrl-C bypasses log4rs; if structured logging of forced quits ever matters, revisit with an async-signal-safe design.
- Deliberately deferred: distinct exit codes for "state unknown" vs generic failure (separate audit finding; would compose well with this latch).
