# Plan 012: Add an informational coverage job to CI

> **Executor instructions**: Follow this plan step by step. Run every verification command and confirm the expected result before moving on. Touch only the file listed as in scope. If any STOP condition occurs, stop and report — do not improvise. Commit your work in the worktree following the plan's git workflow section. SKIP updating `plans/README.md` — your reviewer maintains the index. Before reporting, audit every claim against an actual tool result. You cannot run GitHub Actions locally; be explicit in your report about which checks were static-only.
>
> **Drift check (run first)**: `git diff --stat 46f2e5e..HEAD -- .github/workflows/ci.yml`

## Status

- **Priority**: P3
- **Effort**: S
- **Risk**: LOW
- **Depends on**: none
- **Category**: dx
- **Planned at**: commit `46f2e5e`, 2026-08-30

## Why this matters

The CI gate is rigorous about correctness (check, fmt, clippy `-D warnings`, doc, test, udeps, plus a separate security workflow with cargo-deny, dependency-review, actionlint, zizmor) but reports no coverage at all. The 2026-08-30 audit found real blind spots — four untested rejection branches in the RAF validator, an entirely untested codegen collector — that were only visible by reading source by hand. A coverage summary would have surfaced them immediately. This adds an **informational, non-blocking** job: the goal is visibility, not a threshold to game.

## Current state

`.github/workflows/ci.yml` has two jobs: `rust` (`name: CI / Rust`) and `platform` (`name: CI / Platform (${{ matrix.name }})`, a macOS/Windows matrix). The `rust` job's shape, which the new job mirrors:

- `actions/checkout` pinned by full commit SHA with a `# v7.0.1` trailing comment;
- an "Install native dependencies" step (libusb etc.) and an "Install CUE" step — **both are required**, because `build.rs` shells out to `cue export` and the crate links libusb;
- tools installed via `taiki-e/install-action` pinned by full SHA (`# v2.87.1`) — used for `cargo-udeps` and `typos`;
- cargo invocations use `--locked --all-features --all-targets --workspace --jobs 4`.

The repo's CI conventions to match exactly: every third-party action pinned by full commit SHA with a version comment; offline-first posture (no external upload services); `permissions: contents: read` at the workflow level.

## Commands you will need

| Purpose | Command | Expected on success |
|---------|---------|---------------------|
| Workflow lint | `actionlint .github/workflows/ci.yml` | exit 0, no findings |
| YAML sanity | `rg -n "coverage" .github/workflows/ci.yml` | the new job present |

`actionlint` is available on this machine via mise (`actionlint` on PATH, or `mise which actionlint`). If it is genuinely unavailable, say so in your report rather than claiming the workflow was linted.

Local coverage run (optional, only to validate the command itself — it is slow and needs the toolchain): `cargo llvm-cov --workspace --all-features --locked --summary-only`. `cargo-llvm-cov` may not be installed locally; do NOT install it. If you cannot run it, verify the flags against `cargo llvm-cov --help` only if the binary exists, and otherwise report that the command was not executed locally.

## Scope

**In scope** (the only file you should modify):

- `.github/workflows/ci.yml`

**Out of scope** (do NOT touch):

- `.github/workflows/security.yml` and any other workflow.
- The existing `rust` and `platform` jobs — add a new job; do not restructure or add steps to them.
- Any coverage threshold, gate, or required-status change: the job must never fail the build on low coverage.
- External coverage services (Codecov and friends) — the repo is offline-first; upload nothing.
- Repository settings / branch protection (not files in this repo).

## Git workflow

- Branch: `advisor/012-ci-coverage-job`
- Conventional commit. Suggested: `ci: add informational coverage job`.
- Do NOT push or open a PR.

## Steps

### Step 1: Add the coverage job

Append a third job to `.github/workflows/ci.yml`, named `coverage` with `name: CI / Coverage`, running on the same runner as the `rust` job. It must:

1. check out the repo using the **same pinned `actions/checkout` SHA and version comment** already used in this file (copy it verbatim — do not bump or re-pin);
2. install native dependencies and CUE by copying the existing steps from the `rust` job verbatim (both are needed for `build.rs` and linking);
3. install `cargo-llvm-cov` via `taiki-e/install-action`, reusing the **same pinned action SHA and version comment** as the existing `cargo-udeps`/`typos` steps, with the tool named `cargo-llvm-cov`;
4. run `cargo llvm-cov --workspace --all-features --locked --jobs 4 --summary-only` and write the summary into the job summary (`>> "$GITHUB_STEP_SUMMARY"`), so the numbers are visible without downloading anything;
5. be explicitly non-blocking: set `continue-on-error: true` on the job so a coverage tooling failure never turns CI red.

Do not add `permissions` beyond what the workflow already grants, do not upload artifacts to third parties, and do not add a threshold flag (`--fail-under-lines` and friends).

**Verify**: `actionlint .github/workflows/ci.yml` → exit 0 with no findings.

### Step 2: Confirm nothing else changed

**Verify**: `git diff 46f2e5e -- .github/workflows/ci.yml` shows only the added job — no edits inside the `rust` or `platform` jobs, no action SHA changes anywhere.

## Test plan

- Static: `actionlint` clean; the diff is additive and touches no existing job.
- There is no meaningful local execution of a GitHub Actions job. State plainly in your report that the job's real behavior is unverified until it runs on CI, and that it is `continue-on-error` so a first-run failure cannot break the gate.

## Done criteria

ALL must hold:

- [ ] `.github/workflows/ci.yml` contains a `coverage` job with `continue-on-error: true` and no coverage threshold flag
- [ ] The new job's `actions/checkout` and `taiki-e/install-action` references use the exact SHAs already present in the file (`rg -n "uses:" .github/workflows/ci.yml` shows no new distinct SHA for these two actions)
- [ ] `actionlint .github/workflows/ci.yml` exits 0 (or your report states clearly that actionlint was unavailable)
- [ ] `git diff 46f2e5e -- .github/workflows/ci.yml` shows additions only; the `rust` and `platform` jobs are unchanged
- [ ] `git status` shows only `.github/workflows/ci.yml` modified
- [ ] `plans/README.md` status row updated (SKIPPED per reviewer override)

## STOP conditions

Stop and report if:

- `ci.yml` no longer matches the described structure (drift since `46f2e5e`).
- `taiki-e/install-action` does not support `cargo-llvm-cov` at the pinned version — report; do NOT bump the action's pinned SHA to work around it, and do NOT `cargo install` the tool in CI as a substitute without saying so.
- Making the job work appears to require new workflow `permissions`, a third-party upload, or changes to an existing job — report instead.

## Maintenance notes

- The job is informational by design. If the project ever wants a real gate, that is a separate decision: it needs a baseline, a threshold policy, and a plan for the untestable device paths (the runtime cannot be exercised without a camera, so raw line coverage will always understate quality here).
- Coverage will read low for `src/lib/ptp/**` and `src/lib/camera.rs` because the transport layer has no fake-transport seam yet; that gap is tracked separately as the `preflight::run()` seam finding.
