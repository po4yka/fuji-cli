# Output and JSON Reference

Stdout carries requested data. Diagnostics and logs use stderr. Exit status,
stream selection, JSON shape, and file-publication behavior are public process
contracts.

## JSON

`-j` and `--json` are available for `device list`, `device info`, `simulation list`,
`simulation get`, file-targeted `backup export`, `backup inspect`, and `backup
import --dry-run`.

Commands whose output is binary or has no structured result do not accept
`--json`. Without it, structured command output is human-readable text.

Each JSON invocation writes exactly one UTF-8 JSON document followed by a
newline. Member order is not semantic; field names, value types, nesting, and
document framing are part of the public contract.

`backup inspect --json` contains `artifactSha256`, `formatVersion`,
`payloadLen`, `payloadSha256`, `purpose`, and `source`.

The `source` object has `cameraName`, `firmware`, `manufacturer`, `model`,
`productId`, `serialSha256`, and `vendorId`.

`productId` and `vendorId` are numeric. A consumer that closes stdout after
receiving enough data is treated as a successful early termination, not an
operational failure.

## Logging and Privacy

Repeat `-v` to raise verbosity: `-v`, `-vv`, or `-vvv`. Requested data remains
on stdout; logs and diagnostics remain on stderr.

Verbose output can contain device and host context even when payloads are
omitted. Privacy-review it before attaching any excerpt to a public report.

## Standard Input and Output

Use `-` instead of an input or output filename where the leaf command permits
it. RAF input is limited to 512 MiB, simulation JSON to 1 MiB, and a native
backup payload to the 128 MiB PTP transport ceiling.

Backup export to stdout is binary-only and emits no artifact fingerprint into
the stream. Stdout is not a durable receipt for rendered JPEG output, so render
leaves the camera object available for explicit recovery.

`fujicli completion SHELL` also writes its generated script to stdout. Redirect
it to the standard user completion location for that shell.

## Atomic File Output

`backup export`, `simulation export`, `image render`, and `image recover` do not
replace an existing output path by default. `--force` atomically replaces an
existing regular file; directories and symbolic links are always rejected.

Path outputs are written to a temporary file in the destination directory,
synced, and atomically renamed only after complete output is available. A
forced interruption leaves the old destination intact.

A recoverable `.fujicli-*.tmp` file may remain. Remove it only after confirming
that no `fujicli` process is still writing there.

The safety recovery file required by `backup import` is always created without
clobbering and has no `--force` override.

## Image Publication and Recovery

For a path output, a JPEG is written, synced, and atomically committed before
`DeleteObject`. Fetch, validation, or local-save failure leaves the camera
object intact and reports its handle.

A cleanup or profile-restoration failure after a successful save keeps the
saved JPEG, reports the handle, and exits unsuccessfully. Render to stdout also
leaves the camera object for explicit recovery.
