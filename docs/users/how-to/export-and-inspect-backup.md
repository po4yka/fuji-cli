# Export and Inspect a Backup

A camera-native backup payload is opaque. `fujicli` wraps it in a versioned
artifact with bounded metadata, source identity, payload length, and SHA-256
fingerprints.

Before starting, verify the exact camera status in the
[support matrix](../support.md), close other PTP clients, and choose a new output
path. Backup commands reject emulation.

## Export to a File

```sh
fujicli backup export camera.fbk
```

For a file target, success prints the complete artifact SHA-256. With `--json`,
it prints a JSON object instead.

Save that fingerprint in a trusted record independent of the artifact. A hash
supplied with an untrusted file does not establish provenance.

The native payload is limited by the 128 MiB PTP transport ceiling. Byte
progress may extend the transfer phase up to a fifteen-minute hard cap.

File output is written to a temporary file in the destination directory,
synced, and atomically renamed only after the complete artifact is available.
An existing destination is not replaced unless `--force` is supplied.

`--force` can atomically replace an existing regular file. Directories and
symbolic links are rejected. If the process is forcibly interrupted, the old
destination remains intact and a recoverable `.fujicli-*.tmp` file may remain.

After confirming that no `fujicli` process is writing the temporary file, it
can be removed.

## Inspect Offline

```sh
fujicli backup inspect camera.fbk
fujicli backup inspect camera.fbk --json
```

Inspection verifies magic, format version, exact framing, manifest fields,
payload length, and payload SHA-256. It reports the source model, firmware,
serial fingerprint, payload fingerprint, and complete artifact fingerprint.

Inspection is offline, but the meaningless `--emulate` combination is still
rejected. The exact JSON contract is documented in
[Output and JSON](../reference/output-and-json.md).

## Export to Standard Output

Use `-` as the output path only when a binary consumer is ready:

```sh
fujicli backup export - > camera.fbk
```

The stdout stream is binary-only and contains no fingerprint. Record trust
information through a separate export or inspection workflow rather than
mixing text into the artifact stream.

Backup artifacts can contain private camera state. Do not publish them or raw
verbose logs. Review the [physical-evidence model](../explanation/physical-evidence-model.md)
before sharing a compatibility result.
