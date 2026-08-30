# Camera Support

This tool has only been extensively tested with the **Fujifilm X-T5**. While the
underlying PTP commands are likely compatible with other Fujifilm models,
**compatibility is not guaranteed**. Use this software at your own risk: the
author is not responsible for damage, lost data, or any other adverse outcomes -
physical or psychological - to your camera or equipment.

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
| FUJIFILM X-S20  | X-Trans IV  | Y         | Y       | Y           |           |
| FUJIFILM X100VI | X-Trans V   | ?         |         |             |           |
| FUJIFILM X-H2   | X-Trans V   | ?         |         |             |           |
| FUJIFILM X-H2S  | X-Trans V   | ?         |         |             |           |
| FUJIFILM X-T5   | X-Trans V   | Y         | Y       | Y           | Y         |

## Legend

- **Y** - Known to work; the corresponding command set is implemented and
  tested.
- **?** - Untested but likely works. The camera is recognised over USB and the
  relevant PTP commands are present, but nobody has confirmed end-to-end
  behaviour.
- **N** - Known not to work.
- Blank - Not implemented yet.

A `?` in the "Base Info" column means `fujicli device info` should succeed. A
blank elsewhere means the FML spec for that camera does not yet declare the
feature; see [adding a camera](../contributors/adding-cameras.md) to fill that
in.

The feature table records historical physical-device coverage; it is not the
state-changing compatibility matrix. Mutating commands are currently enabled
only for the Fujifilm X-T5 with exact PTP firmware `4.31`, USB mode `0x6`, and a
fully charged battery. Every other model, firmware, mode, or incomplete
capability/descriptor set fails closed before a write. Adding a row requires
captured device evidence and an explicit FML preflight profile; a nearby
firmware version is never assumed compatible.

## Emulation Mode

The `--emulate VENDOR:PRODUCT` flag selects a different generated logical model.
It never overrides the physical USB vendor / product identity, and the physical
device must itself be a supported camera.

- Emulation does not add support for cameras that aren't yet in the schema; it
  only changes which generated implementation is invoked.
- `device info` is the only purely read-only emulated command.
- Simulation access writes the transient custom-setting selector, so every
  simulation command rejects emulation.
- Every backup command, RAW rendering, and all reverse commands reject
  emulation.
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
- For rendering, a small sample image plus the rendered output.

If a feature is blank, see [adding a camera](../contributors/adding-cameras.md)
and [reversing](../contributors/reversing.md).
