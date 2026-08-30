# Plan 010: Append a terminal outcome record to the probe audit log

> **Executor instructions**: Follow this plan step by step. Run every verification command and confirm the expected result before moving on. Touch only the files listed as in scope. If any STOP condition occurs, stop and report — do not improvise. Commit your work in the worktree following the plan's git workflow section. SKIP updating `plans/README.md` — your reviewer maintains the index. Before reporting, audit every claim against an actual tool result.
>
> **Drift check (run first)**: `git diff --stat 46f2e5e..HEAD -- crates/fujicli-dev docs/contributors/reversing.md`

## Status

- **Priority**: P2
- **Effort**: S
- **Risk**: LOW
- **Depends on**: plan 008 (merged as `9904e07` — the probe, its audit writer, and the guard sequence exist on `main`)
- **Category**: security
- **Planned at**: commit `46f2e5e`, 2026-08-30

## Why this matters

The `probe simulation-namespace` command writes exactly one audit line, before the mutating write runs, with a hardcoded `outcome: "attempted"`. That ordering is deliberate and must stay — it guarantees a durable pre-write trail even if the process dies mid-write. But it also means the audit log **alone** can never distinguish "the camera was written to and successfully restored" from "the write landed and the restore failed, leaving the camera mutated": the real outcome is only printed to stderr, which nothing durably captures. For a probe whose entire justification is auditability on physical hardware, the log must be self-sufficient for later forensics. This was raised as a MEDIUM finding by the sealed-surface review of plan 008.

## Current state

- `crates/fujicli-dev/src/audit.rs` — `AuditRecord` (13 allowlisted fields, including `pub outcome: String`), its private `to_json` (the single place the allowlist is expressed), and `pub fn append(path, record)` which appends one JSONL line, creating the file `0600` on Unix and never truncating. The allowlist test asserts **exact key-set equality** via a `BTreeSet`, so adding a field to `to_json` without updating the test fails immediately — that is intended; keep it that way.
- `crates/fujicli-dev/src/probe.rs` — inside the guarded sequence:

```rust
// crates/fujicli-dev/src/probe.rs (abridged, around the audit write)
    let record = AuditRecord {
        // ... timestamp, tool_version, invocation_id, operation, risk_class,
        // ptp_operation_codes, usb_location, vid_pid, model, firmware,
        // serial_fingerprint, pre_backup_sha256 ...
        outcome: "attempted".to_owned(),
    };
    audit::append(audit_log, &record).context("durably recording the probe attempt")?;

    let _observed = run_write_sequence(io, slot)?;
```

  `run_write_sequence(io, slot)` performs snapshot → write → read-back → restore → verify and returns `anyhow::Result<_>`; every anomalous path inside it returns `Err` (and the CLI then exits non-zero). Today, when it returns `Err`, the `?` propagates immediately and **no second audit line is written**.
- `docs/contributors/reversing.md` — "Requirements for Any Future Dangerous Probe" fixes the audit-log field allowlist (timestamp, tool version, invocation ID, operation/risk class, PTP operation codes, USB location, VID:PID, bounded model/firmware, minimized serial fingerprint, pre-backup digest, outcome) and forbids raw serials, argv, full paths, payloads, custom setting names, arbitrary camera strings, and full error chains. The "Design: the `simulation-namespace` Probe" section documents the run procedure.

Conventions: `anyhow::Result` with `.context(...)` at boundaries; no `unwrap`/`expect` outside tests (workspace lints deny them); `#![forbid(unsafe_code)]`; the crate builds only with `--features reverse-tools,dangerous-reverse-engineering`.

Environment: `export PATH="/Users/po4yka/.local/share/mise/installs/cue/0.17.1:$PATH"` before any cargo command (`build.rs` needs `cue`); `build-gate` is on PATH; use `--jobs 3`.

## Commands you will need

| Purpose | Command | Expected on success |
|---------|---------|---------------------|
| Format check | `cargo fmt --all --check` | exit 0 |
| Dev-crate tests | `build-gate -- cargo test --locked -p fujicli-dev --features reverse-tools,dangerous-reverse-engineering --jobs 3` | all pass |
| Clippy | `build-gate -- cargo clippy --locked --all-features --all-targets --workspace --jobs 3 -- -D warnings` | exit 0 |
| Full gate | `build-gate -- cargo test --locked --all-features --all-targets --workspace --jobs 3` | all pass |

## Scope

**In scope**:

- `crates/fujicli-dev/src/probe.rs`
- `crates/fujicli-dev/src/audit.rs` (only if a small helper is genuinely needed — the record struct and `to_json` allowlist must NOT gain fields)
- `docs/contributors/reversing.md` (document the two-record shape in the probe's design/run-procedure section)

**Out of scope** (do NOT touch):

- The `AuditRecord` field set and `to_json`'s key list — the allowlist is a contract; the terminal record reuses the SAME fields with a different `outcome` value. Do NOT add an `error`/`detail`/`message` field: full error chains are explicitly forbidden by the contract.
- The pre-write record's position — it must still be written BEFORE `run_write_sequence`.
- `src/lib/**`, `fml/**`, `tests/xt5_simulation_domain.rs`, `docs/users/support.md`.
- Any change that makes a failure exit zero, or that adds a retry.

## Git workflow

- Branch: `advisor/010-probe-audit-outcome`
- Conventional commit. Suggested: `fix: record probe terminal outcome in audit log`.
- Do NOT push or open a PR.

## Steps

### Step 1: Classify the terminal outcome without widening the allowlist

Introduce a small closed set of outcome strings and a helper that maps the result of `run_write_sequence` to exactly one of them. Suggested values (keep them short, lowercase, stable — they become part of the log contract):

- `"restored"` — the sequence completed and the restore was verified equal to the snapshot;
- `"write_failed"` — the mutating write itself returned an error (camera most likely untouched, but not guaranteed);
- `"readback_failed"` — the write succeeded but the post-write read-back failed;
- `"restore_failed"` — the restore write returned an error (camera left mutated);
- `"restore_verify_mismatch"` — the restore was sent but the verification read did not match the snapshot (camera state uncertain).

The mapping must come from the sequence's own control flow, not from string-matching an `anyhow` message. If `run_write_sequence` currently collapses these cases into opaque errors, give it a small typed outcome (e.g. an enum returned in the `Err` case via a dedicated error type, or restructure it to return `Result<Observed, ProbeFailure>` where `ProbeFailure` carries the stage). Keep the existing stderr messages and the non-zero exit behavior exactly as they are.

**Verify**: `build-gate -- cargo build --locked -p fujicli-dev --features reverse-tools,dangerous-reverse-engineering --jobs 3` → exit 0.

### Step 2: Append the terminal record on every path

After `run_write_sequence` resolves — on both success and failure — append a second `AuditRecord` that is identical to the pre-write one except for `outcome`, which carries the Step 1 classification. Requirements:

- the terminal append must happen on the failure path too, before the error propagates (do not let `?` skip it);
- a failure to write the terminal audit line must not mask the original probe error — if both fail, the probe error is what the operator sees, and the audit-write failure is surfaced as context;
- no retry of anything;
- the same `invocation_id` must appear on both lines so they can be correlated (this is what makes two lines readable as one attempt).

**Verify**: `build-gate -- cargo clippy --locked --all-features --all-targets --workspace --jobs 3 -- -D warnings` → exit 0.

### Step 3: Tests

Extend the dev-crate tests (driving the existing fake `ProbeIo`, no device I/O):

- **success path**: a fake whose sequence succeeds produces exactly two audit lines; the first has `outcome: "attempted"`, the second `outcome: "restored"`; both share one `invocation_id`.
- **restore-failure path**: a fake whose restore write fails produces two lines, the second with `outcome: "restore_failed"`, and the command still returns `Err`.
- **allowlist holds for the terminal record too**: serialize the terminal record and assert the exact key set is unchanged (reuse the existing allowlist test's `BTreeSet` comparison) and that it contains none of: raw serial, argv, paths, payloads, error chains.

Write audit output to a `tempfile`-backed path in tests (`tempfile` is already a workspace dependency); never write into the repo.

**Verify**: `build-gate -- cargo test --locked -p fujicli-dev --features reverse-tools,dangerous-reverse-engineering --jobs 3` → all pass, including the new tests.

### Step 4: Document the two-record shape

In `docs/contributors/reversing.md`, in the probe's design/run-procedure section, state that one attempt produces two JSONL lines sharing an `invocation_id` — a pre-write `attempted` record and a terminal record whose `outcome` is one of the Step 1 values — and that the field allowlist is identical for both. Keep the file's existing hard-wrapped paragraph style. Do NOT alter the "Maintainer decisions (2026-08-30)" text or the "Requirements for Any Future Dangerous Probe" list.

**Verify**: `build-gate -- cargo test --locked --all-features --all-targets --workspace --jobs 3` → all pass.

## Test plan

- New tests: the three groups in Step 3, in `crates/fujicli-dev/`, driven by the existing fake `ProbeIo` recorder.
- Existing to keep green: all current `fujicli-dev` tests (gate refusal, allowlist, decision table, sequence ordering) and the full workspace suite.

## Done criteria

ALL must hold:

- [ ] `rg -n "attempted" crates/fujicli-dev/src/probe.rs` still shows the pre-write record written BEFORE the write sequence
- [ ] The terminal append is reached on both the success and the failure path (inspect the control flow; a test covers each)
- [ ] `AuditRecord`'s field list and `to_json`'s key list are unchanged (`git diff 46f2e5e -- crates/fujicli-dev/src/audit.rs` shows no added struct field or JSON key)
- [ ] `cargo fmt --all --check` exits 0
- [ ] `build-gate -- cargo clippy --locked --all-features --all-targets --workspace --jobs 3 -- -D warnings` exits 0
- [ ] `build-gate -- cargo test --locked --all-features --all-targets --workspace --jobs 3` exits 0; the new tests exist and pass
- [ ] `git status` shows no modified files outside the in-scope list
- [ ] `plans/README.md` status row updated (SKIPPED per reviewer override)

## STOP conditions

Stop and report if:

- The probe or audit code no longer matches the excerpts (drift since `46f2e5e`).
- Classifying the terminal outcome would require adding a field to `AuditRecord`/`to_json` — the allowlist is a contract; report what you would need instead.
- You cannot append the terminal record on the failure path without swallowing or altering the original error — report the obstacle.
- You find yourself tempted to log an error message, path, or camera string into the audit record — explicitly forbidden; stop.

## Maintenance notes

- The outcome vocabulary is now part of the audit-log contract; adding a value is fine, renaming or removing one is a breaking change for anyone parsing the log.
- Two lines per attempt is the new normal shape; a reviewer should check that any future early return between the two appends still writes the terminal line.
