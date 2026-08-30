# Plan 007: Build the gated 0xD18C still/movie namespace probe tooling (spike — no device run)

> **Executor instructions**: This is a design/spike plan: it builds and tests the probe TOOLING and its run procedure, and stops there. The physical-device run and any FML status change are explicitly a human maintainer step — see STOP conditions. Follow the steps in order, run every verification command, and if anything in "STOP conditions" occurs, stop and report. When done, update the status row in `plans/README.md`.
>
> **Drift check (run first)**: `git diff --stat 4c6fd8c..HEAD -- crates/fujicli-dev docs/contributors/reversing.md tests/xt5_simulation_domain.rs`
> If any in-scope file changed since this plan was written, compare the "Current state" excerpts against the live code before proceeding; on a mismatch, treat it as a STOP condition.

## Status

- **Priority**: P3
- **Effort**: L (tooling M; the device run itself is maintainer time, not executor effort)
- **Risk**: MED (mitigated: the executor never touches hardware; the tool itself enforces the documented safety protocol)
- **Depends on**: plans/005-latch-interrupts-around-camera-writes.md is NOT a dependency (fujicli-dev has its own entry point), but read it for the latch pattern if reusing.
- **Category**: direction
- **Planned at**: commit `4c6fd8c`, 2026-08-30
- **Execution note (2026-08-30)**: BLOCKED at Step 3 by its own STOP conditions, both confirmed real. (1) No library primitive can reach `0xD18C`: `Ptp::set_prop_raw` is `pub(super)`, and the only `MutationPermit` path (`preflight::run`) requires `ModelBindingKind::Native` plus a `Verified` profile — which cannot exist before this probe runs; adding a bypass reopens the `124aa4f` seal and needs explicit maintainer sign-off. (2) No PTP property distinguishing still vs movie custom-setting state is derivable from `fml/`, `docs/`, or `support/`; a physical run may need human LCD observation instead of a wire-level signal. Steps 1-2 landed on `advisor/007-simulation-namespace-probe` (`b409be9`): design note with decision table and both open questions in `docs/contributors/reversing.md`, plus a `dangerous-reverse-engineering`-gated command skeleton that refuses to run with an explanatory error. Refusal-only: does NOT close this plan.

- **Maintainer decisions (2026-08-30)**: both STOP-condition questions were answered by the maintainer.
  1. **Raw single-property primitive: SANCTIONED**, strictly probe-scoped. A new `Camera` method (working name `reverse_probe_write_single_property`) performing one `SetDevicePropValue`/`GetDevicePropValue` round trip against `Ptp` without going through `preflight::run`'s `MutationPermit`/`Verified`-profile requirement, gated behind BOTH `reverse-tools` AND `dangerous-reverse-engineering` so it cannot compile into a default-features distributable. Single send, no auto-retry. This is a deliberate, narrowly-scoped reopening of the `124aa4f` seal, authorized only for this probe. A follow-up plan (008) must specify and implement it under the diff-acceptance discipline for sealed-mutation-surface changes; do NOT widen it beyond the single-property round trip.
  2. **Observable resolution: BOTH approaches, capture-first.** Before any of our own mutating write, run an X RAW Studio USB-traffic capture session — the official software may itself touch `0xD18C` and reveal the still/movie signal on the wire with no dangerous write from us. If the capture does not resolve which namespace `0xD18C` addresses, fall back to the probe with the LCD-observation protocol (PTP-level log paired with human before/after reads of the camera's own C1-C7 menu in still and movie modes). The capture step is preferred because it is non-mutating; the probe is the fallback.

  Net status stays BLOCKED on the physical-device work (capture and, if needed, the sanctioned probe run against an X-T5 with a recovery plan), but both design blockers are now resolved. Next executable step is a plan 008 that specifies the sanctioned primitive and the revised Step 3 guard sequence; the device run itself remains a maintainer step.

## Why this matters

Film-simulation commands are disabled on the X-T5 — the project's only fully verified camera — solely because no captured evidence establishes whether PTP selector `0xD18C` addresses the still or the movie C1–C7 custom-setting namespace (`docs/users/support.md:55-57`; enforced fail-closed by `tests/xt5_simulation_domain.rs:5-28`). The maintainers have already written the exact safety protocol any such probe must follow (`docs/contributors/reversing.md:107-133`); what does not exist is the tool implementing it. Building the tool (without running it) converts the project's single biggest feature blocker from an open-ended research task into a one-session physical-device run.

## Current state

- `docs/contributors/reversing.md:107-133` — "Requirements for Any Future Dangerous Probe". The load-bearing contract, quoted:
  - add it only to `fujicli-dev` behind a new `dangerous-reverse-engineering` feature and a command-specific guard;
  - before the first mutating PTP container the command must: (1) require exact `--device BUS.ADDRESS` and reject emulation/auto-selection; (2) display USB bus/address, VID:PID, PTP manufacturer/model/firmware, and a SHA-256 serial fingerprint, never the raw serial; (3) require exact confirmation of that live serial fingerprint; (4) create, validate, sync, and hash a fresh no-clobber pre-backup; (5) require a fixed command-specific acknowledgement string; (6) durably record an audit attempt, then send the mutating operation once;
  - automatic retry is forbidden; ambiguous results print `DO NOT RETRY AUTOMATICALLY`; selector experiments touch only one explicit slot per invocation and must snapshot, restore, and verify the prior selector once;
  - the audit log is restrictive append-only JSONL with bounded metadata only (no raw serials, argv, full paths, payloads, or full error chains — the doc lists the exact allowed fields).
- `crates/fujicli-dev/` — the existing dev tool: `src/main.rs` (clap entry), `src/reverse.rs` (~130 lines: `DiscoverCommand`/`BackupCommand` enums, `ProbeSummary` accounting, `open`/`info`/`simulation`/`backup`/`render_profile` handlers, `handle` dispatcher), `src/usb.rs`, `src/output.rs` (`NewOutput` no-clobber writer), `src/log.rs`. It builds with `required-features = ["reverse-tools"]` and depends on `fujicli = { path = "../..", default-features = false }` (`crates/fujicli-dev/Cargo.toml`). Today's `discover simulation` reads only the currently selected slot and never writes `0xD18C` — it structurally cannot distinguish the namespaces.
- `tests/xt5_simulation_domain.rs:5-28` — asserts `SimulationAccess`/`SimulationWrite` preflight profiles for X-T5 fw 4.31 are `CameraPreflightProfileStatus::Unverified`. This test encodes the fail-closed invariant and MUST NOT be changed by this plan; it only flips after verified physical evidence lands in `fml/` (maintainer step, out of scope).
- The library side: mutation permits and preflight live in `src/lib/preflight.rs` (permit activation bound to `(bus, address, interface)`), backup export machinery in `src/lib/features/backup/`, and the transient custom-setting selector write is the operation whose namespace is unknown. What the probe must observe: after writing one explicit slot value to `0xD18C`, read back the still-mode and movie-mode custom-setting state to see which namespace changed (the exact read-back properties are an open question below).
- Repo rules that bind this work (`AGENTS.md`): never invent PTP property codes or capabilities; do not widen any production camera claim; distributable binaries use default features so nothing here may leak outside `fujicli-dev` + the new feature flag; CUE schema in `fml/` is the only place camera facts live.
- `build-gate` wraps compiler-backed cargo commands on the maintainer's Mac; if unavailable, run the inner command directly with the same `--jobs` ceiling.

## Commands you will need

| Purpose | Command | Expected on success |
|---------|---------|---------------------|
| Format check | `cargo fmt --all --check` | exit 0 |
| Dev-crate build | `build-gate -- cargo build --locked -p fujicli-dev --features reverse-tools,dangerous-reverse-engineering --jobs 4` | exit 0 |
| Dev-crate tests | `build-gate -- cargo test --locked -p fujicli-dev --features reverse-tools,dangerous-reverse-engineering --jobs 4` | all pass |
| Default-features build (leak check) | `build-gate -- cargo build --locked --workspace --jobs 4` | exit 0, no new code compiled into `fujicli` |
| Full gate | `build-gate -- cargo test --locked --all-features --all-targets --workspace --jobs 4` | all pass |

## Scope

**In scope**:

- `crates/fujicli-dev/Cargo.toml` (new `dangerous-reverse-engineering` feature, depending on `reverse-tools`)
- `crates/fujicli-dev/src/` (new probe command module, e.g. `src/probe.rs`, wired into `main.rs`; audit-log writer)
- `src/lib/` ONLY if the probe needs a narrowly-scoped library hook that does not exist (e.g. a raw single-property write gated behind a feature) — see STOP conditions first
- `docs/contributors/reversing.md` (append the concrete run procedure for this probe)

**Out of scope** (do NOT touch):

- `fml/**` — flipping `SimulationAccess`/`SimulationWrite` to `Verified` requires captured physical evidence that will not exist at the end of this plan.
- `tests/xt5_simulation_domain.rs` — the fail-closed invariant stays.
- `src/cli/**` and any production `fujicli` behavior under default features.
- `docs/users/support.md` — the support table changes only after a physical-device run.

## Git workflow

- Branch: `advisor/007-simulation-namespace-probe`
- Conventional commits (repo example: `fix: isolate reverse engineering tooling`). Suggested: `feat: add gated simulation namespace probe tooling`.
- Do NOT push or open a PR unless the operator instructed it.

## Steps

### Step 1: Design note first (in-repo, small)

Add a short design section to `docs/contributors/reversing.md` (below the "Requirements" section) describing the probe: name (`fujicli-dev probe simulation-namespace`), what it writes (one explicit C1–C7 slot value to `0xD18C`, exactly once), what it reads before/after (snapshot of the current selector plus the still/movie custom-setting observables), and the decision table (which observation implies "still namespace" vs "movie namespace" vs "ambiguous → DO NOT RETRY AUTOMATICALLY"). Where the read-back observables are not derivable from existing code/docs, list them explicitly as OPEN QUESTIONS for the maintainer rather than inventing property codes — inventing codes violates `AGENTS.md`.

**Verify**: `rg -n "simulation-namespace" docs/contributors/reversing.md` → matches; markdown lint via `cargo fmt --all --check` unaffected (docs only).

### Step 2: Feature gate and command skeleton

In `crates/fujicli-dev/Cargo.toml`, add `dangerous-reverse-engineering = ["reverse-tools"]`. Add the `probe simulation-namespace` clap subcommand, compiled only under `#[cfg(feature = "dangerous-reverse-engineering")]`, requiring `--device BUS.ADDRESS` (reject absence — no auto-selection; reuse the `Location` plumbing in `src/usb.rs` but make the device argument mandatory for this command) and rejecting any emulation flag if `fujicli-dev` exposes one.

**Verify**: dev-crate build command → exit 0 with the feature; `build-gate -- cargo build --locked -p fujicli-dev --features reverse-tools --jobs 4` → exit 0 and `probe` absent from `--help` output without the feature.

### Step 3: Implement the six-step guard sequence

Implement the pre-mutation sequence exactly as quoted in "Current state" from `reversing.md:112-121`: identity display with SHA-256 serial fingerprint (never raw serial), interactive fingerprint confirmation, fresh no-clobber pre-backup (reuse `fujicli-dev`'s existing backup path via `NewOutput`), the fixed acknowledgement string (choose one, e.g. `I-UNDERSTAND-THIS-WRITES-SELECTOR-D18C`, and hard-code it), and the append-only JSONL audit record with only the fields the doc allows (`reversing.md:128-133`). Then: snapshot the current selector state, send the single mutating write, read back observables, restore the prior selector once, verify the restore, and print the decision-table verdict. Any timeout/disconnect/malformed response anywhere after the mutating send prints `DO NOT RETRY AUTOMATICALLY` and exits non-zero without any retry.

If a library primitive for the raw single-property write/read does not exist and cannot be added without widening production surface, STOP (see conditions).

**Verify**: dev-crate tests command → all pass, including new unit tests for: the acknowledgement/fingerprint gate refusing wrong input, audit-record field allowlist (serialize a record, assert absent fields: raw serial, argv, paths, payloads), and the decision table (pure function over observed before/after states).

### Step 4: Leak check and full gate

Confirm nothing new compiles into the distributable binary: default-features workspace build, then `rg -n "dangerous-reverse-engineering" src/ Cargo.toml` → no matches outside `crates/fujicli-dev` (and `src/lib` only if a STOP-approved hook was added under the feature).

**Verify**: full-gate test command → all pass; `build-gate -- cargo build --locked --release --workspace --jobs 2` → exit 0 (default features, per `AGENTS.md` distribution rule).

## Test plan

- Unit tests in `fujicli-dev` (Step 3 list): gate refusal, audit-record allowlist, decision-table pure function, snapshot/restore ordering via a fake sequence recorder if the command's I/O is factored into a trait (factor it that way — the existing `ProbeSummary::observe` pattern in `src/reverse.rs:35-49` shows the crate's accounting style).
- No test may perform device I/O. There is no fixture for `0xD18C` behavior — that is the point of the probe.
- Verification: the dev-crate test command → all pass.

## Done criteria

- [ ] `build-gate -- cargo build --locked -p fujicli-dev --features reverse-tools,dangerous-reverse-engineering --jobs 4` exits 0; without the new feature the probe command does not exist
- [ ] Unit tests for gate, audit allowlist, and decision table exist and pass
- [ ] `tests/xt5_simulation_domain.rs` is byte-identical to its state at `4c6fd8c` (`git diff 4c6fd8c -- tests/xt5_simulation_domain.rs` → empty)
- [ ] `fml/` untouched (`git diff 4c6fd8c -- fml/` → empty)
- [ ] `docs/contributors/reversing.md` documents the run procedure and open questions
- [ ] Full gate and default-features release build pass
- [ ] `plans/README.md` status row updated — status at best `DONE (tooling)`; note that the device run remains open

## STOP conditions

Stop and report back (do not improvise) if:

- The probe cannot be implemented without adding a raw property-write primitive to `src/lib` that would be reachable under default features or outside the `dangerous-reverse-engineering` gate — report the exact API you would need and wait for maintainer review (recent commit `124aa4f "fix: seal raw PTP mutation access"` shows this surface was deliberately closed; do not reopen it unilaterally).
- You cannot determine the read-back observables (which properties distinguish still vs movie custom-setting state) from existing code, captures under `support/`, or docs — record them as open questions in the design note and stop after Step 2; do not invent PTP codes.
- Anything requires editing `fml/`, `tests/xt5_simulation_domain.rs`, or `docs/users/support.md`.
- You find yourself about to run the probe against hardware. Never do this — the physical run is a maintainer-only step with a recovery plan.

## Maintenance notes

- After the maintainer's device run: captured evidence goes to the reversing workflow, `fml/` gains the verified still/movie selector facts, `tests/xt5_simulation_domain.rs` flips to assert the new verified status, and `docs/users/support.md` updates the X-T5 Simulations cell — all outside this plan.
- The audit-log JSONL format should stay stable once first used; treat its field set as a contract.
- If plan 005 (interrupt latch) landed, consider reusing its latch semantics around the probe's single mutating send inside `fujicli-dev`.
