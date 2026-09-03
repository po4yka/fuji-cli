# Cameras

A _camera_ block declares a model: how it talks USB-side, what generation it
belongs to, and what features `fujicli` should expose for it. Defined in
[`fml/camera.cue`](../../fml/camera.cue).

```cue
cameras: <id>: #Camera & {
    spec: {
        name:        string          // marketing name
        generation:  #RefGeneration  // id of a generation
        _generation: _               // expands to generations[generation].spec
        usb:         #USB
        features?:   #Features
    }
}
```

`<id>` is the camera's stable Rust ident; every generated struct and constant is
derived from it (e.g. `x_t5` -> `XT5`, `XT5Simulation`, `XT5RenderProfile`,
`C_X_T5` in the support registry).

## USB

```cue
usb: {
    vendor_id:  uint16 | *0x04cb   // Fujifilm by default
    product_id: uint16             // required
    chunk_size_ceiling: uint | *1048576 // PTP bulk transfer ceiling, default 1 MiB
}
```

`chunk_size_ceiling` is an evidence-backed upper bound, not the initial transfer
size. Runtime starts conservatively, packet-aligns both directions, and may
promote only read-only traffic after successful large transfers. Do not raise a
ceiling without trace and physical-device evidence.

## PTP Identity and Preflight Profiles

State-changing support is separate from the feature toggle. An exact PTP
identity and one profile per verified firmware/operation describe the safety
gate:

```cue
ptp: {
    manufacturer: "FUJIFILM"
    model:        "X-T5"
}
preflight: [{
    operation:               "simulation_access"
    status:                  "unverified"
    firmware:                "4.31"
    minimum_battery_percent: 100
    allowed_usb_modes:       [0x6]
    required_operations:     [0x1001, 0x1014, 0x1015, 0x1016]
    required_properties: [
        {code: 0xD16E, data_type: 0x0004, writable: false},
        {code: 0xD36B, data_type: 0xFFFF, writable: false},
    ]
}]
```

Temporary simulation reads and persistent simulation writes use separate
profiles: access needs a writable selector but only readable setting
properties. X-T5 keeps both profiles unverified until physical evidence binds
`0xD18C` to its still/movie custom-setting domain. RAW recovery likewise
separates read-only object fetch from the optional destructive cleanup profile.
The runtime requires an exact firmware string and a `verified` profile. Each
operation/property must also be advertised by `GetDeviceInfo`; every property
is inspected with `GetDevicePropDesc`.
Optional `data_type` pins the PTP type, while `writable: true` requires a
writable descriptor (`false` permits either because some read paths share a
property with a write profile). Do not copy a profile to another firmware or
model without captured traffic and a physical-device run.

### Static descriptors

Some cameras refuse `GetDevicePropDesc` with a PTP response code for every
property in a mode while still serving `GetDevicePropValue` (the X-T5 on 4.31
in USB mode `0x6` does; its firmware image confirms the DeviceInfo for that mode
is assembled at run time, see
[x-t5-firmware-4.31-static-analysis-2026-09-03](../internals/x-t5-firmware-4.31-static-analysis-2026-09-03.md)).
A required property may then carry a `static_descriptor` that the runtime
substitutes for the refused one:

```cue
{
    code:      0xD18C
    data_type: 0x0004
    writable:  true
    static_descriptor: {
        evidence: "FWUP0030.DAT 4.31 descriptor table: UINT16 get/set enumeration ..."
        form: {kind: "enumeration", values: [1, 2, 3, 4, 5, 6, 7]}
    }
}
```

`form` is `{kind: "none"}`, `{kind: "enumeration", values: [...]}`, or
`{kind: "range", minimum, maximum, step}`. A static descriptor exists only to
authorize writes, so CUE forces `data_type` and `writable: true`, and strings
(`0xFFFF`) may only use `form: none`. Codegen additionally rejects a pinned
`data_type` that differs from the datatype the option's own wire codec writes.
At run time the live value must decode exactly as the pinned datatype and fall
inside `form`; otherwise preflight fails closed. `evidence` is free text that
names the source of the shape (firmware image, device audit, prior art) and is
carried into the generated registry for error messages.

A static descriptor does not change a profile's `status`. X-T5 simulation
profiles carry them so the descriptor refusal no longer blocks the permit, but
they remain `unverified` until a physical write run resolves the `0xD18C`
still/movie namespace question.

## Firmware Capabilities

Feature shape and wire compatibility are separate. `capabilities` layers
generation defaults, model facts, and exact firmware overrides; codegen
materializes one profile per firmware and runtime lookup never falls back to a
nearby version:

```cue
capabilities: {
    generation: _generation.capabilities
    model: option_overrides: [/* every enum used by X-T5 write paths */]
    firmware: {
        "3.01": {}
        "4.00": option_overrides: [{
            ref: "film_simulation"
            allowed_values: [/* includes reala_ace */]
        }]
        "4.31": {
            option_overrides: [/* exact logical-value allowlist */]
            raw_conversion: {
                id: "x_t5-4.31-raw-layout-unverified"
                evidence: {status: "unverified", manifests: []}
                binding: {usb_modes: [0x6]}
                read: {
                    profile_code: "ff179502", header_padding: 0x1ee
                    declared_field_count: 29, total_length: 625
                    fields: [/* 28 exact read-side field ids */]
                }
                write: {
                    profile_code: "ff179502", header_padding: 0x1ee
                    declared_field_count: 29, total_length: 629
                    fields: [/* 29 assumed write-side field ids */]
                }
            }
        }
    }
}
```

An enum capability owns its allowed logical ids and its firmware-specific wire
map. A scalar wire value is canonical for writes; a list means canonical first
plus accepted read aliases. Codegen rejects missing or ambiguous mappings.
It also rejects a verified operation whose exact firmware profile omits any
enum consumed by that operation.
Simulation PTP reads and writes use the selected profile directly. RAW layouts
are exact-firmware descriptors with separate read/write counts, lengths, and
orders. `write_verified` is reserved and currently rejected by codegen until
evidence manifests/hashes, camera state, and lossless opaque-byte preservation
are machine-checked; all current descriptor states therefore cannot authorize
an upload, D185 write, or conversion trigger. Every enum consumed
by the X-T5 selector, simulation, or RAW path is explicitly pinned so there is
no implicit global encoding fallback.

Documented capability presence does not authorize writes. Only a matching
`verified` preflight operation can create a validated session; X-T5 `3.01` and
`4.00` capability entries are regression/documentation fixtures, while current
backup mutation support remains restricted to `4.31`. Custom-setting access is
disabled because its still/movie namespace is ambiguous. RAW conversion is also
disabled until the evidence validator and state-aware lossless codec exist and
the 4.31 descriptor passes golden capture and HIL verification.

## Features

```cue
features?: {
    backup?:     true
    simulation?: #Simulation
    render?:     #Render
}
```

A missing key means "not supported".

### Backup

`backup` is the only feature without an inline schema. It's a boolean toggle
because the underlying PTP exchange is uniform across the supported models.

### Simulation

```cue
simulation: {
    slots: uint             // C1 .. C<slots>
    settings: [...{
        id:  string | *ref  // defaults to ref
        ref: #RefOption     // must name a declared option
    }]
    transformations?: [...]
    rules?:           [...]
}
```

`settings` is the **ordered** list of options this camera's simulation profile
is composed of. Order matters for PTP read/write; the codegen runs them in
declaration order, perturbed only by gating dependencies inferred from rules.
See [rules](rules.md) for how validation, repair, and read-gating are derived
from the rule list, and [grammar](grammar.md) for the predicate language those
rules use.

If your camera shares its generation's profile verbatim, set
`settings: _generation._simulation.settings` and inherit the rules the same way.

### Render

```cue
render: {
    profile_code:   uint32  // identifies the wire format
    header_padding: uint32  // bytes between profile code and fields
    fields: [...#Field]     // ordered list of slots
    transformations?: [...]
    rules?:           [...]
}

#Field:
    | {id: string, ref: #RefOption, skip_read?: true, skip_write?: true}
    | {id: string,                  skip_read?: true, skip_write?: true}
```

Fields fall into two kinds:

- **`ref` fields** inherit type and encoding from a declared option
  (`{ref: "image_size"}`). The `id` defaults to the ref. The field participates
  in CLI args and bidirectional value mapping.
- **Inline fields** carry only an `id`, no `ref`. They are bare `i32` slots used
  for headers, trailers, and undocumented padding values (`{id: "head_0"}` /
  `{id: "tail_0", skip_read: true}`). They never appear on the CLI.

`skip_read` / `skip_write` make a slot one-way.

`profile_code` is part of the wire format. The camera enforces it on read.
Reverse it before guessing (see [reversing](../contributors/reversing.md)).
`header_padding` is also camera-specific wire data and has no default. Record
the exact observed byte count for every render-capable camera; the X-T5 uses
`0x1ee` bytes.

## CUE-Side Validation

Each feature block carries an `_validation` field that CUE evaluates at export
time:

```cue
_validation: {
    ids: list.UniqueItems & [for s in settings {s.id}]
}
```

Setting / field `id`s must be unique. If you accidentally introduce a duplicate,
`cue export` fails with a precise location.

## Available Predicates

Inside a camera's simulation or render block, the `#Grammar` you write rules and
transformations in is _typed by the local setting list_. That means:

- A predicate `{ref: "image_size", lt: 5}` is rejected at CUE export because
  `image_size` is an enum.
- A predicate `{ref: "monochromatic_color_temperature", gte: 0}` only works
  inside a camera/generation whose settings include that field.
- `{ref: "monochromatic_color_tint", present: true}` works against any `id` in
  scope.

This pre-flight check catches the most common bugs before codegen ever runs. See
[grammar](grammar.md) for the full leaf vocabulary.
