# Reversing Fujifilm Cameras

Reverse engineering is provided by the unpublished `fujicli-dev` workspace
package. The production `fujicli` parser contains no reverse command, including
when the workspace is validated with all features. Hiding a command from help
is not a security boundary.

Build and run the development binary only by exact package selection:

```sh
cargo run --locked -p fujicli-dev --features reverse-tools -- \
  --device BUS.ADDRESS discover info -vvv
```

`fujicli-dev` always requires an exact `--device`; it has no automatic camera
selection and no `--emulate`. Its current commands perform read-only PTP
discovery only. The binary is `publish = false`, excluded from workspace
default members, and never staged by the production release workflow.

The normal library deliberately keeps `Ptp`, its transport fields, raw send and
property setters, generated manager traits, and mutation permits crate-private.
Do not add a generic raw-send command or expose those internals under
`reverse-tools`. New discovery operations should be narrow, named, read-only
`Camera` methods. A state-changing probe needs its own reviewed high-level
workflow, exact FML model/firmware policy, safety and recovery plan, explicit
operation permit, fault-injection tests, and physical-device acceptance
evidence before it can be merged. If unconstrained protocol experimentation is
needed, keep it in `fujicli-dev` rather than weakening the consumer library
boundary.

The current discovery surface is deliberately limited:

| Command | PTP operations | Classification |
| --- | --- | --- |
| every command | `OpenSession`, `CloseSession` | transient state-selecting session control |
| `discover info` | `GetDeviceInfo`, `GetDevicePropValue` | read-only |
| `discover simulation` | `GetDevicePropValue` | read-only |
| `discover backup export` | `GetObjectInfo`, `GetObject` | read-only |
| `discover render-profile` | `GetDeviceInfo`, `GetDevicePropDesc`, `GetDevicePropValue` | read-only |

There is no `SetDevicePropValue`, custom-slot selector, upload, restore, delete,
or generic raw-send command. Other custom-setting slots must be selected
physically on the camera before repeating the read-only probe, or studied from
a privacy-reviewed vendor USB capture.

## Prerequisites

- A full backup before doing anything else - both for the camera _and_ for any
  custom settings you care about. If the schema doesn't yet declare
  `features.backup: true` for the model, use
  [`fujicli-dev discover backup export`](#backup) to attempt the export; the
  non-reverse `fujicli backup export` is gated on the declared feature and won't
  run.
- Run probes with maximum verbosity: `-vvv` prints PTP operation metadata,
  response lengths, and success or failure, but not response payloads or camera
  serial numbers. `discover info --print-values` and
  `discover simulation --print-values` additionally print each property payload
  (length, hex, decoded scalar or string); the simulation set includes the
  custom setting name text. Privacy-review the diagnostics before sharing them.

## Probing Existing PTP Surface

### Base Info

```sh
cargo run --locked -p fujicli-dev --features reverse-tools -- \
  --device BUS.ADDRESS discover info -vvv
```

A successful `GetDeviceInfo` operation and its response length show that the
camera at least speaks PTP and base info may work. The diagnostic output omits
the raw reply and serial number. Individual unsupported probes are reported at
debug level; the command exits unsuccessfully only when none of its probes
succeed.

### Backup

```sh
cargo run --locked -p fujicli-dev --features reverse-tools -- \
  --device BUS.ADDRESS discover backup export /tmp/probe.fbk -vvv
```

A successful export validates the object format, metadata padding, and declared
payload length for the read side of the standard Fuji backup exchange. It is
not enough to declare restore support. The destination must be a new file;
stdout and overwrite are rejected. Backup bytes are never copied into
diagnostics. Restore must be described by an exact FML preflight profile and
exercised through the normal guarded command.

### Simulation

```sh
cargo run --locked -p fujicli-dev --features reverse-tools -- \
  --device BUS.ADDRESS discover simulation -vvv
```

Reads every PTP property code known to the schema in the camera's current slot.
Errors on individual codes are expected for cameras that don't expose a
particular setting, so a partial result remains successful. If no property probe
succeeds, the command exits unsuccessfully instead of reporting an empty
discovery as success. The output informs the `settings:` list for the camera's
simulation block. Diagnostics omit returned property bytes and custom-setting
names.

Reverse mode does **not** verify allowed values and deliberately does not write
the custom-setting selector. Discovering other slots or mutating an unknown
property requires a separately reviewed probe and physical-device recovery
plan; it is not exposed by this CLI.

## Requirements for Any Future Dangerous Probe

Do not add a dangerous command merely by widening `reverse-tools`. If a
state-changing experiment becomes necessary, add it only to `fujicli-dev`
behind a new `dangerous-reverse-engineering` feature and a command-specific
guard. Before the first mutating PTP container, the command must:

1. require exact `--device BUS.ADDRESS` and reject emulation or auto-selection;
2. display USB bus/address, VID:PID, PTP manufacturer/model/firmware, and a
   SHA-256 serial fingerprint, never the raw serial;
3. require exact confirmation of that live serial fingerprint;
4. create, validate, sync, and hash a fresh no-clobber pre-backup;
5. require a fixed command-specific acknowledgement string;
6. durably record an audit attempt, then send the mutating operation once.

Automatic retry is forbidden. A timeout, disconnect, malformed response, or
other ambiguous result must stop the session, report camera state as unknown,
and print `DO NOT RETRY AUTOMATICALLY`. Selector experiments may touch only one
explicit slot per invocation and must snapshot, restore, and verify the prior
selector once. Generic opaque restore remains prohibited.

The audit log must be restrictive, append-only JSONL and contain only bounded
metadata: timestamp, tool version, invocation ID, operation/risk class, PTP
operation codes, USB location, VID:PID, bounded model/firmware, a minimized
serial correlation fingerprint, pre-backup digest, and outcome. It must not
contain raw serials, argv, full paths, property or backup payloads, custom
setting names, arbitrary camera strings, or full error chains.

## Design: the `simulation-namespace` Probe

This section designs the still/movie namespace probe called for in the
[support policy](../users/support.md) and
[fail-closed model](../users/explanation/fail-closed-safety-model.md): does
selector `0xD18C`
address the X-T5's still C1-C7 custom-setting slots, its movie C1-C7 slots, or
something else. The command is `fujicli-dev probe simulation-namespace`,
gated behind the `dangerous-reverse-engineering` feature described above. It
is now implemented; the physical-device run remains a maintainer step (see
"Maintainer decisions" below -- run the non-mutating X RAW Studio capture
first, and treat this probe strictly as the fallback).

### Run procedure

```sh
cargo run --locked -p fujicli-dev --features reverse-tools,dangerous-reverse-engineering -- \
  --device BUS.ADDRESS -vvv probe simulation-namespace c1 \
  /path/to/new-backup.fbk /path/to/audit-log.jsonl \
  --confirm-fingerprint <SHA256> \
  --acknowledge I-UNDERSTAND-THIS-WRITES-SELECTOR-D18C
```

The command implements the six-step guard sequence from "Requirements for Any
Future Dangerous Probe" above:

1. Opens the camera at the exact `--device` given (never auto-selected) and
   prints USB bus/address, VID:PID, PTP manufacturer/model/firmware, and the
   SHA-256 fingerprint of the live serial number. The raw serial is never
   printed, logged, or written anywhere.
2. Compares that live fingerprint against `--confirm-fingerprint`. A
   mismatch aborts before any write. Because the operator cannot know the
   live fingerprint in advance, the intended flow is two invocations: a
   first attempt with any placeholder value (which fails and prints the live
   fingerprint), then a second, deliberate invocation with that value copied
   into `--confirm-fingerprint`.
3. Compares `--acknowledge` against the fixed string
   `I-UNDERSTAND-THIS-WRITES-SELECTOR-D18C`. A mismatch aborts before any
   write.
4. Exports a fresh backup to the given `backup` path (must not already
   exist; the export path reuses the same validated no-clobber writer as
   `discover backup export`) and computes its SHA-256 digest.
5. Appends one JSONL line to `audit-log` (created with mode `0600` if
   absent). Symlinks are rejected; on Unix, an existing file is accepted
   only when it grants no group or other access. The line records the attempt
   -- timestamp, tool version, invocation ID, operation/risk class, the two
   PTP operation codes involved, USB location, VID:PID, bounded
   model/firmware, the serial fingerprint, the pre-backup digest, and outcome
   `attempted`. This record is durably written _before_ the mutating send, so
   a durable trail exists even if the process is interrupted immediately
   afterward.
6. Reads the current raw value of `0xD18C` (snapshot), writes the chosen
   C1-C7 slot's wire value once, reads `0xD18C` back (observed), writes the
   snapshot value back once (restore), and reads `0xD18C` once more to
   verify the restore. Any failure or mismatch at any of these steps prints
   `DO NOT RETRY AUTOMATICALLY` and exits non-zero; there is no automatic
   retry anywhere in this sequence. Whether this step succeeds or fails, the
   command appends a second, terminal JSONL line to `audit-log` before
   returning: the exact same allowlisted fields as the pre-write record,
   including the same invocation ID (so the two lines correlate as one
   attempt), with `outcome` replaced by one of `restored`,
   `snapshot_failed`, `write_failed`, `readback_failed`, `restore_failed`,
   `restore_verify_read_failed`, or `restore_verify_mismatch`, classified
   from this step's own control flow rather than from an error message. A
   Ctrl-C received from the probe write through restore verification is
   latched until the original selector has been restored and verified; its
   terminal outcome is `interrupted_after_restore`. A second Ctrl-C
   force-quits with exit code 3 and reports the camera state as unknown.
   Ctrl-C outside this protected mutation-and-restore window exits with code
   130. A failure while appending the terminal line is itself reported but
   never masks or replaces the underlying probe failure. One invocation of
   this command therefore always produces exactly two JSONL lines in the
   audit log (or exactly one if an earlier gate -- acknowledgement,
   fingerprint, or backup export -- aborts before this step is reached),
   never fewer on the failure path and never more via retry.

No PTP property is currently known to distinguish the still and movie
custom-setting namespaces on the wire (open question 1, still unresolved at
the wire level), so the command's verdict is always "ambiguous" today: it
prints `DO NOT RETRY AUTOMATICALLY` and instructs the operator to corroborate
manually via the camera's own C1-C7 LCD menu, read before and after the
write, per the maintainer decision below. The command does not fabricate a
still/movie verdict from an unknown signal.

What it writes: exactly one explicit C1-C7 slot value to `0xD18C`, exactly
once per invocation, as the single mutating PTP container required by the
guard sequence above.

What it reads before the write: the current raw value of `0xD18C` (the prior
selector, to be restored and verified afterward) plus the existing read-only
discovery surface (`GetDeviceInfo`, `0xD16E` USB mode, `0xD36B` battery info)
for identity display and the pre-backup.

What it reads after the write: ideally, one or more properties whose value
differs depending on whether the write landed in the still or the movie
custom-setting namespace. No such property is currently known.

Decision table (as implemented in `crates/fujicli-dev/src/decision.rs`; the
"observed still/movie signal" column is the open question below, so the
implementation always resolves to the ambiguous row until a signal is
supplied):

| Observed still/movie signal | Verdict |
| --- | --- |
| Signal indicates only the still namespace changed | still |
| Signal indicates only the movie namespace changed | movie |
| Signal indicates both, neither, or is unreadable/times out | ambiguous — print `DO NOT RETRY AUTOMATICALLY` |

### OPEN QUESTIONS for the maintainer

1. **No known read-back observable.** Nothing in `fml/`, `docs/`, or the
   capture-free `support/` directory identifies a PTP property (or property
   pair) whose value distinguishes still-mode custom-setting state from
   movie-mode custom-setting state after `0xD18C` is written. Inventing a
   property code to fill this gap would violate `AGENTS.md`'s prohibition on
   inventing PTP property codes, so this design does not propose one. A
   physical run may need to pair the probe's PTP-level log with a human
   reading the camera's own C1-C7 menu (LCD) before and after the write,
   rather than relying on a wire-level signal alone.
2. **No raw single-property write/read primitive is reachable from
   `fujicli-dev`.** `Ptp::set_prop_raw` in `src/lib/ptp/mod.rs` is
   `pub(super)`, and the only code path that ever constructs a
   `MutationPermit` (`preflight::run` in `src/lib/preflight.rs`) requires
   `camera.binding == ModelBindingKind::Native` (never true for
   `Camera::open_unknown`, the only camera `fujicli-dev` can construct) and a
   `CameraPreflightProfile` whose `status` is `Verified` for the requested
   firmware and operation. X-T5 firmware 4.31's `SimulationAccess` and
   `SimulationWrite` profiles are `Unverified`
   (`tests/xt5_simulation_domain.rs`), so `select_profile` refuses to hand out
   a permit before this probe could even run. Reaching `0xD18C` therefore
   needs a new library primitive that bypasses this permit machinery for an
   explicitly unverified property — exactly the raw mutation surface commit
   `124aa4f` ("fix: seal raw PTP mutation access") deliberately closed. Adding
   it is a maintainer decision, not something this design authorizes
   unilaterally; see the executor report for the exact API shape considered.

### Maintainer decisions (2026-08-30)

Both open questions above have been answered by the maintainer.

1. **Observable: resolve by capture first, LCD probe as fallback.** Before any
   mutating write of our own, run an X RAW Studio USB-traffic capture: the
   official software may itself touch `0xD18C` and expose the still/movie
   signal on the wire, with no dangerous write from us. Only if the capture
   fails to establish which namespace `0xD18C` addresses do we fall back to the
   probe with the LCD-observation protocol (PTP-level log paired with a human
   reading the camera's own C1-C7 menu before and after, in still and movie
   modes). The non-mutating capture is always attempted first.
2. **Raw single-property primitive: sanctioned, probe-scoped only.** A new
   `Camera` method (working name `reverse_probe_write_single_property`)
   performing one `SetDevicePropValue`/`GetDevicePropValue` round trip without
   the `MutationPermit`/`Verified`-profile requirement, gated behind BOTH
   `reverse-tools` AND `dangerous-reverse-engineering` so it cannot compile
   into a default-features distributable. Single send, no auto-retry. This is a
   deliberate, narrowly-scoped reopening of the `124aa4f` seal, authorized only
   for this probe; it must not be widened beyond the single-property round trip.
   Its implementation is tracked as a follow-up plan and must go through the
   diff-acceptance review for sealed-mutation-surface changes.

The physical-device work (the capture and, if needed, the sanctioned probe run
against an X-T5 with a recovery plan) remains a maintainer step.

## Reversing Render Profiles

Rendering is the hard one. Fujifilm doesn't document the conversion profile wire
format, and the only reliable source of truth is observing PTP traffic from
Fujifilm X RAW Studio while it manipulates the camera.

The high-level goal: discover the camera's `profile_code`, the number of fields,
the order of fields, and any value-transformation aliases between the
user-facing options and what the camera stores on the wire.

Start with the built-in read-only capture before observing X RAW Studio:

```sh
cargo run --locked -p fujicli-dev --features reverse-tools -- \
  --device BUS.ADDRESS discover render-profile \
  /tmp/x-t5-4.31-profile.json -vvv
```

On a physical X-T5 (firmware 4.31, USB mode 0x6) with no RAF loaded, both
`GetDevicePropDesc(D185)` and `GetDevicePropValue(D185)` answer `GeneralError`
(as do `D183` and `D17B`), so this read-only probe fails without producing an
artifact. The camera appears to serve the conversion profile only while a RAF
is loaded for conversion, which the probe never does. On that camera the
descriptor half is moot anyway: in USB mode 0x6 it answers `GeneralError` to
every `GetDevicePropDesc`. The vendor capture below is the only known route.

The command issues only `GetDeviceInfo`, best-effort reads USB mode and the raw
`GetDevicePropDesc(D185)` bytes, and retrieves `GetDevicePropValue(D185)`. The
JSON contains the exact descriptor and payload as lowercase hex with SHA-256,
plus a best-effort descriptor summary and only envelope fields that can be
parsed without assuming padding or field order. An unknown descriptor datatype
therefore does not discard the payload. It intentionally records camera state
as `unknown`. The artifact is privacy-sensitive and must be reviewed before it
is shared. The command never uploads a RAF, changes a selector/property, or
creates mutation authorization.

### Recommended Setup

- **Windows VM (QEMU + USB pass-through)** - running Fujifilm's closed-source X
  RAW Studio. Minimal install ISOs without Microsoft's bullshit can be built at
  <https://schneegans.de/windows/unattend-generator/>.
- **Wireshark** - for USB packet capture. A patched build that exposes
  `frame.raw` in `tshark` is required for USB traffic.

### Procedure

1. Load the `usbmon` kernel module.
2. Start `support/monitor-rendering.sh <usb-interface>
   <render-header-padding-bytes>` **before** connecting the camera, for example
   `support/monitor-rendering.sh usbmon1 0x1ee` for the X-T5. Pass the exact
   padding observed for the selected camera (or the candidate value being
   investigated); the helper deliberately has no cross-camera default. See
   <https://gitlab.com/wireshark/wireshark/-/issues/20908> for why ordering
   matters.
3. Start the Windows VM.
4. Connect the camera and pass it through to the VM.
5. Open Fujifilm X RAW Studio.
6. Make rendering changes - film simulations, tone curves, grain. You don't need
   to "finalise" the render; the live preview is enough.
7. Watch the script's output: conversion-profile bytes are printed as they
   change. Diff successive captures to map field offsets to user-facing
   settings.

### Translating Findings into FML

Each observed field becomes one entry in the camera's `render.fields` list. If
two settings appear as one wire field with a discrete jump (e.g.
`dynamic_range = hdr800_plus` becoming `dynamic_range = hdr800` plus a
"priority" flag), encode it as a `transformation`:

```cue
transformations: [
    {
        when: {ref: "dynamic_range", equals: "hdr800_plus"}
        apply: [
            {ref: "dynamic_range",          value: "hdr800"},
            {ref: "dynamic_range_priority", value: "plus"},
        ]
    },
]
```

The codegen pipeline will automatically infer the inverse on read, so the user
keeps seeing `hdr800_plus`. See
[internals / analyses](../internals/analyses.md#inverse-transformations) for the
constraints (the apply pattern must be unique among transformations).

## Known Pitfalls

- **X RAW Studio may read fewer conversion-profile fields than it writes back.**
  Historical X-T5 notes describe one extra 32-bit write value, commonly `0` or
  `1`, but do not retain firmware/state/version metadata or wire bytes. Treat
  read and write as separate layouts and capture both directions; neither is
  ground truth for the other.
- **Padding sizes vary or may not be constant.** `0x1ee` is the current X-T5
  assumption, not retained capture evidence. Discovery must preserve the whole
  payload and must not infer or zero padding. A write descriptor needs a golden
  payload and a declared padding policy.

Privacy-reviewed minimized payloads belong under
`tests/wire/render_profiles/<model>/<firmware>/`; follow the manifest contract
in that directory. Keep full PCAP/PCAPNG files in the private HIL evidence store
and retain their SHA-256 in the committed manifest.

`write_verified` is currently a reserved status: codegen rejects it even when a
manifest path is present. Enabling it requires a machine-checked manifest/hash
contract, live camera-state binding, and lossless preservation tests for
padding and opaque fields. Until all three exist, captured evidence can improve
read compatibility but cannot authorize RAW writes.
