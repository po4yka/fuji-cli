# Reversing Fujifilm Cameras

```text
Reverse engineer device communication

Only run this if you have a full device backup and know what you are doing. Misuse can corrupt your camera or void your warranty.

Usage: fujicli device reverse [OPTIONS] <COMMAND>

Commands:
  backup      Attempt to manage backups
  info        Attempt to get camera info
  simulation  Get information about supported simulation management commands
  help        Print this message or the help of the given subcommand(s)
```

This subcommand is `hide = true` in the CLI on purpose. It probes the camera for
capabilities and is useful only when adding support for a new model.

## Prerequisites

- A full backup before doing anything else - both for the camera _and_ for any
  custom settings you care about. If the schema doesn't yet declare
  `features.backup: true` for the model, use
  [`fujicli device reverse backup export`](#backup) to attempt the export; the
  non-reverse `fujicli backup export` is gated on the declared feature and won't
  run.
- Run probes with maximum verbosity: `-vvv` prints PTP operation metadata,
  response lengths, and success or failure, but not response payloads or camera
  serial numbers. Privacy-review the diagnostics before sharing them.

## Probing Existing PTP Surface

### Base Info

```sh
fujicli device reverse info -vvv
```

A successful `GetDeviceInfo` operation and its response length show that the
camera at least speaks PTP and base info may work. The diagnostic output omits
the raw reply and serial number. Individual unsupported probes are reported at
debug level; the command exits unsuccessfully only when none of its probes
succeed.

### Backup

```sh
fujicli device reverse backup export /tmp/probe.fbk
fujicli device reverse backup import /tmp/probe.fbk
```

Successful round-trip means the camera supports the standard Fuji backup
commands. Mark `features: backup: true` in the camera spec. Backup bytes are
written only to the selected output; they are never copied into diagnostic
logs.

### Simulation

```sh
fujicli device reverse simulation
```

Iterates over every PTP property code known to the schema, across every
custom-setting slot. Errors on individual codes are expected for cameras that
don't expose a particular setting, so a partial result remains successful. If
no property probe succeeds, the command exits unsuccessfully instead of
reporting an empty discovery as success. The output tells you which props the
camera actually responds to, which informs the `settings:` list for the camera's
simulation block. Diagnostics omit the returned property bytes and
custom-setting names.

The reverse mode does **not** verify allowed values; it only reads. Writing to a
property you don't understand is what backups exist for.

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
2. Start `support/monitor-rendering.sh` **before** connecting the camera. (See
   <https://gitlab.com/wireshark/wireshark/-/issues/20908> for why ordering
   matters).
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
- **Padding sizes vary or may not be constant.** The X-T5 uses `0x1EE` bytes
  between the profile-code string and the field array. Until a second
  render-capable camera is reversed, the codegen assumes this value is
  universal; see
  [`crates/codegen/src/common/renders/camera.rs`](../../crates/codegen/src/common/renders/camera.rs).
