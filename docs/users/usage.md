# Usage

```text
A CLI to manage Fujifilm devices, simulations, backups, and rendering

Usage: fujicli [OPTIONS] <COMMAND>

Commands:
  device      Manage devices
  simulation  Manage film simulations
  backup      Manage backups
  image       Manage and render images
  help        Print this message or the help of the given subcommand(s)

Options:
  -j, --json               Format output using json
  -v, --verbose...         Log extra debugging information (multiple instances increase verbosity)
  -d, --device <DEVICE>    Manually specify target device using USB <BUS>.<ADDRESS>
      --emulate <EMULATE>  Treat device as a different model using <VENDOR_ID>:<PRODUCT_ID>
  -h, --help               Print help
  -V, --version            Print version
```

Every subcommand has a short alias: `device -> d`, `simulation -> s`,
`backup -> b`, `image -> i`. Within a subcommand, common operations are also
aliased (`list -> l`, `get -> g`, `set -> s`, `export -> e`, `import -> i`,
`render -> r`).

The `-d / --device` flag accepts a USB bus/address pair (e.g. `1.4`) and is only
needed when more than one supported camera is plugged in.
`--emulate VENDOR:PRODUCT` selects another generated logical model while the
physical USB identity remains unchanged and must identify a supported camera.
Only `device info` is read-only under emulation. Simulation access first writes
Fujifilm's custom-setting selector, so all simulation commands, backups, RAW
rendering, and reverse commands reject emulation; see
[camera support](support.md#emulation-mode).

## Devices

```sh
# List connected supported cameras.
fujicli device list

# Print extended info for the currently selected camera (model, serial,
# battery, USB mode).
fujicli device info
```

`device list` reads USB descriptors only. It does not claim a camera interface
or open a PTP session; use `device info` when live camera properties are needed.

## Backups

The camera-native backup payload is opaque. The normal commands wrap it in a
versioned fujicli artifact containing bounded metadata, source identity,
payload length, and SHA-256 fingerprints.

```sh
fujicli backup export camera.fbk
fujicli backup inspect camera.fbk
fujicli backup import camera.fbk --dry-run
fujicli backup import camera.fbk \
  --yes \
  --recovery-backup before-import.fbk \
  --expect-sha256 SHA256_RECORDED_AT_EXPORT
```

`backup inspect` is offline. It verifies magic, format version, exact framing,
manifest fields, payload length, and payload SHA-256, then prints the source
model, firmware, serial fingerprint, payload fingerprint, and complete artifact
fingerprint. Pass `--json` for machine-readable output.

When export targets a file, it prints the complete artifact SHA-256 (or a JSON
object under `--json`) so automation can save an independent trusted record.
Export to stdout remains binary-only and prints no fingerprint into the stream.

The native payload remains limited to the PTP transport ceiling of 128 MiB. The
complete artifact is read and cryptographically checked before the CLI opens a
camera connection. Import then requires an exact source/target schema camera,
USB product, live PTP model, manufacturer, and firmware match. By default it
also requires the source camera serial fingerprint. To transfer settings to a
different body of the same model and firmware, obtain its fingerprint with
`fujicli device info`, then pin that target during the dry run and restore:

```sh
fujicli backup import camera.fbk --dry-run \
  --target-serial-sha256 SHA256_FROM_DEVICE_INFO
```

```sh
fujicli backup import camera.fbk \
  --yes \
  --recovery-backup before-import.fbk \
  --expect-sha256 SHA256_RECORDED_AT_EXPORT \
  --target-serial-sha256 SHA256_FROM_DRY_RUN
```

Destructive import requires `--yes`, the complete artifact's
`--expect-sha256`, and a new `--recovery-backup` file. The CLI exports the
target's current state, syncs it, and creates that file without clobbering an
existing path before it sends `SendObjectInfo`. A recovery artifact is bound to
that exact camera serial. Any validation, compatibility, export, or
recovery-write failure stops before restore metadata is sent.

The envelope and hashes detect corruption, truncation, accidental substitution,
and target mismatch; they do not authenticate Fujifilm's opaque native payload.
Fujifilm exposes no signature or public parser here, so the value passed to
`--expect-sha256` must come from a trusted export/inspection record independent
of the artifact being imported. A hash supplied by the same untrusted party as
the file does not establish provenance.

All backup commands reject `--emulate`, including offline `backup inspect`,
where the option is meaningless.

Destructive import from stdin is additionally disabled unless `--allow-stdin`
is supplied. The external artifact fingerprint, explicit serial binding,
confirmation, and recovery path form the unattended-automation contract. Do
not automatically retry a restore after a timeout, disconnect, or other wire
error: the camera state is unknown. A successful command means that the PTP
restore operation was accepted, not that post-reboot persistence was
independently verified.

Restore and export use large-transfer timing: byte progress may extend the
data phase up to a fifteen-minute hard cap, and a fully uploaded restore may
wait up to ten minutes for its final camera response. A ten-second USB slice is
not reported as a failed restore while the operation-specific deadline remains.

File exports are written to a temporary file in the destination directory,
synced, and atomically renamed only after the complete output is available. If
the process is forcibly interrupted, the previous destination remains intact;
a recoverable `.fujicli-*.tmp` file may remain beside it. After confirming that
no `fujicli` process is still writing there, that temporary file can be removed.

## Simulations

A _simulation_ is one of the camera's custom-setting slots (e.g. C1-C7). The
number of slots is per-camera (`SLOTS` in the generated code).

X-T5 simulation access is currently disabled. The camera has separate still
and movie C1-C7 namespaces, while available PTP evidence does not establish
which namespace selector `0xD18C` addresses. Consequently `list`, `get`,
`export`, `set`, and `import` fail during preflight before the selector or any
profile property is written. The command grammar below remains the intended
interface for a future firmware profile backed by physical domain evidence.

```sh
# List slots with their assigned names.
fujicli simulation list

# Read one slot.
fujicli simulation get c1

# Update fields on a slot. Any subset is allowed; the rest is read from
# the camera and the result validated.
fujicli simulation set c1 \
  --target-serial-sha256 SHA256_FROM_DEVICE_INFO \
  --film-simulation reala-ace \
  --grain-effect weak-small \
  --white-balance auto

# Round-trip JSON to disk.
fujicli simulation export c1 c1.json
fujicli simulation import c1 c1.json \
  --target-serial-sha256 SHA256_FROM_DEVICE_INFO
```

Simulation imports accept complete profiles exported for the connected camera;
they are not partial updates. Missing required settings, unknown or misspelled
fields, and files larger than 1 MiB are rejected before any setting is written.
Use `simulation set` for a partial update. If applying several settings fails,
the CLI attempts to restore the original slot. A failed restore is reported as
an unknown camera state rather than success.

The exact set of `--<field>` flags is generated from the FML schema; run
`fujicli simulation set --help` to list what your build supports. Aliases work -
both `--white-balance auto` and `--white-balance Auto` parse to the same
variant, and most options accept short forms (e.g. `mono` for `monochrome`).
Numeric options accept negative values as separate tokens. For a string or enum
value that itself starts with `-`, use the attached form `--option=-value`; an
unattached `--next-flag` remains a flag and a missing value is a usage error.
Pass `--json` for machine-readable output on `get`/`list`.

`get`, `export`, and `list` are not wire-level reads: selecting a stored profile
would temporarily write `0xD18C`. They therefore share the same fail-closed
domain requirement as `set` and `import`. Re-enabling them requires a physical
still/movie matrix that identifies the selected namespace and verifies selector
restoration and persistence across reconnect and power-cycle.

`set` and `import` require the lowercase SHA-256 fingerprint of the connected
camera's PTP serial. Obtain it from `fujicli device info`; the binding prevents
a bus/address change or a second attached body from receiving the write.

## Images

```sh
# Render a RAF in-camera using the active settings.
fujicli image render \
  --target-serial-sha256 SHA256_FROM_DEVICE_INFO \
  input.raf out.jpg

# Render using a previously-exported simulation.
fujicli image render --simulation-file c1.json \
  --target-serial-sha256 SHA256_FROM_DEVICE_INFO \
  input.raf out.jpg

# Override individual fields on top of the active or file-provided profile.
fujicli image render \
  --target-serial-sha256 SHA256_FROM_DEVICE_INFO \
  --film-simulation classic-chrome \
  --grain-effect off \
  input.raf out.jpg

# Faster but lower quality preview render.
fujicli image render --draft \
  --target-serial-sha256 SHA256_FROM_DEVICE_INFO \
  input.raf out.jpg

# Recover a retained JPEG reported by a failed render. Recovery keeps the
# camera object unless deletion is requested explicitly.
fujicli image recover \
  --target-serial-sha256 SHA256_FROM_DEVICE_INFO \
  424242 recovered.jpg
fujicli image recover --delete-after-save \
  --target-serial-sha256 SHA256_FROM_DEVICE_INFO \
  424242 recovered.jpg
```

The render command layers a simulation file, when supplied, and then any inline
`--<field>` overrides. Fields neither source sets are pulled from the camera's
current RAW conversion profile. Direct slot selection is unavailable until the
still/movie selector domain is physically verified.

RAF upload and rendered-JPEG fetch use progress-aware large-transfer timing.
After the conversion trigger, framed `DeviceBusy` responses from handle polling
are retried with bounded backoff inside the five-minute render deadline. If that
deadline expires before stable handles are observed, the camera may still be
processing: the CLI retains every observed handle, suppresses automatic session
close, and does not replay the trigger or delete a possible result.

Before backup restore, simulation access/write, or RAW conversion, the CLI
checks the physical USB identity, exact PTP identity and serial, firmware
matrix entry, USB mode, battery, advertised operations/properties, and live
property descriptors. Unknown firmware and unverified matrix entries fail
closed; normal commands have no experimental override. The current X-T5 policy
requires firmware `4.31`, USB mode `0x6`, and 100% battery. The battery value is
a deliberately conservative project threshold, not a claimed Fujifilm minimum.
Film-simulation values are additionally checked against the exact firmware
capability profile before any selector or upload mutation. In particular,
Reala Ace is absent from the X-T5 `3.01` profile and present from `4.00`; those
documented profiles do not themselves enable writes, which remain verified only
for `4.31`.

Use `-` in place of any input or output filename to read from stdin or write to
stdout. RAF input is limited to 512 MiB; simulation JSON remains limited to
1 MiB. Render accepts only a structurally bounded X-T5 RAF. Inputs are read and
the output transaction is opened before the first camera mutation.

RAW conversion snapshots the camera's conversion profile before upload,
verifies the requested profile by raw readback, and restores and verifies the
exact snapshot after the rendered object is fetched. A new handle is owned only
after two stable handle polls and `GetObjectInfo` identify exactly one EXIF/JPEG
object. The fetched byte count and JPEG structure are validated before local
publication.

If polling, `GetObjectInfo`, fetch, size validation, or JPEG validation fails,
the error lists every observed candidate handle and no deletion is attempted.
`image recover` uses a read-only recovery-fetch preflight; its optional
`--delete-after-save` cleanup has a separate destructive preflight after the
local file has committed.

For a path output, the JPEG is written, synced, and atomically committed before
`DeleteObject`. A fetch, validation, or local-save failure leaves the camera
object intact and reports its handle. A cleanup or profile-restore failure after
a successful save keeps the saved JPEG, reports the handle, and exits
unsuccessfully. Stdout is not a durable receipt, so render output written to
stdout leaves the camera object available for explicit recovery.

## Output and Logging

`-j / --json` switches list/get commands to pretty JSON. Without it, output is
human-readable.

`-v` (repeatable: `-v`, `-vv`, `-vvv`) raises log verbosity. For `device
reverse` commands, `-vvv` reports PTP operation metadata and response lengths,
but never response payloads, camera serial numbers, backup contents, or
custom-setting names. Privacy-review diagnostics before attaching them to a bug
report.

## Exit Codes

Exit status is a public contract for scripts and wrappers, alongside stdout,
stderr, and JSON shape. `0` means success. `1` means failure in a state that
is safe to retry after investigating -- a bad argument that clap did not
already catch, a missing file, no camera found, or a preflight rejection
before any camera write. `2` means clap rejected the command line itself
(an invalid subcommand, a missing required argument, or similar). `3` means
a state-changing operation was already sent to the camera and its outcome
could not be confirmed afterward -- do not retry automatically; verify the
camera's actual state first. `130` means the process was interrupted
(Ctrl-C) outside of a camera write.
