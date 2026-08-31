# Physical-evidence Model

Software evidence and camera evidence answer different questions. The
[support matrix](../support.md) is the only model-by-model compatibility table.
Its `Y` entries require a run on that exact physical model.

## Evidence Levels

| Evidence | What it establishes | What it does not establish |
| --- | --- | --- |
| FML schema and generated Rust | The model is represented and the generated contract compiles. | USB reachability, firmware behavior, or safe mutation. |
| Unit, process, and fixture tests | Local parsing, grammar, dispatch, and modeled failure behavior. | Physical USB/PTP behavior or persistence. |
| Captured traffic or vendor evidence | Observed wire facts for the captured model, mode, firmware, and workflow. | A nearby model, firmware, or unobserved selector domain. |
| Physical-camera run | The recorded command and outcome on the exact device state. | Other firmware, USB modes, bodies, or commands not exercised. |

Emulation is software evidence. It selects a generated logical model while the
physical USB identity remains unchanged, so it never upgrades a support claim.

## Mutation Evidence

A successful transport exchange or PTP `OK` response is not enough to claim a
state-changing feature. Record the outcomes separately:

1. Transport: the intended device and interface were used without an ambiguous
   timeout or disconnect.
2. PTP: the expected operation and response completed.
3. Semantic state: write, read back, and compare the intended value.
4. Restoration: verify rollback or deletion where the workflow requires it.
5. Persistence: reconnect or power-cycle when the claim includes persistence.

If any required outcome is unknown, report the state as unknown. Do not retry a
mutation automatically or promote a support-table claim.

## Record a Device Run

Record the exact model, firmware, USB mode, battery when relevant, host,
`fujicli` version or commit, command, and observed semantic result. State which
operations were not exercised.

Keep generated-code inspection, local tests, hosted CI, captured traffic, and
physical-camera evidence distinct. A green result in one category cannot stand
in for another.

## Share Evidence Safely

Privacy-review diagnostics before attaching them to a public issue or pull
request. Do not publish raw serials, backup artifacts, private RAF/JPEG files,
custom-setting names, full paths, or unreviewed `-vvv` traces.

Prefer the minimized serial SHA-256 fingerprint printed by `device info` when a
stable correlation value is necessary. Include only the smallest diagnostic
excerpt that supports the claim.

Follow the matrix's [reporting contract](../support.md#reporting-compatibility)
when submitting a result.
