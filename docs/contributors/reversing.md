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
  serial numbers. Privacy-review the diagnostics before sharing them.

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

This section designs (but does not yet implement) the still/movie namespace
probe called for in `docs/users/support.md` and `docs/users/usage.md`: does
selector `0xD18C` address the X-T5's still C1-C7 custom-setting slots, its
movie C1-C7 slots, or something else. The command name is
`fujicli-dev probe simulation-namespace`, gated behind the
`dangerous-reverse-engineering` feature described above.

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

Decision table (as designed; the "observed still/movie signal" column is the
open question below):

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
