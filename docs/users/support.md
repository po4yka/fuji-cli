# Camera Support

The schema recognizes several Fujifilm generations, but physical-device
evidence is limited and state-changing support is narrower still. Do not infer
compatibility from a shared generation, a nearby firmware version, or a
successful build. Use this software at your own risk: the author is not
responsible for damage, lost data, or any other adverse outcome involving your
camera or equipment.

| Model           | Generation  | Base Info | Backups | Simulations | Rendering |
| --------------- | ----------- | --------- | ------- | ----------- | --------- |
| FUJIFILM X-E1   | X-Trans     | ?         |         |             |           |
| FUJIFILM X-M1   | X-Trans     | ?         |         |             |           |
| FUJIFILM X70    | X-Trans II  | ?         |         |             |           |
| FUJIFILM X-E2   | X-Trans II  | ?         |         |             |           |
| FUJIFILM X-T1   | X-Trans II  | ?         |         |             |           |
| FUJIFILM X-T10  | X-Trans II  | ?         |         |             |           |
| FUJIFILM X100F  | X-Trans III | ?         |         |             |           |
| FUJIFILM X-E3   | X-Trans III | ?         |         |             |           |
| FUJIFILM X-H1   | X-Trans III | ?         |         |             |           |
| FUJIFILM X-Pro2 | X-Trans III | ?         |         |             |           |
| FUJIFILM X-T2   | X-Trans III | ?         |         |             |           |
| FUJIFILM X-T20  | X-Trans III | ?         |         |             |           |
| FUJIFILM X100V  | X-Trans IV  | ?         |         |             |           |
| FUJIFILM X-E4   | X-Trans IV  | ?         |         |             |           |
| FUJIFILM X-Pro3 | X-Trans IV  | ?         |         |             |           |
| FUJIFILM X-S10  | X-Trans IV  | ?         |         |             |           |
| FUJIFILM X-T3   | X-Trans IV  | ?         |         |             |           |
| FUJIFILM X-T4   | X-Trans IV  | ?         |         |             |           |
| FUJIFILM X-S20  | X-Trans IV  | Y         | Y       | N           |           |
| FUJIFILM X100VI | X-Trans V   | ?         |         |             |           |
| FUJIFILM X-H2   | X-Trans V   | ?         |         |             |           |
| FUJIFILM X-H2S  | X-Trans V   | ?         |         |             |           |
| FUJIFILM X-T5   | X-Trans V   | Y         | Y       | N           | R         |

## Legend

- **Y** - Historical physical-device evidence confirms the command set worked
  on that model. Current mutation authority is defined separately below.
- **?** - Untested but likely works. The camera is recognised over USB and the
  relevant PTP commands are present, but nobody has confirmed end-to-end
  behaviour.
- **N** - Known not to work or deliberately unavailable for safety.
- **R** - Read-only discovery/historical evidence exists, but writes are
  intentionally disabled until the exact wire descriptor is verified.
- Blank - Not implemented yet.

The X-S20's `N` for Simulations is historical only: the schema declares no
`preflight` block for this camera, so every simulation command, including the
read commands `list`, `get`, and `export`, fails closed today. The `N` records
current availability, not a hardware failure.

A `?` in the "Base Info" column means `fujicli device info` should succeed. A
blank elsewhere means the FML spec for that camera does not yet declare the
feature; see [adding a camera](../contributors/adding-cameras.md) to fill that
in.

The feature table records historical physical-device coverage; it is not the
state-changing compatibility matrix. Backup mutations are enabled only for the
Fujifilm X-T5 with exact PTP firmware `4.31`, USB mode `0x6`, and the required
battery level. Simulation commands are deliberately unavailable because the
PTP evidence does not identify whether selector `0xD18C` addresses the still or
movie C1-C7 namespace. RAW rendering is also disabled even on 4.31:
the retained code documents an unverified 28-slot read/29-slot write assumption,
not a trace-backed descriptor. Every other model, firmware, mode, or incomplete
capability/descriptor set also fails closed: simulation commands, including
`list`, `get`, and `export`, run the same preflight check as a write and are
refused before they touch the camera. Adding a row requires captured device
evidence and an explicit FML preflight profile; a nearby firmware version is
never assumed compatible.

### Latest physical-device verification

X-T5, PTP firmware `4.31`, USB mode `0x6` (USB RAW CONV./BACKUP RESTORE),
battery 65%, macOS host, 2026-08-31, commits `e0654cf` and `3706f01`:

- Verified: `device list`, `device info`, `backup export` (twice, identical
  artifact SHA-256, 38780-byte payload), `backup inspect`. Simulation commands
  fail closed with the unverified-profile error, as intended (`N`).
- Not exercised: `backup import`, `image render`, `image recover` on a real
  object. The Backups `Y` covers restore only through the earlier historical
  evidence, not this run.
- Before `e0654cf`, `backup export` failed on this camera because it answers
  `GetObjectInfo` without the 1020-byte padding seen in the original capture.
- In USB Card Reader mode the camera exposes no Fuji properties; since
  `bec0858` `device info` names the required USB mode instead of failing with
  `DevicePropNotSupported (0x200a)` (verified on the camera). See
  [macOS camera access](how-to/macos-camera-access.md) for `ptpcamerad`.
- The camera answers `GeneralError` to every `GetDevicePropDesc` in USB mode
  `0x6`. Since `50cc37c`, preflight validates the read-only USB mode and
  battery properties by value shape, so `image recover` reaches the camera
  object lookup (verified). Simulation writes stay blocked: their writable
  requirements cannot be proven without descriptors.

The generated X-T5 capability matrix records Reala Ace as unavailable on
firmware `3.01` and available from `4.00`. This records option availability only.
Firmware `4.31` retains the current RAW code/padding/order assumptions for
read-only comparison, but they do not grant write authority.

## Emulation Mode

The `--emulate VENDOR:PRODUCT` flag selects a different generated logical model.
It never overrides the physical USB vendor / product identity, and the physical
device must itself be a supported camera.

- Emulation does not add support for cameras that aren't yet in the schema; it
  only changes which generated implementation is invoked.
- `device info` is the only purely read-only emulated command.
- Simulation access writes the transient custom-setting selector, so every
  simulation command rejects emulation.
- Every backup command and RAW rendering reject emulation.
- Selecting a USB device explicitly with `--device BUS.ADDRESS` does not bypass
  physical identity validation.

If you're not actively debugging or contributing, you probably don't want this
flag.

## Reporting Compatibility

If your camera is in the table with `?` and the feature works, file a short
issue with:

- Model, firmware version, and the camera's USB ID (`fujicli device
  list`).
- The command that worked (`fujicli ... -vvv`).
- A privacy-reviewed diagnostic excerpt. Do not upload private RAF/JPEG files,
  serial numbers, backup artifacts, or custom-setting names unless they are
  necessary and you intend to disclose them.

If a feature is blank, see [adding a camera](../contributors/adding-cameras.md)
and [reversing](../contributors/reversing.md).

The production `fujicli` binary does not expose reverse-engineering commands.
Those workflows use the separately gated, non-distributable `fujicli-dev`
crate.
