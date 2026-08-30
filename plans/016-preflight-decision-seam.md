# Plan 016: Make the preflight authorization decision unit-testable by separating it from I/O

> **Executor instructions**: Follow this plan step by step. Run every verification command and confirm the expected result before moving on. Touch only the file listed as in scope. This plan must not change any runtime behavior — it is a refactor plus tests. If any STOP condition occurs, stop and report — do not improvise. Commit your work in the worktree following the plan's git workflow section. SKIP updating `plans/README.md` — your reviewer maintains the index. Before reporting, audit every claim against an actual tool result.
>
> **Drift check (run first)**: `git diff --stat 46f2e5e..HEAD -- src/lib/preflight.rs`

## Status

- **Priority**: P2
- **Effort**: M
- **Risk**: MED (touches the single function that authorizes every state-changing camera operation; the mitigation is that behavior must stay bit-identical and the full suite must prove it)
- **Depends on**: none
- **Category**: tests
- **Planned at**: commit `46f2e5e`, 2026-08-30

## Why this matters

`preflight::run` is the one choke point that decides whether a mutation permit is granted: it checks the native binding, physical identity, firmware profile, device info, USB mode, battery, serial binding, and property descriptors, in that order, and only then activates a permit. Every individual `validate_*` helper has unit tests — but the **composition** has none, because `run` takes `&mut Camera` and a `Camera` cannot exist without real USB hardware. A regression that reordered the checks, dropped one, or activated the permit before a check would compile cleanly and pass the entire suite, surfacing only on a physical X-T5.

The audit's original framing was "introduce a fake transport seam", i.e. make `Ptp` generic over a transport trait. **This plan deliberately does not do that.** Threading a generic parameter through `Ptp` (a 3000-line module) and `Camera` to reach `run` is a large, risky change to the transport layer for a test-only benefit. The cheaper and safer decomposition is to separate *policy* from *I/O*: `run` keeps doing all device reads, then hands the gathered facts to a pure function that makes the decision. The pure function is trivially testable, and the transport layer is untouched.

## Current state

`src/lib/preflight.rs`, the orchestrator (abridged, `preflight.rs:433-470`):

```rust
    camera: &'camera mut Camera,
    serial_binding: Option<&SerialFingerprint>,
) -> anyhow::Result<ValidatedCameraSession<'camera, Operation>> {
    camera.ptp.clear_mutation_permit();
    ensure!(
        camera.binding == ModelBindingKind::Native,
        "state-changing camera operations require a native physical model binding"
    );

    let definition = camera.r#impl.camera_definition();
    validate_physical_identity(definition, camera.physical_identity)?;
    let info = camera.ptp.get_info()?;
    let profile = select_profile(definition, Operation::KIND, &info.device_version)?;
    let capability_profile = select_capability_profile(definition, &info.device_version)?;
    validate_device_info(definition, profile, &info)?;

    let usb_mode = u32::from(camera.ptp.get_prop::<u16>(0xD16E_u16)?);
    let battery_percent = read_battery_percent(&mut camera.ptp)?;
    validate_mode_and_battery(profile, Operation::KIND, usb_mode, battery_percent)?;

    let serial_sha256 = crate::features::backup::sha256_hex(info.serial_number.as_bytes());
    validate_serial_binding(serial_binding, &serial_sha256)?;

    let descriptors = read_and_validate_descriptors(&mut camera.ptp, profile)?;
    let permit_id = camera.ptp.activate_mutation_permit()?;
    // ... MutationPermit::new(...), PreflightEvidence { ... }
```

The device I/O calls are exactly four: `camera.ptp.get_info()`, `camera.ptp.get_prop::<u16>(0xD16E)`, `read_battery_percent(&mut camera.ptp)`, and `read_and_validate_descriptors(&mut camera.ptp, profile)` — plus the permit calls `clear_mutation_permit()` / `activate_mutation_permit()`. Everything else is pure computation over values already in hand.

The existing unit tests at the bottom of the file cover the individual validators (e.g. `unknown_firmware_fails_closed`, `x_t5_reala_ace_capability_starts_at_firmware_4_00`, `firmware_capability_selection_does_not_fall_back_to_another_version`, `permit_rejects_inactive_or_mismatched_transport_binding`) and construct `CameraPreflightProfile` / `DeviceInfo` fixtures inline. Reuse those fixture-building patterns.

Conventions: `anyhow::Result` with `ensure!`/`bail!`; no panics on device input; workspace lints deny `unwrap_used`/`panic` outside tests; the typestate `Operation` generic carries `Operation::KIND`.

Environment: `export PATH="/Users/po4yka/.local/share/mise/installs/cue/0.17.1:$PATH"` before any cargo command; `build-gate` is on PATH; use `--jobs 3`.

## Commands you will need

| Purpose | Command | Expected on success |
|---------|---------|---------------------|
| Format check | `cargo fmt --all --check` | exit 0 |
| Focused tests | `build-gate -- cargo test --locked -p fujicli --lib preflight --jobs 3` | all pass |
| Clippy | `build-gate -- cargo clippy --locked --all-features --all-targets --workspace --jobs 3 -- -D warnings` | exit 0 |
| Full gate | `build-gate -- cargo test --locked --all-features --all-targets --workspace --jobs 3` | all pass |

## Scope

**In scope** (the only file you should modify):

- `src/lib/preflight.rs`

**Out of scope** (do NOT touch):

- `src/lib/ptp/**` and `src/lib/camera.rs` — the whole point of this decomposition is to leave the transport layer alone. Do NOT add a generic parameter or a transport trait to `Ptp`.
- `MutationPermit` / `MutationAuthorization` construction semantics — the permit must still be created from exactly the same inputs, at exactly the same point in the sequence.
- The order of the device reads, or the number of them: `run` must issue the same PTP operations in the same order as today.
- Any behavior change whatsoever. This is a refactor: same errors, same messages, same short-circuit points.

## Git workflow

- Branch: `advisor/016-preflight-decision-seam`
- Conventional commit. Suggested: `refactor: separate preflight decision from device reads`.
- Do NOT push or open a PR.

## Steps

### Step 1: Define the gathered-facts input and the pure decision function

In `src/lib/preflight.rs`, add a private struct holding exactly the facts `run` gathers before deciding — everything the checks need, and nothing that requires a device:

```rust
/// Everything `run` reads from the camera (or already knows) before any
/// authorization decision is made. Keeping this separate from `run` lets the
/// decision sequence be unit-tested without a USB device.
struct PreflightFacts<'a> {
    binding: ModelBindingKind,
    definition: &'static SupportedCamera,
    physical_identity: PhysicalUsbIdentity,
    info: &'a crate::ptp::DeviceInfo,
    usb_mode: u32,
    battery_percent: u8,      // match the existing type
    serial_binding: Option<&'a SerialFingerprint>,
}
```

(Adjust field types to match the live code — read it; the excerpt above is a guide.)

Then add a pure function performing the decision sequence in exactly today's order, returning the values `run` needs afterwards:

```rust
/// The complete preflight authorization decision, with no device I/O.
/// Returns the selected profiles on success. The order of checks here is the
/// contract: physical identity, then firmware profile selection, then device
/// info, then mode/battery, then serial binding.
fn decide_preflight(
    facts: &PreflightFacts<'_>,
    operation: CameraPreflightOperation,
) -> anyhow::Result<(&'static CameraPreflightProfile, &'static CameraFirmwareCapabilityProfile)>
```

It performs, in this exact order: the native-binding `ensure!`, `validate_physical_identity`, `select_profile`, `select_capability_profile`, `validate_device_info`, `validate_mode_and_battery`, `validate_serial_binding` (hashing the serial as `run` does today). Descriptor reading and permit activation stay in `run` — they are I/O.

**Verify**: `build-gate -- cargo build --locked --workspace --jobs 3` → exit 0.

### Step 2: Rewrite `run` to gather facts, then call the decision

`run` becomes: clear the permit → read `info` → read `usb_mode` → read `battery_percent` → build `PreflightFacts` → call `decide_preflight` → read and validate descriptors → activate the permit → build `MutationPermit` and `PreflightEvidence` exactly as before.

**Critical constraint**: the sequence of PTP operations on the wire must not change. Today `get_info` happens before profile selection, and `get_prop(0xD16E)`/battery happen after `validate_device_info` but before `validate_mode_and_battery`. Hoisting the two reads earlier changes *when* they are issued relative to the pure checks. That is acceptable **only** if no check between them can currently short-circuit before the reads — verify this by reading the current code, and if a check does short-circuit first (so a bad-firmware camera is never asked for its USB mode today), preserve that by either keeping the reads in place and passing closures, or splitting `decide_preflight` into two pure stages (`decide_identity_and_profile` before the reads, `decide_mode_battery_and_binding` after). **Prefer the two-stage split** — it preserves wire behavior exactly and is still fully testable. Choose it unless you can demonstrate the single-stage version issues identical operations in identical order on every path.

**Verify**: `build-gate -- cargo test --locked -p fujicli --lib preflight --jobs 3` → all existing tests pass unchanged.

### Step 3: Add composition tests

In the file's tests module, using the existing fixture style, add tests that drive the pure decision function(s) directly:

- `rejects_non_native_binding_before_any_other_check` — emulated/unknown binding fails, and the error is the native-binding message (proves the first gate is first).
- `rejects_wrong_physical_identity_before_profile_selection` — a camera whose VID/PID mismatch fails with the identity error even when the firmware would also be unsupported (proves ordering, not just that both reject).
- `rejects_unknown_firmware_before_mode_and_battery` — an unsupported firmware fails with the firmware error even when the USB mode and battery are also invalid.
- `rejects_wrong_usb_mode_and_low_battery` — one case per rejection, with the other inputs valid.
- `rejects_mismatched_serial_binding` — valid everything else, wrong fingerprint.
- `accepts_a_fully_valid_x_t5_4_31_case` — all inputs valid → returns the expected profile pair. This is the test that fails if a check is accidentally made unreachable.

Each ordering test must assert on the specific error message substring (the repo's existing style: `error.to_string().contains(...)`), because "it returned Err" alone does not prove which gate fired.

**Verify**: `build-gate -- cargo test --locked -p fujicli --lib preflight --jobs 3` → all pass, including the new tests.

### Step 4: Prove behavior is unchanged

**Verify**: `build-gate -- cargo test --locked --all-features --all-targets --workspace --jobs 3` → all pass with **no existing test modified** (`git diff 46f2e5e -- tests/` empty, and no assertion changed inside `preflight.rs`'s pre-existing tests); `build-gate -- cargo clippy --locked --all-features --all-targets --workspace --jobs 3 -- -D warnings` → exit 0.

## Test plan

- New: the six composition tests in Step 3, in `src/lib/preflight.rs`'s tests module, modelled on the existing validator tests' fixture construction.
- Existing to keep green, unmodified: every current `preflight` test, plus the full workspace suite.
- Not covered here (and out of scope): descriptor reading and permit activation, which remain I/O-bound.

## Done criteria

ALL must hold:

- [ ] The pure decision function(s) exist and contain the check sequence; `run` contains the device reads and permit activation
- [ ] The PTP operation order issued by `run` is unchanged (state this explicitly in your report, with the reasoning that supports it)
- [ ] No pre-existing test was modified or deleted (`git diff 46f2e5e -- src/lib/preflight.rs` shows the old test bodies intact)
- [ ] The six new composition tests exist and pass, each asserting a specific error message
- [ ] `cargo fmt --all --check` exits 0
- [ ] `build-gate -- cargo clippy --locked --all-features --all-targets --workspace --jobs 3 -- -D warnings` exits 0
- [ ] `build-gate -- cargo test --locked --all-features --all-targets --workspace --jobs 3` exits 0
- [ ] `git status` shows only `src/lib/preflight.rs` modified
- [ ] `plans/README.md` status row updated (SKIPPED per reviewer override)

## STOP conditions

Stop and report if:

- `run` no longer matches the excerpt (drift since `46f2e5e`).
- Preserving the exact PTP operation order requires touching `src/lib/ptp/**` or `src/lib/camera.rs` — report; the transport layer is out of scope by design.
- A pre-existing test fails after the refactor — that means behavior changed; report the test and the diff in behavior rather than adjusting the test.
- The borrow checker forces you to clone `DeviceInfo` or the descriptors to build `PreflightFacts` — a small borrow is fine, a deep clone of device data on the hot path is not; report the obstacle instead of cloning.

## Maintenance notes

- The check order in the pure function is now a tested contract; a reviewer should treat a reordering as a behavior change requiring a test update and an explicit rationale.
- This deliberately leaves descriptor reading and permit activation untested by unit tests. If those ever need coverage, the honest options are a transport trait (the larger refactor this plan declined) or a device-backed integration test — not a mock that asserts on itself.
