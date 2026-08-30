# Plan 008: Add the sanctioned probe-scoped raw single-property write primitive and wire the 0xD18C probe to it

> **Executor instructions**: Follow this plan step by step. Run every verification command and confirm the expected result before moving on. Touch only the files listed as in scope. This plan deliberately reopens a sealed mutation surface under a feature gate — if any STOP condition occurs, stop immediately and report; do not improvise around the seal. Commit your work in the worktree following the plan's git workflow section. SKIP updating `plans/README.md` — your reviewer maintains the index. Before reporting, audit every claim against an actual tool result.
>
> **Drift check (run first)**: `git diff --stat 6270fcf..HEAD -- Cargo.toml src/lib/ptp/mod.rs src/lib/camera.rs crates/fujicli-dev`
> If any in-scope file changed since this plan was written, compare the "Current state" excerpts against the live code before proceeding; on a mismatch, treat it as a STOP condition.

## Status

- **Priority**: P3
- **Effort**: L
- **Risk**: MED (reopens the raw-mutation surface sealed by commit `124aa4f`, but strictly behind a non-default feature and scoped to one property round trip)
- **Depends on**: plan 007 (DONE for Steps 1-2 — the design note, feature flag, and probe skeleton are on `main` as of `b409be9`; this plan replaces the probe's refusal with a real, guarded implementation)
- **Category**: direction
- **Planned at**: commit `6270fcf`, 2026-08-30

## Why this matters

Film-simulation commands are disabled on the X-T5 because no evidence establishes whether PTP selector `0xD18C` addresses the still or the movie C1-C7 custom-setting namespace. Plan 007 built the design note, the `dangerous-reverse-engineering` feature, and a `probe simulation-namespace` command skeleton that currently refuses to run, because reaching `0xD18C` needs a raw single-property write and the library's only write path requires a `MutationPermit` from a `Verified` preflight profile — which cannot exist for an unverified property. The maintainer has now **sanctioned** a narrowly-scoped primitive to close that gap (recorded in `docs/contributors/reversing.md`, "Maintainer decisions (2026-08-30)"). This plan implements that primitive and replaces the probe's refusal with the real guard sequence, so the only remaining work is the maintainer's physical-device run (an X RAW Studio capture first, then this probe as the fallback).

## Current state

### The seal this plan reopens (understand before touching it)

`SetDevicePropValue` is only reachable through one path today; this is deliberate (`124aa4f "fix: seal raw PTP mutation access"`):

- `src/lib/ptp/mod.rs:441-450` — `send_for_operation` (the `pub(crate)` read path used by `get_prop_raw`) calls `validate_read_only_send(code, params)?` first, which **rejects** state-changing commands.
- `src/lib/ptp/mod.rs:452-476` — `send_mutating` / `send_mutating_for_operation` (`pub(super)`) is the only path that issues a state-changing command. It requires a `&mut MutationPermit` and calls `validate_mutation_permit` before `send_unchecked_for_operation`.
- `src/lib/ptp/mod.rs:478-502` — `send_unchecked_for_operation` (private `fn`) is the transport core: it owns the transaction id, poisoning flag, chunk policy, and deadline handling. Both the read and mutating paths funnel through it. **The new primitive must go through this core** to keep that transport safety — it must not hand-roll a transfer.
- `src/lib/preflight.rs:46-133` — `MutationPermit` / `MutationAuthorization` are private to the `preflight` module and can only be built by `preflight::run` (`src/lib/preflight.rs:457` calls `activate_mutation_permit`). `MutationAuthorization::validate` (`preflight.rs:82-122`) rejects any `SetDevicePropValue` whose property was not validated by a `Verified` preflight profile. There is deliberately no way to build a permit for `0xD18C`.

Conclusion: the sanctioned primitive cannot obtain or fake a `MutationPermit` (correct — that machinery must stay intact). It must instead issue the single `SetDevicePropValue` **directly through the transport core**, compiled only under the new feature.

### The read side already exists

- `src/lib/ptp/mod.rs:661-666` — `get_prop_raw(prop)` (`pub(crate)`) issues `GetDevicePropValue` via the read path. Reuse it for the before/after read-backs; do not add a new read primitive.
- `src/lib/camera.rs:419-423` — `reverse_device_property(code)` (`pub`, `#[cfg(feature = "reverse-tools")]`) already exposes `get_prop_raw` to `fujicli-dev`.

### Feature wiring today

- Root `Cargo.toml:59-61` — `[features]` has `default = []` and `reverse-tools = []`. There is **no** `dangerous-reverse-engineering` feature in the root crate yet.
- `crates/fujicli-dev/Cargo.toml:12-15` — `reverse-tools = ["fujicli/reverse-tools"]` and `dangerous-reverse-engineering = ["reverse-tools"]`. The dev-crate `dangerous-reverse-engineering` currently forwards only to its own `reverse-tools`; it must also forward to a new root-crate `fujicli/dangerous-reverse-engineering`.

### The probe skeleton to replace

- `crates/fujicli-dev/src/probe.rs` — `ProbeCommand::SimulationNamespace { slot }` whose `handle` currently `bail!`s with the "blocked" message. `CustomSettingSlot` (C1-C7) is defined here.
- `crates/fujicli-dev/src/reverse.rs:35-49` — `ProbeSummary` accounting pattern (the crate's style for observing a sequence of fallible steps).
- `crates/fujicli-dev/src/output.rs` — `NewOutput` no-clobber writer (reuse for the pre-backup and the audit log).
- `crates/fujicli-dev/src/usb.rs` — `Location` plumbing; the `probe` subcommand already requires an explicit `--device`.
- `docs/contributors/reversing.md` — "Design: the `simulation-namespace` Probe" section (contract, decision table) and "Maintainer decisions (2026-08-30)" (the two sanctioned choices). The six-step guard requirements are in "Requirements for Any Future Dangerous Probe" earlier in the same file.

### Conventions

`anyhow::Result` with contextual errors; no panics on device/user input; workspace lints deny `unwrap_used`/`panic`; `#![forbid(unsafe_code)]`. Feature-gated public library items use `#[doc(hidden)] #[cfg(feature = "...")]` (see `reverse_device_property`). `build-gate` wraps compiler-backed cargo commands; the machine-wide wrapper caps jobs at 3, so use `--jobs 3` (and `--jobs 2` for release). `cue` must be on PATH (`/Users/po4yka/.local/share/mise/installs/cue/0.17.1`).

## Commands you will need

| Purpose | Command | Expected on success |
|---------|---------|---------------------|
| Format check | `cargo fmt --all --check` | exit 0 |
| Dev-crate build (dangerous) | `build-gate -- cargo build --locked -p fujicli-dev --features reverse-tools,dangerous-reverse-engineering --jobs 3` | exit 0 |
| Dev-crate tests (dangerous) | `build-gate -- cargo test --locked -p fujicli-dev --features reverse-tools,dangerous-reverse-engineering --jobs 3` | all pass |
| Lib tests with feature | `build-gate -- cargo test --locked -p fujicli --features dangerous-reverse-engineering --lib --jobs 3` | all pass |
| Default-features build (leak check) | `build-gate -- cargo build --locked --workspace --jobs 3` | exit 0 |
| Full gate | `build-gate -- cargo test --locked --all-features --all-targets --workspace --jobs 3` | all pass |
| Clippy (all features) | `build-gate -- cargo clippy --locked --all-features --all-targets --workspace --jobs 3 -- -D warnings` | exit 0 |
| Release build (distribution rule) | `build-gate -- cargo build --locked --release --workspace --jobs 2` | exit 0 |

## Suggested executor toolkit

- After finishing, a `code-reviewer` (or `oh-my-claudecode:code-reviewer`) pass is REQUIRED before this is merge-ready — this diff touches the sealed mutation surface (see `~/.claude/rules/llm-rust-prompts.md` diff-acceptance gate). Note this in your report; the human reviewer will run it.

## Scope

**In scope**:

- `Cargo.toml` (root — add the `dangerous-reverse-engineering` feature)
- `src/lib/ptp/mod.rs` (add the gated transport-core write primitive)
- `src/lib/camera.rs` (add the gated `Camera` wrapper)
- `crates/fujicli-dev/Cargo.toml` (forward the feature to `fujicli/dangerous-reverse-engineering`)
- `crates/fujicli-dev/src/probe.rs` (replace the refusal with the guard sequence)
- `crates/fujicli-dev/src/` (new small modules if needed: audit-log writer, decision table — keep them in the dev crate)
- `docs/contributors/reversing.md` (update the run procedure to reflect the implemented command; do NOT change the two open questions' resolution text already recorded)

**Out of scope** (do NOT touch):

- `MutationPermit` / `MutationAuthorization` / `preflight::run` — the permit machinery stays exactly as is. The primitive must NOT construct, fake, or weaken a permit.
- `validate_read_only_send`, `send_for_operation`, `send_mutating*` — do not relax any of them.
- `fml/**`, `tests/xt5_simulation_domain.rs`, `docs/users/support.md` — the fail-closed invariant and support table change only after captured physical evidence, which this plan does not produce.
- Any auto-retry of the mutating write. Single send only.
- Running anything against hardware.

## Git workflow

- Branch: `advisor/008-probe-write-primitive`
- Conventional commits (repo example: `fix: seal raw PTP mutation access`). Suggested: `feat: add gated probe-scoped raw property write`.
- Do NOT push or open a PR unless the operator instructed it.

## Steps

### Step 1: Add the root-crate feature

In root `Cargo.toml` `[features]`, add `dangerous-reverse-engineering = ["reverse-tools"]` (it implies `reverse-tools` so the read helpers are available). Keep `default = []` — this feature must never be default.

**Verify**: `build-gate -- cargo build --locked --workspace --jobs 3` → exit 0 (default features, new feature not compiled); `cargo metadata --format-version 1 | rg -q dangerous-reverse-engineering` or `rg -n "dangerous-reverse-engineering" Cargo.toml` → present.

### Step 2: Add the transport-core write primitive on `Ptp`

In `src/lib/ptp/mod.rs`, add a method gated behind `#[cfg(feature = "dangerous-reverse-engineering")]`, placed near `get_prop_raw`/`set_prop_raw`:

```rust
/// Issues exactly one `SetDevicePropValue` for `prop` WITHOUT a
/// `MutationPermit`, bypassing the preflight/permit authorization that
/// `send_mutating` requires. This deliberately reopens the raw-mutation
/// surface sealed by `124aa4f`, sanctioned ONLY for the `0xD18C` still/movie
/// namespace probe (see docs/contributors/reversing.md, "Maintainer
/// decisions"). It still goes through the transport core, so transaction-id
/// sequencing, poisoning, and chunk policy are preserved. Single send; the
/// caller must not retry. Compiled only under `dangerous-reverse-engineering`.
#[cfg(feature = "dangerous-reverse-engineering")]
pub(crate) fn probe_write_single_property_unverified(
    &mut self,
    prop: u16,
    value: &[u8],
) -> anyhow::Result<Vec<u8>> {
    ensure!(
        command_risk(CommandCode::SetDevicePropValue, &[u32::from(prop)])
            == CommandRisk::StateChanging,
        "probe write expected a state-changing SetDevicePropValue"
    );
    debug!("PROBE (unverified, unpermitted) SetDevicePropValue 0x{prop:04x}");
    self.send_unchecked_for_operation(
        PtpOperation::Standard,
        CommandCode::SetDevicePropValue,
        &[u32::from(prop)],
        Some(value),
    )
}
```

Do not change any existing method. Confirm `send_unchecked_for_operation`, `command_risk`, `CommandRisk`, `PtpOperation`, `CommandCode` are all in scope at that location (they are, per the excerpts).

**Verify**: `build-gate -- cargo build --locked -p fujicli --features dangerous-reverse-engineering --jobs 3` → exit 0.

### Step 3: Add the gated `Camera` wrapper

In `src/lib/camera.rs`, beside `reverse_device_property` (`camera.rs:419-423`), add:

```rust
/// Probe-only single-property write. See
/// [`crate::ptp::Ptp::probe_write_single_property_unverified`]; sanctioned for
/// the 0xD18C namespace probe, compiled only under
/// `dangerous-reverse-engineering`.
#[doc(hidden)]
#[cfg(feature = "dangerous-reverse-engineering")]
pub fn reverse_probe_write_single_property(
    &mut self,
    prop: u16,
    value: &[u8],
) -> anyhow::Result<Vec<u8>> {
    self.ptp.probe_write_single_property_unverified(prop, value)
}
```

The read counterpart the probe needs already exists as `reverse_device_property`.

**Verify**: `build-gate -- cargo build --locked -p fujicli --features dangerous-reverse-engineering --jobs 3` → exit 0.

### Step 4: Forward the dev-crate feature to the library feature

In `crates/fujicli-dev/Cargo.toml`, change `dangerous-reverse-engineering = ["reverse-tools"]` to `dangerous-reverse-engineering = ["reverse-tools", "fujicli/dangerous-reverse-engineering"]`.

**Verify**: `build-gate -- cargo build --locked -p fujicli-dev --features reverse-tools,dangerous-reverse-engineering --jobs 3` → exit 0; the dev crate can now see `Camera::reverse_probe_write_single_property`.

### Step 5: Implement the probe guard sequence in `probe.rs`

Replace the `bail!` refusal in `ProbeCommand::SimulationNamespace`'s handler with the six-step guard sequence from `docs/contributors/reversing.md` "Requirements for Any Future Dangerous Probe", using `Camera::open_unknown` → the read helpers → `reverse_probe_write_single_property`. Structure it so the non-I/O logic is unit-testable as pure functions:

1. **Identity + fingerprint gate** — open by explicit `--device`, read `GetDeviceInfo`, display bus/address, VID:PID, PTP manufacturer/model/firmware, and a **SHA-256 serial fingerprint** (never the raw serial — hash it with the `sha2` crate already in the workspace). Require the operator to type the exact live fingerprint back; mismatch aborts before any write.
2. **Fixed acknowledgement string** — hard-code `I-UNDERSTAND-THIS-WRITES-SELECTOR-D18C`; require exact match.
3. **Pre-backup** — export a fresh no-clobber backup via the existing `reverse_export_backup` + `NewOutput` path; validate and hash it before proceeding.
4. **Audit record** — append one JSONL line to a no-clobber audit log with ONLY the allowed fields (`reversing.md` "Requirements": timestamp, tool version, invocation ID, operation/risk class, PTP op codes, USB location, VID:PID, bounded model/firmware, minimized serial fingerprint, pre-backup digest, outcome). Timestamps/IDs must be injected from the call site, not generated inside a pure function (keeps it testable).
5. **Snapshot → write → read-back → restore → verify** — read the current `0xD18C` value (snapshot), write the chosen C1-C7 slot once via the primitive, read the still/movie observables, then restore the snapshot value once and verify the read-back equals the snapshot. Any timeout/disconnect/malformed response after the write prints `DO NOT RETRY AUTOMATICALLY` and exits non-zero with no retry.
6. **Verdict** — feed the observed before/after states into a pure `decision(observed) -> Verdict { Still | Movie | Ambiguous }` function matching the decision table in `reversing.md`; on `Ambiguous`, print `DO NOT RETRY AUTOMATICALLY`.

Because no still/movie wire observable is known (open question 1, unresolved at the wire level), the read-back in sub-step 5 records whatever `0xD18C`-adjacent properties the design note lists plus the raw `0xD18C` echo, and the verdict function returns `Ambiguous` unless a concrete observable is supplied — the command must NOT fabricate a Still/Movie verdict from an unknown signal. Print guidance that the operator should corroborate with the camera's C1-C7 LCD menu per the maintainer decision.

Factor the device round trips behind a small trait (e.g. `ProbeIo` with `read_prop`/`write_prop`/`export_backup`) so tests can drive the sequence with a fake recorder (mirror `ProbeSummary`'s style), asserting: snapshot happens before write, restore happens after read-back, and no second write ever occurs.

**Verify**: `build-gate -- cargo test --locked -p fujicli-dev --features reverse-tools,dangerous-reverse-engineering --jobs 3` → all pass.

### Step 6: Unit tests

Add, in the dev crate, tests for the pure/fake-testable surface (no device I/O):

- **Gate refusal**: a wrong fingerprint and a wrong acknowledgement string each abort before any `write_prop` call on the fake (assert the fake recorded zero writes).
- **Audit allowlist**: serialize an audit record and assert the JSON contains only the allowed keys and NONE of: raw serial, argv, full paths, property payloads, full error chains.
- **Decision table**: `decision(...)` returns `Still`/`Movie`/`Ambiguous` for the table's rows, and returns `Ambiguous` for the "no known observable" default.
- **Sequence ordering**: driving the sequence against the fake records exactly `read(snapshot) → write → read(observable) → write(restore) → read(verify)` in that order, with exactly two writes total (the probe write and the single restore), and the restore write carries the snapshot value.

**Verify**: `build-gate -- cargo test --locked -p fujicli-dev --features reverse-tools,dangerous-reverse-engineering --jobs 3` → all pass, including the new tests.

### Step 7: Leak check, docs, and full gate

- Leak check: `build-gate -- cargo build --locked --workspace --jobs 3` (default features) → exit 0, and `rg -n "probe_write_single_property_unverified|reverse_probe_write_single_property" src/` shows both are behind `#[cfg(feature = "dangerous-reverse-engineering")]` (inspect the lines above each match).
- Update `docs/contributors/reversing.md`'s design section to say the command is now implemented and describe the exact run procedure (device selection, fingerprint confirmation, acknowledgement string, where the backup and audit log are written). Keep the hard-wrapped paragraph style of that file. Do NOT alter the recorded "Maintainer decisions" text.
- Full gate + release build.

**Verify**: `build-gate -- cargo test --locked --all-features --all-targets --workspace --jobs 3` → all pass; `build-gate -- cargo clippy --locked --all-features --all-targets --workspace --jobs 3 -- -D warnings` → exit 0; `build-gate -- cargo build --locked --release --workspace --jobs 2` → exit 0.

## Test plan

- New tests: the four groups in Step 6, in `crates/fujicli-dev/src/probe.rs` (or a sibling module), driven by a fake `ProbeIo`. Model the fake on `ProbeSummary` (`crates/fujicli-dev/src/reverse.rs:35-49`).
- No test performs device I/O; there is no `0xD18C` fixture — that is the point of the probe.
- Existing `fujicli-dev` tests (probe present/absent by feature, `--device` required) must stay green.

## Done criteria

Machine-checkable. ALL must hold:

- [ ] `rg -n "probe_write_single_property_unverified" src/lib/ptp/mod.rs` shows exactly one definition, and the preceding line is `#[cfg(feature = "dangerous-reverse-engineering")]`
- [ ] `rg -n "reverse_probe_write_single_property" src/lib/camera.rs` shows the wrapper gated by the same `#[cfg(...)]`
- [ ] `build-gate -- cargo build --locked --workspace --jobs 3` (default features) exits 0 and neither symbol compiles (grep-confirm the cfg gate)
- [ ] `crates/fujicli-dev/src/probe.rs` no longer contains the `bail!("blocked` refusal; the command runs the guard sequence
- [ ] `MutationPermit`, `MutationAuthorization`, `preflight::run`, `validate_read_only_send`, `send_mutating*` are byte-unchanged (`git diff 6270fcf -- src/lib/preflight.rs` shows no change to permit types; the mod.rs diff adds only the new gated method)
- [ ] `git diff 6270fcf -- fml/ tests/xt5_simulation_domain.rs docs/users/support.md` → empty
- [ ] `cargo fmt --all --check` exits 0
- [ ] `build-gate -- cargo clippy --locked --all-features --all-targets --workspace --jobs 3 -- -D warnings` exits 0
- [ ] `build-gate -- cargo test --locked --all-features --all-targets --workspace --jobs 3` exits 0; the four Step 6 test groups exist and pass
- [ ] `build-gate -- cargo build --locked --release --workspace --jobs 2` exits 0
- [ ] `git status` shows no modified files outside the in-scope list
- [ ] `plans/README.md` status row updated (SKIPPED per reviewer override)

## STOP conditions

Stop and report back (do not improvise) if:

- Any in-scope file drifted since `6270fcf` (excerpts don't match).
- You find you must modify `MutationPermit`/`MutationAuthorization`/`preflight::run`/`validate_read_only_send`/`send_mutating*`, or fake/weaken a permit, to make the primitive work — the primitive must be a clean transport-core call under the feature gate. If it can't be, report the exact obstacle.
- The primitive or its `Camera` wrapper compiles under default features (leak-check fails) — do not ship a distributable that can reach it.
- Implementing the read-back in Step 5 requires inventing a PTP property code to name the still/movie observable — do NOT invent one; keep the verdict `Ambiguous` and record the observable as still-open, per `AGENTS.md`.
- Anything requires editing `fml/`, `tests/xt5_simulation_domain.rs`, or `docs/users/support.md`.
- You are about to run the probe against hardware. Never do this — the capture and probe run are maintainer steps.

## Maintenance notes

- This diff reopens a sealed surface; per `~/.claude/rules/llm-rust-prompts.md` it MUST get a separate `code-reviewer` pass before merge (the reviewer runs it, not the executor). Call this out in your report.
- The primitive is intentionally the narrowest possible reopening: one property, one send, no retry, feature-gated, no permit. Any future widening (more properties, loops, default-feature exposure) must be re-sanctioned by the maintainer.
- After the maintainer's physical run resolves the still/movie namespace, `fml/` gains the verified selector facts, `tests/xt5_simulation_domain.rs` flips its assertion, and `docs/users/support.md` updates the X-T5 Simulations cell — all outside this plan.
- The audit-log JSONL field set is a contract; keep it stable once first written.
