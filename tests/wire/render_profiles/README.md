# RAW conversion wire evidence

Store minimized, privacy-reviewed D185 payloads by exact model and firmware:

```text
tests/wire/render_profiles/<model>/<firmware>/
  manifest.json
  camera-read-<state>.bin
  xraw-write-<state>-noop.bin
  xraw-write-<state>-<single-setting>.bin
```

`manifest.json` must record the physical/PTP model, exact firmware, USB mode,
camera state, X RAW Studio version when applicable, capture direction and
operation, SHA-256 of the private source PCAP/PCAPNG, SHA-256 of every extracted
payload, and privacy-review status. Never derive a golden payload from the
encoder under test. Do not commit serial numbers, RAF/JPEG data, or unrelated
USB traffic.

A descriptor may become `read_verified` after its passive read payloads and
manifest hashes pass compatibility tests. `write_verified` is reserved and
currently rejected by codegen: accepting it additionally requires a
machine-checked manifest/hash parser, live state binding, lossless no-op
round-trip tests, controlled single-setting write captures, and the physical
HIL matrix. Missing, ambiguous, or contradictory evidence keeps all RAW writes
disabled.
