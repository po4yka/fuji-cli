# Dry-run and Restore a Backup

Restore is destructive. Confirm that the exact camera, firmware, USB mode, and
operation are authorized in the [support matrix](../support.md). Close every
other PTP client before continuing.

## Validate the Artifact and Target

The CLI reads and cryptographically checks the complete artifact before opening
a camera connection. It then requires an exact source/target match for schema
camera, USB product, live PTP model, manufacturer, and firmware.

By default, the source serial fingerprint must also match the target. Start with
a dry run:

```sh
fujicli backup import camera.fbk --dry-run
```

To transfer settings to a different body of the same model and firmware, obtain
its serial fingerprint with `fujicli device info`, then pin it during dry-run:

```sh
fujicli backup import camera.fbk --dry-run \
  --target-serial-sha256 SHA256_FROM_DEVICE_INFO
```

Use the fingerprint confirmed by the dry run for the restore. Fingerprints must
be exactly 64 lowercase hexadecimal characters.

## Prepare Independent Recovery

Record the complete artifact SHA-256 from a trusted export or inspection. The
artifact envelope detects corruption, truncation, accidental substitution, and
target mismatch, but it does not authenticate Fujifilm's opaque native payload.

Choose a new recovery path. Restore exports the target's current state, syncs
it, and creates that file without clobbering before sending `SendObjectInfo`.
The recovery artifact is bound to that exact camera serial.

Any validation, compatibility, export, or recovery-write failure stops before
restore metadata is sent. The recovery path never has a force override.

## Restore Once

```sh
fujicli backup import camera.fbk \
  --yes \
  --recovery-backup before-import.fbk \
  --expect-sha256 SHA256_RECORDED_AT_EXPORT \
  --target-serial-sha256 SHA256_FROM_DRY_RUN
```

Destructive import requires `--yes`, the complete artifact
`--expect-sha256`, and a new `--recovery-backup` file. A same-body restore can
omit `--target-serial-sha256` because the artifact already binds the source.

Grammar is validated before any input file or camera is opened. `--yes` and
`--recovery-backup` cannot be combined with `--dry-run`; the recovery
destination must be a file path rather than `-`.

Import from stdin is disabled unless `--allow-stdin` is supplied, and that flag
is rejected unless the input is `-`. The trusted artifact fingerprint, serial
binding, confirmation, and recovery path form the automation contract.

## Interpret the Result

A successful command means the PTP restore operation was accepted. It does not
prove that settings persisted after reboot.

Byte progress may extend the restore data phase up to a fifteen-minute hard
cap. A fully uploaded restore may then wait up to ten minutes for the final
camera response.

Ten-second USB slices are not reported as failures while the
operation-specific deadline remains.

After a timeout, disconnect, interruption, or other ambiguous wire result, the
camera state is unknown. Do not retry automatically. Inspect the physical
camera first and follow the [exit-code contract](../reference/exit-codes.md).
