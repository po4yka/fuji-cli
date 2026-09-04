# CLI Reference

The clap command model is the source of truth for syntax. Use `fujicli --help`
for top-level help and `fujicli COMMAND --help` for the installed build.
Packaged distributions also include section 1 man pages.

## Command Model

The operational command families are `device`, `simulation`, `backup`, and
`image`. Their short aliases are `d`, `s`, `b`, and `i`. `completion` generates
a shell completion script.

Common operations also use aliases: `list -> l`, `get -> g`, `set -> s`,
`export -> e`, `import -> i`, and `render -> r`.

Schema-driven leaf commands have two help levels. `-h` shows compact setting
names. `--help` also shows FML ranges, steps, string limits, and canonical enum
or lookup values.

Those constraints are global schema limits. The connected camera's exact
firmware profile may allow a narrower value set.

Only `-v` and `--verbose` are global. Formatting, device selection, and
emulation flags exist only on leaf commands that use them, and must appear
after the leaf command.

Meaningless combinations such as `device list --device`, `backup inspect
--device`, or `image render --json` are usage errors with status `2`. They are
not silently ignored.

`-d` and `--device BUS.ADDRESS` select one camera when several supported
devices are connected. `--emulate VENDOR:PRODUCT` selects a generated logical
model but does not change the physical USB identity.

Only `device info` is read-only under emulation. Simulation commands first
write the custom-setting selector, so simulation access, backups, and RAW
rendering reject emulation. See the [support policy](../support.md#emulation-mode).

## Device Commands

```sh
fujicli device list
fujicli device info
```

`device list` reads USB descriptors without claiming an interface or opening a
PTP session. `device info` reads the live model, serial, battery, and USB mode.

## Backup Commands

```sh
fujicli backup export camera.fbk
fujicli backup inspect camera.fbk
fujicli backup import camera.fbk --dry-run
```

Use the task guides for [export and inspection](../how-to/export-and-inspect-backup.md)
and [restore](../how-to/dry-run-and-restore-backup.md). Every backup command
rejects `--emulate`, including offline inspection.

## Simulation Commands

Every simulation command is refused by preflight today, on every camera in the
schema, including reads. On the X-T5 firmware `4.31` the refusal is `firmware
4.31 has only an unverified SimulationAccess profile`, because the schema's
`simulation_access` and `simulation_write` profiles are still `unverified`. On
every other camera, including the X-S20, the refusal is `firmware X is not in
the SimulationAccess compatibility matrix for MODEL; supported firmware: none`,
because no camera other than the X-T5 declares a `preflight` block at all. See
the [support matrix](../support.md) for the current per-camera state and
[the fail-closed safety model](../explanation/fail-closed-safety-model.md) for
why `list`, `get`, and `export` share the write path's preflight.

A simulation is a camera custom-setting slot such as C1-C7. Slot count is
camera-specific and generated as `SLOTS`.

```sh
fujicli simulation list
fujicli simulation get c1
fujicli simulation export c1 c1.json
fujicli simulation set c1 \
  --target-serial-sha256 SHA256_FROM_DEVICE_INFO \
  --film-simulation reala-ace \
  --grain-effect weak-small \
  --white-balance auto
fujicli simulation import c1 c1.json \
  --target-serial-sha256 SHA256_FROM_DEVICE_INFO
```

The grammar remains visible even when runtime policy disables the operation.
Consult the [support matrix](../support.md) before using it.

Imports require complete profiles exported for the connected camera; they are
not partial updates. Missing settings, unknown or misspelled fields, and files
larger than 1 MiB are rejected before a setting is written.

Use `simulation set` for partial updates. Unspecified fields are read from the
camera and the result is validated. If a multi-setting update fails, the CLI
attempts to restore the original slot.

Writing a slot selects it on the camera while the recipe is written and
verified. Like the read commands, `set` and `import` then put the camera's
custom-setting selector back on the slot it had before the command ran, so a
successful write never changes which slot the camera is using.

A failed restoration, of the recipe or of the previously selected slot, reports
unknown camera state, not success. `set` and `import` require the lowercase
SHA-256 fingerprint of the connected camera's PTP serial, obtained from
`device info`.

That binding prevents a bus/address change or a second attached body from
receiving the write.

The exact setting flags come from FML. Use `simulation set --help` for the
installed build. Aliases such as `auto` and `Auto` map to the same variant, and
many options accept short forms such as `mono` for `monochrome`.

Numeric options accept negative values as separate tokens. If a string or enum
value begins with `-`, use `--option=-value`; a detached `--next-flag` remains a
flag, and the missing value is a usage error.

`simulation get` and `list` accept `--json`. `get`, `export`, and `list` are not
wire-level reads because selecting a stored profile temporarily writes the
custom-setting selector.

## Image Commands

```sh
fujicli image render \
  --target-serial-sha256 SHA256_FROM_DEVICE_INFO \
  input.raf out.jpg

fujicli image render --simulation-file c1.json \
  --target-serial-sha256 SHA256_FROM_DEVICE_INFO \
  input.raf out.jpg

fujicli image render --draft \
  --target-serial-sha256 SHA256_FROM_DEVICE_INFO \
  input.raf out.jpg

fujicli image recover \
  --target-serial-sha256 SHA256_FROM_DEVICE_INFO \
  424242 recovered.jpg

fujicli image recover --delete-after-save \
  --target-serial-sha256 SHA256_FROM_DEVICE_INFO \
  424242 recovered.jpg
```

The handle passed to `image recover` names an object in the camera's internal
conversion store. In USB RAW CONV./BACKUP RESTORE mode the memory card is not
exposed, so the handle is meaningful only on the same camera and until that
object is deleted. A stale handle fails with `InvalidObjectHandle (0x2009)`.

Render layers a simulation file, when provided, then inline setting overrides.
Unset fields come from the camera's current RAW-conversion profile. Direct slot
selection is unavailable until its still/movie selector domain is verified.

RAF input is limited to 512 MiB and simulation JSON to 1 MiB. Render accepts
only a structurally bounded X-T5 RAF. Inputs are read and the output transaction
is opened before the first camera mutation.

RAF upload and JPEG fetch use progress-aware large-transfer timing. Framed
`DeviceBusy` responses during handle polling receive bounded backoff within the
five-minute render deadline.

If stable handles are not observed before the deadline, the camera may still be
processing. The CLI retains every observed handle, suppresses automatic session
close, and does not replay the trigger or delete a possible result.

RAW conversion snapshots the conversion profile before upload, verifies the
requested profile by raw readback, and restores and verifies the exact snapshot
after fetching the rendered object.

A handle is owned only after two stable polls and `GetObjectInfo` identify
exactly one EXIF/JPEG object. The fetched byte count and JPEG structure are
validated before local publication.

If polling, object information, fetch, size, or JPEG validation fails, the
error lists every observed candidate handle and performs no deletion.

`image recover` uses a read-only recovery-fetch preflight. Its optional
`--delete-after-save` cleanup has a separate destructive preflight after the
local file commits.

Output commit and recovery semantics are in
[Output and JSON](output-and-json.md#image-publication-and-recovery).

## Contributor-only Commands

The production binary has no reverse-engineering subcommand. Discovery and
probe workflows use the gated, non-distributable `fujicli-dev` crate; see the
[reversing guide](../../contributors/reversing.md).

## Completion Command

```sh
fujicli completion bash
fujicli completion zsh
fujicli completion fish
fujicli completion powershell
```

The command writes the generated script to stdout and diagnostics, if any, to
stderr. See [installation](../installation.md#shell-completions) for file
placement guidance.
