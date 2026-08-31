# Fail-closed Safety Model

`fujicli` separates command grammar from authority. A command can be present in
help while runtime policy refuses it for the connected camera. The
[support matrix](../support.md) remains the source of truth for current models.

## Preflight Before Mutation

Before backup restore, simulation access or write, and RAW conversion, the CLI
checks physical USB identity, exact PTP identity and serial, firmware matrix
entry, USB mode, and battery.

It also checks advertised operations and properties plus live property
descriptors.

Unknown firmware, unverified matrix entries, incomplete descriptors, and
unsupported values fail before the intended mutation. Normal production
commands have no experimental override.

The current X-T5 mutation policy requires exact PTP firmware `4.31`, USB mode
`0x6`, and 100% battery. The battery threshold is deliberately conservative;
it is not claimed to be Fujifilm's minimum.

Film-simulation values are checked against the exact firmware capability
profile before selector or upload mutation. Reala Ace is absent from the X-T5
`3.01` profile and present from `4.00`.

Those profiles record option availability only. They do not authorize writes;
current authorization remains narrower and is documented in the
[support matrix](../support.md).

## Read-like Simulation Commands Can Write

Fujifilm cameras have separate still and movie C1-C7 namespaces. Available PTP
evidence does not establish which namespace selector `0xD18C` addresses.

`simulation list`, `get`, and `export` are therefore not wire-level reads.
Selecting a stored profile temporarily writes that selector, so these commands
share the same fail-closed requirement as `set` and `import`.

Re-enabling the commands requires a physical still/movie matrix that identifies
the namespace and verifies selector restoration and persistence across
reconnect and power-cycle.

## Emulation Does Not Grant Authority

`--emulate VENDOR:PRODUCT` changes only the generated logical model. It does not
change physical USB identity or add camera support.

Only `device info` is read-only under emulation. All simulation commands,
backups, and RAW rendering reject emulation. Selecting a device by bus/address
does not bypass physical identity checks.

## Unknown State Stops Automation

When several simulation settings are applied and a write fails, the CLI tries
to restore the original slot. A failed restoration is unknown camera state, not
success.

A backup restore accepted by PTP is not independent proof of persistence after
reboot. Timeout, disconnect, or an ambiguous final response after a mutation
also leaves state unknown.

Rendering uses the same principle. Ambiguous handle discovery retains every
candidate, suppresses automatic session close, and does not replay a trigger or
delete a possible result.

Exit status `3` represents this boundary. Automation must not retry. The
physical camera must be inspected first; see the
[exit-code reference](../reference/exit-codes.md).

## Integrity Is Not Authenticity

The backup envelope and SHA-256 values detect corruption, truncation,
substitution, and target mismatch. Fujifilm provides no signature or public
parser for the opaque native payload.

`--expect-sha256` must therefore come from a trusted record independent of the
artifact. A hash delivered by the same untrusted party as the file does not
establish provenance.
