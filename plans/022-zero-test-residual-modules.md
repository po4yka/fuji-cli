# Plan 022: Cover the last zero-test modules (PTP error display, dev-crate device address parsing)

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat f2a78d0..HEAD -- src/lib/ptp/error.rs crates/fujicli-dev/src/usb.rs`
> Note: the working tree at audit time already carried unrelated in-flight
> edits in `crates/fujicli-dev/src/firmware.rs`, `crates/fujicli-dev/src/main.rs`,
> and a new `crates/fujicli-dev/src/surface.rs`. Those files are out of scope
> here; their state is irrelevant to this plan. Only the two in-scope files
> matter for the excerpt check.

## Status

- **Priority**: P3
- **Effort**: S
- **Risk**: LOW
- **Depends on**: none
- **Category**: tests
- **Planned at**: commit `f2a78d0`, 2026-09-03

## Why this matters

Two small modules carry user-visible behavior with zero adjacent tests:

1. `src/lib/ptp/error.rs` renders PTP response codes into error text. Every
   failed camera operation surfaces through `Display`, so its exact format
   is part of the CLI's error contract. Nothing pins it.
2. `crates/fujicli-dev/src/usb.rs` parses the `--device BUS.ADDRESS`
   argument. Every dev-crate command requires it, so a parsing regression
   breaks the whole gated tool silently.

Both are cheap to test and currently invisible to the suite. This plan adds
plain unit tests to both files.

## Current state

### `src/lib/ptp/error.rs` (53 lines, verified in full at commit `f2a78d0`)

- `pub enum Error { Response(u16), Malformed(String), Usb(rusb::Error), Io(io::Error) }`
  (lines 5-11).
- `Display` (lines 13-26):
  - `Response(r)` resolves `ResponseCode::try_from(r)`; a known code prints
    `"{name:?} (0x{r:04x})"` — the debug name, lowercase hex, four digits.
    An unknown code prints `"Unknown (0x{r:04x})"`.
  - `Usb` prints `"USB error: {e}"`; `Io` prints `"IO error: {e}"`;
    `Malformed` passes the string through.
- `From<io::Error>` (lines 44-52): `ErrorKind::UnexpectedEof` becomes
  `Malformed("Unexpected end of message")`; every other kind stays `Io`.
- `From<rusb::Error>` (lines 38-42): wraps as `Usb`.
- `ResponseCode` is defined in `src/lib/ptp/container.rs:139-173`
  (`Ok = 0x2001`, `DeviceBusy = 0x2019`, and so on). `0xffff` is not a
  member, so it exercises the `Unknown` arm.

### `crates/fujicli-dev/src/usb.rs` (42 lines, verified in full)

- `pub struct Location { pub bus: u8, pub address: u8 }` (lines 5-9).
- `FromStr` (lines 11-27): splits on the first `.`; both halves must parse
  as `u8`. Error messages name the bad half: `"invalid device format"`,
  `"invalid USB bus number"`, `"invalid USB address"`.
- `Display` (lines 29-33): `"{bus}.{address}"`.
- `exact_device` (lines 35-42) enumerates real USB — hardware-bound, not
  unit-testable; out of scope.

Repo conventions that apply:

- Workspace lints deny `panic`, `unwrap_used`, `todo`, `unimplemented`
  (root `Cargo.toml` lines 15-40). Tests use `.expect("...")` or match on
  the `Result`.
- Error messages are user-facing contracts: assert exact strings where the
  string is fully owned by this crate, and prefix/suffix assertions only
  where a dependency (`rusb`, `std::io`) owns part of the text.
- Compiler-backed Cargo commands on the managed Mac run under `build-gate`
  with at most four jobs (`AGENTS.md`); without `build-gate`, run the inner
  command directly.

## Commands you will need

| Purpose | Command | Expected on success |
|---|---|---|
| PTP error tests | `build-gate -- cargo test --locked -p fujicli --lib ptp::error --jobs 4` | all pass, 6 new tests |
| Dev-crate tests | `build-gate -- cargo test --locked -p fujicli-dev --jobs 4` | all pass, 7 new tests |
| Formatting | `cargo fmt --all --check` | exit 0 |
| Lint (both crates) | `build-gate -- cargo clippy --locked --all-features --all-targets --workspace --jobs 4 -- -D warnings` | exit 0 |

## Scope

**In scope** (the only files you should modify):
- `src/lib/ptp/error.rs` — append a `#[cfg(test)] mod tests` block only
- `crates/fujicli-dev/src/usb.rs` — append a `#[cfg(test)] mod tests` block only

**Out of scope** (do NOT touch, even though they look related):
- `exact_device` in `crates/fujicli-dev/src/usb.rs` — needs hardware.
- The in-flight dev-crate work (`firmware.rs`, `main.rs`, `surface.rs`) —
  leave their working-tree state exactly as found.
- `ResponseCode` in `container.rs` — it already has tests; this plan only
  consumes it.
- Any production line of either in-scope file.

## Git workflow

- Branch: `advisor/022-zero-test-residual-modules`
- One commit: `test(ptp,dev): cover error display and device address parsing`
- Do NOT push or open a PR unless the operator instructed it.

## Steps

### Step 1: Tests for `src/lib/ptp/error.rs`

Append `#[cfg(test)] mod tests` with six tests, each asserting current
behavior:

1. `known_response_code_prints_debug_name_and_lowercase_hex` —
   `Error::Response(0x2001).to_string()` equals `"Ok (0x2001)"`, and
   `Error::Response(0x2019).to_string()` equals `"DeviceBusy (0x2019)"`.
2. `unknown_response_code_prints_unknown` —
   `Error::Response(0xffff).to_string()` equals `"Unknown (0xffff)"`.
3. `usb_error_display_keeps_the_source_text` — build
   `Error::from(rusb::Error::Access)`; assert the display starts with
   `"USB error: "`. The tail belongs to `rusb`, so do not assert it.
4. `io_error_display_keeps_the_source_text` —
   `Error::Io(std::io::Error::other("boom"))` display starts with
   `"IO error: "`.
5. `unexpected_eof_maps_to_malformed_end_of_message` —
   `Error::from(std::io::Error::from(std::io::ErrorKind::UnexpectedEof))`
   matches `Error::Malformed(ref message)` with
   `message == "Unexpected end of message"`.
6. `other_io_kinds_stay_io_variant` —
   `Error::from(std::io::Error::from(std::io::ErrorKind::PermissionDenied))`
   matches `Error::Io(_)`.

Also cover `Malformed` passthrough in test 4 by asserting the full string of
`Error::Malformed("bad shape".to_owned())` equals `"bad shape"` (extend the
test or add it to test 4; keep six tests total or split as you prefer — the
case coverage is what matters).

**Verify**: `build-gate -- cargo test --locked -p fujicli --lib ptp::error --jobs 4`
→ all pass, 6 new tests.

### Step 2: Tests for `crates/fujicli-dev/src/usb.rs`

Append `#[cfg(test)] mod tests` with seven tests:

1. `parses_bus_and_address` — `"3.42".parse::<Location>()` yields
   `Location { bus: 3, address: 42 }`.
2. `parses_zero_bus_and_address` — `"0.0"` parses.
3. `rejects_missing_separator` — `"3"` is an error whose message contains
   `"invalid device format"`.
4. `rejects_non_numeric_bus` — `"a.1"` errors with `"invalid USB bus number"`.
5. `rejects_non_numeric_address` — `"1.b"` errors with
   `"invalid USB address"`.
6. `rejects_out_of_range_components` — `"300.1"` (bus above `u8::MAX`) and
   `"1.999"` (address above `u8::MAX`) are both errors.
7. `display_round_trips_the_parsed_form` — parse `"3.42"`, then
   `format!("{location}")` equals `"3.42"`.

**Verify**: `build-gate -- cargo test --locked -p fujicli-dev --jobs 4` →
all pass, 7 new tests.

### Step 3: Confirm the gate

**Verify**: `build-gate -- cargo test --locked -p fujicli --lib ptp:: --jobs 4`
→ all pass (no PTP regressions).
**Verify**: `cargo fmt --all --check` → exit 0.
**Verify**: `build-gate -- cargo clippy --locked --all-features --all-targets --workspace --jobs 4 -- -D warnings`
→ exit 0.
**Verify**: `git diff --exit-code Cargo.lock` → exit 0.

## Test plan

- 6 new tests in `src/lib/ptp/error.rs`: known code, unknown code, USB
  prefix, IO prefix + Malformed passthrough, UnexpectedEof → Malformed
  mapping, other kinds stay Io.
- 7 new tests in `crates/fujicli-dev/src/usb.rs`: happy path, zero
  components, three rejection messages, range rejection, Display round-trip.
- Structural pattern: plain `#[test]` fns with literal expected strings,
  matching `src/lib/policy.rs` tests.
- Verification commands in each step above.

## Done criteria

Machine-checkable. ALL must hold:

- [ ] `build-gate -- cargo test --locked -p fujicli --lib ptp::error --jobs 4`
      exits 0 with 6 new passing tests
- [ ] `build-gate -- cargo test --locked -p fujicli-dev --jobs 4` exits 0
      with 7 new passing tests
- [ ] `git diff` on both in-scope files shows only new `#[cfg(test)]` blocks
- [ ] `cargo fmt --all --check` and the workspace clippy command exit 0
- [ ] `git diff --exit-code Cargo.lock` — lockfile unchanged
- [ ] `plans/README.md` status row for 022 updated

## STOP conditions

Stop and report back (do not improvise) if:

- Either file's excerpts do not match live code (the modules were refactored
  since `f2a78d0`).
- A documented exact string fails — for example `Display` no longer formats
  lowercase hex. That is a behavior change someone made deliberately or a
  bug; either way report the actual string instead of adjusting the test to
  a new expectation on your own.
- The dev crate does not compile because of the in-flight
  `firmware.rs`/`main.rs`/`surface.rs` work. This plan must not unblock or
  finish that work; report and stop.

## Maintenance notes

- If `ResponseCode` gains vendor codes, add one to test 1 rather than
  relying on `0x2001`/`0x2019` alone — the test's purpose is name
  resolution, not the specific constants.
- The `From<io::Error>` UnexpectedEof mapping is the wire-boundary
  convention for truncated device messages; if it changes, the backup
  artifact parser's error text changes too, so keep this test in sync with
  any such refactor.
- `exact_device` remains intentionally untested: it needs a physical USB
  bus, and per `AGENTS.md` hardware behavior requires a recorded device run,
  not a local claim.
