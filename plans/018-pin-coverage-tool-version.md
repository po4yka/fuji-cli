# Plan 018: Pin the cargo-llvm-cov version in the CI coverage job

> **Executor instructions**: Follow this plan step by step. Run every verification command and confirm the expected result before moving on. Touch only the file listed as in scope. If any STOP condition occurs, stop and report — do not improvise. Commit your work in the worktree following the plan's git workflow section. SKIP updating `plans/README.md` — your reviewer maintains the index. You cannot run a GitHub Actions job locally; be explicit about what was static-only.
>
> **Drift check (run first)**: `git diff --stat d4746a6..HEAD -- .github/workflows/ci.yml`

## Status

- **Priority**: P3
- **Effort**: XS
- **Risk**: LOW
- **Depends on**: plan 012 (merged as `e693fc4` — the coverage job exists)
- **Category**: dx
- **Planned at**: commit `d4746a6`, 2026-08-30

## Why this matters

Every other tool this workflow installs is pinned to an exact version — `cargo-udeps@0.1.61`, `typos@1.50.0` — so a CI run is reproducible and an upstream release cannot silently change behavior. The coverage job added by plan 012 installs `cargo-llvm-cov` unpinned, because no version had been evidenced at authoring time. This closes that one inconsistency.

`cargo-llvm-cov` **0.9.0** is the current release on crates.io, confirmed via `cargo search cargo-llvm-cov` on 2026-08-30. That is the version to pin.

## Current state

`.github/workflows/ci.yml`, inside the `coverage` job (`name: CI / Coverage`, `continue-on-error: true`):

```yaml
      - name: Install cargo-llvm-cov
        uses: taiki-e/install-action@742a3317eac7bd62f91cd888b4eead5e784ba833 # v2.87.1
        with:
          tool: cargo-llvm-cov
          fallback: none
```

The sibling steps in the `rust` job of the same file show the pinned style to match — they pass `tool: <name>@<version>` to the same action. Read them before editing and copy their exact spelling.

## Commands you will need

| Purpose | Command | Expected on success |
|---------|---------|---------------------|
| Workflow lint | `actionlint .github/workflows/ci.yml` | exit 0, no findings |
| Confirm the pin | `rg -n "cargo-llvm-cov" .github/workflows/ci.yml` | shows the versioned tool spec |

`actionlint` is on PATH via mise. If it is genuinely unavailable, say so rather than claiming the workflow was linted.

## Scope

**In scope** (the only file you should modify):

- `.github/workflows/ci.yml` (only the `tool:` line in the coverage job's install step)

**Out of scope** (do NOT touch):

- The pinned `taiki-e/install-action` SHA and its version comment — unchanged.
- Any other job, step, or tool version in this or any other workflow.
- The `fallback: none` setting, the `continue-on-error: true` flag, and the coverage command itself.
- Adding a coverage threshold or an upload — still explicitly unwanted.

## Git workflow

- Branch: `advisor/018-pin-coverage-tool`
- Conventional commit. Suggested: `ci: pin the cargo-llvm-cov version`.
- Do NOT push or open a PR.

## Steps

### Step 1: Pin the version

Change the coverage job's install step to request `cargo-llvm-cov@0.9.0`, matching the exact spelling the sibling steps use for `cargo-udeps` and `typos`. Change nothing else.

**Verify**: `rg -n "cargo-llvm-cov" .github/workflows/ci.yml` → shows `cargo-llvm-cov@0.9.0`; `actionlint .github/workflows/ci.yml` → exit 0, no findings.

### Step 2: Confirm the diff is a one-liner

**Verify**: `git diff d4746a6 -- .github/workflows/ci.yml` → exactly one changed line (the `tool:` value); no other job, step, or SHA touched.

## Test plan

- Static only: `actionlint` clean and a one-line diff.
- The install step's real behavior (whether `taiki-e/install-action` has a prebuilt `cargo-llvm-cov@0.9.0` for the runner) can only be confirmed by an actual CI run. Say this plainly in your report. The job is `continue-on-error: true`, so a resolution failure cannot break the gate.

## Done criteria

ALL must hold:

- [ ] `rg -n "tool: cargo-llvm-cov@0.9.0" .github/workflows/ci.yml` matches
- [ ] `actionlint .github/workflows/ci.yml` exits 0 (or your report states actionlint was unavailable)
- [ ] `git diff d4746a6 -- .github/workflows/ci.yml` changes exactly one line
- [ ] `git status` shows only `.github/workflows/ci.yml` modified
- [ ] `plans/README.md` status row updated (SKIPPED per reviewer override)

## STOP conditions

Stop and report if:

- The coverage job no longer matches the excerpt (drift since `d4746a6`).
- The sibling steps turn out to pin versions by a different mechanism than `tool: name@version` — match whatever they actually do and report the difference.
- Pinning appears to require bumping the `taiki-e/install-action` SHA — do not bump it; report instead.

## Maintenance notes

- When bumping this pin later, bump it deliberately alongside the other tool pins, not automatically.
- If a future CI run shows `taiki-e/install-action` cannot resolve a prebuilt `0.9.0` for the runner, the options are pinning a different released version or dropping `fallback: none` — decide with the run's log in hand, not speculatively.
