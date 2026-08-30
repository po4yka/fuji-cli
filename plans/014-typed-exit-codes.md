# Plan 014: Give "camera state unknown" its own exit code

> **Executor instructions**: Follow this plan step by step. Run every verification command and confirm the expected result before moving on. Touch only the files listed as in scope. If any STOP condition occurs, stop and report — do not improvise. Commit your work in the worktree following the plan's git workflow section. SKIP updating `plans/README.md` — your reviewer maintains the index. Before reporting, audit every claim against an actual tool result.
>
> **Drift check (run first)**: `git diff --stat 542d4df..HEAD -- src/main.rs src/cli`

## Status

- **Priority**: P2
- **Effort**: S
- **Risk**: LOW
- **Depends on**: plan 013 (merged — the CLI now opens cameras through `usb::get_native_camera`; this plan builds on that tree)
- **Category**: bug
- **Planned at**: commit `542d4df`, 2026-08-30

## Why this matters

This CLI's safety model rests on one distinction: some failures leave the camera in a known-good state and are safe to retry, and some leave it in an unknown state and **must never be retried automatically**. The code says so repeatedly in prose — `"camera state is unknown and the restore must not be retried automatically"` (`src/cli/backup/mod.rs:339,342`), `"DO NOT RETRY AUTOMATICALLY"` (`src/cli/common/interrupt.rs:32`) — and the docs build a whole operator procedure around it. But every one of these exits with status `1`, exactly like "no camera found" or a bad argument. A wrapper script — which the docs actively encourage, with `--yes` and `--expect-sha256` designed for automation — cannot tell the two apart without scraping stderr text. That is a safety-relevant gap in the process contract, not a cosmetic one.

## Current state

- `src/main.rs:14-36` — the whole exit path:

```rust
fn run() -> anyhow::Result<()> {
    let cli = cli::Cli::parse();
    log::init(cli.options.verbose)?;
    cli::common::interrupt::install()?;
    cli::handle(cli)?;
    Ok(())
}

fn handle_result(result: anyhow::Result<()>) -> anyhow::Result<()> {
    match result {
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|error| error.kind() == std::io::ErrorKind::BrokenPipe) =>
        {
            Ok(())
        }
        result => result,
    }
}

fn main() -> anyhow::Result<()> {
    handle_result(run())
}
```

  Returning `anyhow::Result` from `main` means the standard `Termination` impl prints the error chain and exits `1`. Clap's own parse errors exit `2` (clap does that itself, before `run` returns). There is no `std::process::exit` or `ExitCode` anywhere in `src/` — verified by grep.

  Note the existing `downcast_ref` on `BrokenPipe`: that is the pattern this plan extends — classify by inspecting the error, not by matching message strings.

- The call sites that mean "state unknown" today, all currently exiting `1`:
  - `src/cli/backup/mod.rs:339` — reconnect after an accepted restore failed;
  - `src/cli/backup/mod.rs:342` — the fresh-session verification export failed;
  - `src/cli/backup/mod.rs:346` — `ensure!` on a changed identity after restore;
  - `src/cli/common/interrupt.rs` — `critical_camera_write` returns an error after a latched interrupt (the write completed but an interrupt was requested);
  - `src/lib/features/simulation/transaction.rs:307,311,315` — selector restore failures leave selector state unknown. **These are in the library**, so they need a different treatment — see Scope.

Conventions: `anyhow::Result` with `.context(...)` at boundaries; workspace lints deny `unwrap_used`/`panic`; `tests/cli_process.rs` spawns the built binary and asserts on output/status for argument-validation paths — that is the natural home for exit-code tests.

Environment: `export PATH="/Users/po4yka/.local/share/mise/installs/cue/0.17.1:$PATH"` before any cargo command; `build-gate` is on PATH; use `--jobs 3`.

## Commands you will need

| Purpose | Command | Expected on success |
|---------|---------|---------------------|
| Format check | `cargo fmt --all --check` | exit 0 |
| CLI process tests | `build-gate -- cargo test --locked --test cli_process --jobs 3` | all pass |
| Clippy | `build-gate -- cargo clippy --locked --all-features --all-targets --workspace --jobs 3 -- -D warnings` | exit 0 |
| Full gate | `build-gate -- cargo test --locked --all-features --all-targets --workspace --jobs 3` | all pass |

## Scope

**In scope**:

- `src/main.rs` (classify the error and choose the exit code)
- `src/cli/common/` (a small new module for the marker type, or add it beside the existing helpers — your choice, keep it in one place)
- `src/cli/backup/mod.rs`, `src/cli/common/interrupt.rs` (tag the state-unknown errors)
- `tests/cli_process.rs` (exit-code coverage)
- `docs/users/usage.md` (document the exit codes — they are now a public contract)

**Out of scope** (do NOT touch):

- `src/lib/**` — the library must stay CLI-agnostic. The simulation-transaction errors at `transaction.rs:307-315` already carry structured failure information; if the CLI can classify them from the existing typed error, do so **at the CLI boundary**. If it cannot without changing the library's error type, leave them exiting `1` and report that in NOTES — do not reshape a library error type in this plan.
- Clap's exit code `2` for parse errors, and `0` for success — unchanged.
- The exit code for an interrupt outside a camera write: `src/cli/common/interrupt.rs` already exits `130` there; leave it.
- Any change to which operations are considered dangerous, or to error message text (the messages are asserted by existing tests).

## Git workflow

- Branch: `advisor/014-typed-exit-codes`
- Conventional commit. Suggested: `feat: exit non-retryable failures with a distinct code`.
- Do NOT push or open a PR.

## Steps

### Step 1: Add a marker for non-retryable outcomes

Add a tiny zero-data marker error type (e.g. `pub struct CameraStateUnknown;` implementing `std::error::Error` + `Display`) in a single place under `src/cli/common/`. Its `Display` should be short and non-duplicative — the descriptive message already comes from the `.context(...)` chain; this type exists to be `downcast_ref`'d, not to add prose.

**Verify**: `build-gate -- cargo build --locked --workspace --jobs 3` → exit 0.

### Step 2: Tag the state-unknown call sites

At each CLI site listed in "Current state", attach the marker to the error chain so `main` can find it via `downcast_ref` — keeping the existing human-readable context exactly as it is. Concretely: where the code today does `.context("... camera state is unknown ...")`, the error must end up carrying BOTH that context string (unchanged, existing tests assert on it) and the marker type.

Sites to tag: `backup/mod.rs:339`, `backup/mod.rs:342`, `backup/mod.rs:346`, and the post-interrupt error returned by `critical_camera_write` in `common/interrupt.rs`.

Do NOT tag: "no camera found", argument validation, file I/O, preflight rejections that happen *before* any write. Those are safe-to-retry failures and must keep exit code `1`. The rule is: the marker means *a state-changing operation was already sent and its outcome is not confirmed*.

**Verify**: `build-gate -- cargo clippy --locked --all-features --all-targets --workspace --jobs 3 -- -D warnings` → exit 0.

### Step 3: Map the marker to an exit code in `main`

Change `main` to return `std::process::ExitCode` (keep `run`/`handle_result` returning `anyhow::Result<()>`). On error: if the chain contains the marker (`error.chain().any(|source| source.is::<CameraStateUnknown>())` or an equivalent `downcast_ref` walk), print the error chain to stderr **exactly as the current `Termination` impl does** (`{error:?}` — the debug format is what produces the "Caused by:" chain) and return `ExitCode::from(3)`. Otherwise print the same way and return `ExitCode::FAILURE` (which is 1). Success returns `ExitCode::SUCCESS`.

Keep the `BrokenPipe` special case working exactly as today: it must still yield success.

Pick `3` as the state-unknown code and treat it as a fixed part of the CLI contract from now on.

**Verify**: `build-gate -- cargo test --locked --test cli_process --jobs 3` → all existing tests still pass.

### Step 4: Test the exit codes

In `tests/cli_process.rs` (which already spawns the built binary), add coverage that does not need a camera:

- an argument-validation failure still exits `1` (pick a case the file already exercises and assert the status, not just stderr);
- a clap parse error still exits `2`;
- successful `--help`/`--version` still exits `0`.

For the state-unknown path: a full end-to-end reproduction requires a camera, so instead unit-test the classification itself — a small test asserting that an `anyhow` error built with the marker plus context is classified as state-unknown, and one built without it is not. Put that test next to the classification function so it runs in the normal suite.

**Verify**: `build-gate -- cargo test --locked --all-features --all-targets --workspace --jobs 3` → all pass, including the new tests.

### Step 5: Document the contract

In `docs/users/usage.md`, add a short "Exit codes" section: `0` success, `1` failure (safe to retry after investigating), `2` invalid arguments (clap), `3` a state-changing operation was sent and its outcome is unconfirmed — **do not retry automatically**, `130` interrupted. Match the file's existing prose style and its paragraph wrapping.

**Verify**: `build-gate -- cargo test --locked --all-features --all-targets --workspace --jobs 3` → all pass.

## Test plan

- New: the exit-status assertions and the classification unit tests from Step 4.
- Existing to keep green, unmodified in substance: all of `tests/cli_process.rs`'s current assertions (you may add status assertions to existing cases, but do not weaken or remove an existing assertion).
- Not covered automatically: the real state-unknown path needs a camera; say so in your report.

## Done criteria

ALL must hold:

- [ ] `rg -n "ExitCode" src/main.rs` shows the new mapping; `rg -n "process::exit" src/main.rs` returns nothing (use `ExitCode`, not `process::exit`, so destructors run)
- [ ] The `BrokenPipe` case still yields success (its existing test in `src/main.rs` passes unchanged)
- [ ] Exactly the four sites listed in Step 2 are tagged; `rg -n "CameraStateUnknown" src/` shows the type, those four sites, and the classification in `main` — nothing else
- [ ] `cargo fmt --all --check` exits 0
- [ ] `build-gate -- cargo clippy --locked --all-features --all-targets --workspace --jobs 3 -- -D warnings` exits 0
- [ ] `build-gate -- cargo test --locked --all-features --all-targets --workspace --jobs 3` exits 0; new tests exist and pass
- [ ] `docs/users/usage.md` documents all five codes
- [ ] `git status` shows no modified files outside the in-scope list
- [ ] `plans/README.md` status row updated (SKIPPED per reviewer override)

## STOP conditions

Stop and report if:

- `src/main.rs` no longer matches the excerpt (drift since `542d4df`).
- Tagging a site would change an error message that an existing test asserts on — the message must stay; report if you cannot attach the marker without altering it.
- Classifying the simulation-transaction failures would require changing a library error type — leave them at exit `1` and report it as a follow-up; the library stays CLI-agnostic.
- You find yourself classifying by matching message strings (e.g. `error.to_string().contains("unknown")`) — that is exactly the fragile coupling this plan replaces; stop and report instead.

## Maintenance notes

- Exit codes are now a public contract alongside stdout/stderr and JSON shape; changing one is a breaking change for wrappers.
- The rule for future code: tag with the marker only when a state-changing operation has already been sent and its outcome is unconfirmed. Preflight rejections and pre-write failures stay at `1`.
- If the simulation-transaction failures are left unclassified by this plan, they are the obvious next candidate once the library's error type is revisited.
