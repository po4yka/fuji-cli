# Plan 002: Sanitize untrusted PTP device strings before they reach terminal error output

> **Executor instructions**: Follow this plan step by step. Run every verification command and confirm the expected result before moving to the next step. If anything in the "STOP conditions" section occurs, stop and report — do not improvise. When done, update the status row for this plan in `plans/README.md`.
>
> **Drift check (run first)**: `git diff --stat 4c6fd8c..HEAD -- src/lib/preflight.rs src/main.rs src/lib/features/backup/artifact.rs`
> If any in-scope file changed since this plan was written, compare the "Current state" excerpts against the live code before proceeding; on a mismatch, treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: S
- **Risk**: LOW
- **Depends on**: none
- **Category**: security
- **Planned at**: commit `4c6fd8c`, 2026-08-30

## Why this matters

The PTP `GetDeviceInfo` strings (`manufacturer`, `model`, `device_version`) are decoded from arbitrary device-supplied UTF-16 (`PtpString::read_options` in `src/lib/ptp/codec.rs:221-253` accepts any code units, including control and ANSI-escape characters). `src/lib/preflight.rs` interpolates these raw strings into `bail!`/`ensure!` messages, and `src/main.rs` prints the resulting `anyhow` error chain to stderr with no filtering. A USB device spoofing the public FUJIFILM X-T5 VID/PID can therefore force a preflight failure (e.g. report an unsupported firmware string) and inject terminal control sequences into the user's terminal — output spoofing or exploitation of a vulnerable terminal emulator. The repo already has the right sanitization idea in `src/lib/features/backup/artifact.rs::validate_identity_text`, but it runs only when building a `BackupArtifact`, after these earlier preflight error paths.

## Current state

- `src/lib/preflight.rs` — preflight validation; the offending interpolation sites:
  - `preflight.rs:506-515` — `bail!("firmware {firmware} is not in the {:?} compatibility matrix for {} ...")` where `firmware` is the raw `info.device_version` string.
  - `preflight.rs:517-524` — `ensure!` messages `"ambiguous preflight profiles for firmware {firmware} ..."` and `"firmware {firmware} has only an unverified {operation:?} profile"`.
  - `preflight.rs:534-537` — `select_capability_profile` interpolates `firmware` into its error.
  - `preflight.rs:560-571` — `validate_device_info` interpolates raw `info.manufacturer` and `info.model` on mismatch:

```rust
// src/lib/preflight.rs:560-571
    ensure!(
        info.manufacturer == identity.manufacturer,
        "PTP manufacturer mismatch: expected {}, received {}",
        identity.manufacturer,
        info.manufacturer
    );
    ensure!(
        info.model == identity.model,
        "PTP model mismatch: expected {}, received {}",
        identity.model,
        info.model
    );
```

- `src/main.rs:34-36` — `main()` returns `anyhow::Result<()>`; the error chain is printed to stderr by the standard `Termination` impl with no filtering. Do not change `main.rs` in this plan; fixing at the interpolation source is the correct layer.
- The existing exemplar for rejecting control characters (used for stored identity, stricter than needed for display):

```rust
// src/lib/features/backup/artifact.rs:277-288
fn validate_identity_text(value: &str, description: &str) -> anyhow::Result<()> {
    ensure!(!value.trim().is_empty(), "{description} is empty");
    ensure!(
        value.len() <= MAX_IDENTITY_TEXT_BYTES,
        "{description} exceeds {MAX_IDENTITY_TEXT_BYTES} bytes"
    );
    ensure!(
        !value.chars().any(char::is_control),
        "{description} contains control characters"
    );
    Ok(())
}
```

- Conventions: `anyhow::Result` with contextual errors at boundaries; no panics on device input; workspace lints deny `unwrap_used`/`panic`. Error-message tests in this repo assert on substrings via `error.to_string().contains(...)` — see the tests module at the bottom of `src/lib/preflight.rs` (e.g. `unknown_firmware_fails_closed` at `preflight.rs:928`). Rust is formatted with the repo `rustfmt.toml`.
- Some Cargo commands on the maintainer's Mac go through a machine-wide `build-gate` wrapper. If `build-gate` is not on `PATH`, run the inner cargo command directly with the same `--jobs` ceiling.

## Commands you will need

| Purpose | Command | Expected on success |
|---------|---------|---------------------|
| Format check | `cargo fmt --all --check` | exit 0 |
| Focused tests | `build-gate -- cargo test --locked -p fujicli --lib preflight --jobs 4` | all pass |
| Workspace build | `build-gate -- cargo build --locked --workspace --jobs 4` | exit 0 |
| Clippy | `build-gate -- cargo clippy --locked --all-features --all-targets --workspace --jobs 4 -- -D warnings` | exit 0 |

## Scope

**In scope** (the only files you should modify):

- `src/lib/preflight.rs`

**Out of scope** (do NOT touch, even though they look related):

- `src/lib/features/backup/artifact.rs` — its `validate_identity_text` is a strict validator for stored identity, not a display sanitizer; leave it.
- `src/lib/ptp/codec.rs` — `PtpString` must keep decoding faithfully; sanitization belongs at display boundaries, not the wire codec.
- `src/main.rs` — no global stderr filtering; fix at the source.
- Any change to which conditions pass or fail preflight — this plan only changes how failure messages render untrusted strings.

## Git workflow

- Branch: `advisor/002-sanitize-preflight-strings`
- Conventional commits, imperative mood, subject under 72 chars (repo examples: `fix: seal raw PTP mutation access`, `fix: harden PTP bulk framing`). Suggested: `fix: sanitize device strings in preflight errors`.
- Do NOT push or open a PR unless the operator instructed it.

## Steps

### Step 1: Add a display sanitizer helper

In `src/lib/preflight.rs`, near the other private helpers (e.g. above `validate_physical_identity` at `preflight.rs:541`), add:

```rust
const MAX_DISPLAY_TEXT_CHARS: usize = 64;

/// Renders an untrusted device-supplied string safely for terminal error
/// messages: strips control characters (ANSI escapes included) and caps
/// length so a spoofed device cannot inject terminal sequences via stderr.
fn sanitize_for_display(value: &str) -> String {
    let mut sanitized: String = value
        .chars()
        .filter(|character| !character.is_control())
        .take(MAX_DISPLAY_TEXT_CHARS)
        .collect();
    if value.chars().filter(|character| !character.is_control()).count() > MAX_DISPLAY_TEXT_CHARS {
        sanitized.push_str("...");
    }
    sanitized
}
```

(Exact shape may be simplified — e.g. collect once into a `Vec<char>` to avoid the double pass — but the behavior must be: control characters removed, at most `MAX_DISPLAY_TEXT_CHARS` characters kept, truncation marked.)

**Verify**: `cargo fmt --all --check` → exit 0 (after running `cargo fmt --all` if needed).

### Step 2: Apply it at every interpolation of device-supplied strings in `preflight.rs`

Wrap the untrusted value at each site found in "Current state" — pass `sanitize_for_display(firmware)`, `sanitize_for_display(&info.manufacturer)`, `sanitize_for_display(&info.model)` into the message formatting. Sites: `preflight.rs:506-515`, `preflight.rs:517-524`, `preflight.rs:534-537`, `preflight.rs:560-571`. Then sweep the whole file for any other `bail!`/`ensure!`/`anyhow!` interpolating `info.` fields or `firmware` (`rg -n 'firmware|info\.' src/lib/preflight.rs` and inspect each match); apply the helper to every device-sourced string, not only the listed sites. Values coming from the static generated schema (`camera.name`, `identity.manufacturer`, `identity.model`, `profile.firmware`) are trusted and must NOT be wrapped.

**Verify**: `build-gate -- cargo build --locked --workspace --jobs 4` → exit 0.

### Step 3: Add regression tests

In the existing `#[cfg(test)] mod tests` of `src/lib/preflight.rs`, add unit tests for `sanitize_for_display`:

- a string containing an ESC character (`"\u{1b}[31mEVIL\u{1b}[0m4.31"`) comes back with no `char::is_control` characters and still contains `"EVIL"` and `"4.31"`;
- a 200-character string is truncated to `MAX_DISPLAY_TEXT_CHARS` characters plus the `...` marker;
- a clean string like `"4.31"` passes through unchanged.

Also confirm no existing test in the module asserts on a message substring that sanitization would now alter (they assert on plain-ASCII fixture values, which pass through unchanged).

**Verify**: `build-gate -- cargo test --locked -p fujicli --lib preflight --jobs 4` → all pass, including the new tests.

## Test plan

- New tests: the three `sanitize_for_display` cases above, in `src/lib/preflight.rs` tests module, modeled structurally on the existing small assertion-style tests there (e.g. `unknown_firmware_fails_closed`, `preflight.rs:928`).
- Verification: `build-gate -- cargo test --locked -p fujicli --lib preflight --jobs 4` → all pass.

## Done criteria

Machine-checkable. ALL must hold:

- [ ] `cargo fmt --all --check` exits 0
- [ ] `build-gate -- cargo clippy --locked --all-features --all-targets --workspace --jobs 4 -- -D warnings` exits 0
- [ ] `build-gate -- cargo test --locked --all-features --all-targets --workspace --jobs 4` exits 0; new sanitizer tests exist and pass
- [ ] Every `bail!`/`ensure!`/`anyhow!` in `src/lib/preflight.rs` that interpolates `info.manufacturer`, `info.model`, or a `firmware` value derived from `info.device_version` goes through `sanitize_for_display` (check with `rg -n 'info\.(manufacturer|model)|firmware' src/lib/preflight.rs`)
- [ ] `git status` shows no modified files outside the in-scope list
- [ ] `plans/README.md` status row updated

## STOP conditions

Stop and report back (do not improvise) if:

- The code at the cited `preflight.rs` lines doesn't match the excerpts (drift since `4c6fd8c`).
- An existing test fails because it asserted on a raw control character or a >64-char device string in a message — that would mean sanitization is changing a contract someone depends on; report instead of weakening the sanitizer.
- You find device-sourced strings interpolated into user-facing errors in files other than `preflight.rs` — note them in your report as follow-up candidates; do not expand scope.

## Maintenance notes

- Any future `bail!`/`ensure!` in preflight (or elsewhere) that renders a `DeviceInfo` string must use this helper; a reviewer should watch for new raw interpolations of `info.*` fields.
- Deliberately deferred: auditing non-preflight display paths (e.g. `device info` output, log lines in `camera.rs`) for the same pattern — worth a follow-up sweep, kept out of scope here to keep the diff small.
