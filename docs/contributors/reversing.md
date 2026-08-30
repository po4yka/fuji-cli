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

## Reversing Render Profiles

Rendering is the hard one. Fujifilm doesn't document the conversion profile wire
format, and the only reliable source of truth is observing PTP traffic from
Fujifilm X RAW Studio while it manipulates the camera.

The high-level goal: discover the camera's `profile_code`, the number of fields,
the order of fields, and any value-transformation aliases between the
user-facing options and what the camera stores on the wire.

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

- **X RAW Studio sometimes reads fewer conversion-profile fields than it writes
  back.** Observed on the X-T5; root cause unknown. When reversing, prefer the
  _write_ path as ground truth.
- **Padding sizes vary or may not be constant.** The X-T5 uses `0x1ee` bytes
  between the profile-code string and the field array. Record the observed
  count as the camera's required `render.header_padding`; codegen deliberately
  has no shared default.
