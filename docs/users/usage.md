# Usage

```
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
`--emulate VENDOR:PRODUCT` forces fujicli to treat the connected device as a
different model - useful for development; see
[camera support](support.md#emulation-mode).

## Devices

```sh
# List connected supported cameras.
fujicli device list

# Print extended info for the currently selected camera (model, serial,
# battery, USB mode).
fujicli device info
```

## Backups

Backups are camera-native blobs; treat them as opaque.

```sh
fujicli backup export camera.fbk  # write to file
fujicli backup export -           # write to stdout
fujicli backup import camera.fbk
```

Backup imports are limited to 256 MiB. The complete input is read and checked
before the CLI opens a camera connection.

File exports are written to a temporary file in the destination directory,
synced, and atomically renamed only after the complete output is available. If
the process is forcibly interrupted, the previous destination remains intact;
a recoverable `.fujicli-*.tmp` file may remain beside it. After confirming that
no `fujicli` process is still writing there, that temporary file can be removed.

## Simulations

A _simulation_ is one of the camera's custom-setting slots (e.g. C1-C7). The
number of slots is per-camera (`SLOTS` in the generated code).

```sh
# List slots with their assigned names.
fujicli simulation list

# Read one slot.
fujicli simulation get c1

# Update fields on a slot. Any subset is allowed; the rest is read from
# the camera and the result validated.
fujicli simulation set c1 \
  --film-simulation reala-ace \
  --grain-effect weak-small \
  --white-balance auto

# Round-trip JSON to disk.
fujicli simulation export c1 c1.json
fujicli simulation import c1 c1.json
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
Pass `--json` for machine-readable output on `get`/`list`.

## Images

```sh
# Render a RAF in-camera using the active settings.
fujicli image render input.raf out.jpg

# Render using slot C1's settings.
fujicli image render --slot c1 input.raf out.jpg

# Render using a previously-exported simulation.
fujicli image render --simulation-file c1.json input.raf out.jpg

# Override individual fields on top of any of the above.
fujicli image render --slot c1 \
  --film-simulation classic-chrome \
  --grain-effect off \
  input.raf out.jpg

# Faster but lower quality preview render.
fujicli image render --draft input.raf out.jpg
```

The render command always layers in this order: simulation source (slot or
file), then any inline `--<field>` overrides. Fields your CLI flags don't set
are pulled from the camera's current state.

Use `-` in place of any input or output filename to read from stdin or write to
stdout. RAF input is limited to 512 MiB; simulation JSON remains limited to
1 MiB. Inputs are read before the camera connection is opened.

## Output and Logging

`-j / --json` switches list/get commands to pretty JSON. Without it, output is
human-readable.

`-v` (repeatable: `-v`, `-vv`, `-vvv`) raises log verbosity. For `device
reverse` commands, `-vvv` reports PTP operation metadata and response lengths,
but never response payloads, camera serial numbers, backup contents, or
custom-setting names. Privacy-review diagnostics before attaching them to a bug
report.
