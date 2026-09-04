# fujicli-fuzz

Coverage-guided fuzzing for the PTP wire parsers. Every target feeds a
camera-shaped byte stream into one parser and asserts the parser never
panics, hangs, or aborts. The crate is deliberately outside the workspace:
it has its own lockfile, and the production binary never links libFuzzer.

## Targets

| Target | Entry point | Oracle |
| --- | --- | --- |
| `ptp_descriptor` | `decode_device_prop_desc_for_fuzzing` (gated wrapper for the hand-rolled `DevicePropDesc::decode`) | no panic on any byte string |
| `ptp_object_info` | `decode_exact::<ObjectInfo>` | decode, re-encode, bytes must match the input |
| `ptp_device_info` | `decode_exact::<DeviceInfo>` | decode, re-encode, bytes must match the input |
| `ptp_container_info` | `decode_exact::<ContainerInfo>` | `payload_len()` succeeds exactly when `total_len` covers the header |

The harnesses enter the parsers through `fujicli`'s `fuzzing` feature. The
feature is not in `default`, so distributable binaries never ship the extra
public surface, and the documented encapsulation contract
(`use fujicli::ptp::Ptp;` must not compile) still holds.

## Running

Prerequisites: the pinned nightly toolchain, a C toolchain, `libusb-1.0`
headers, CUE on `PATH` (the `fujicli` build script exports `fml/`), and
`cargo-fuzz` (`cargo install cargo-fuzz --locked`).

```sh
# Build every target (on Apple Silicon pass the host target explicitly).
cargo fuzz build --target aarch64-apple-darwin

# Time-boxed smoke run of one target.
cargo fuzz run ptp_container_info --target aarch64-apple-darwin -- -max_total_time=30

# Longer run writing artifacts for any finding.
cargo fuzz run ptp_descriptor --target aarch64-apple-darwin -- -max_total_time=600
```

Artifacts for findings land in `fuzz/artifacts/` (git-ignored).

## Rules

- A crash, hang, or out-of-memory report is a finding. Turn the reproducing
  input into a unit test next to the parser it breaks, fix in a separate
  commit, and keep the artifact out of git.
- Every new wire parser that reads device bytes gets a target in the same
  release that adds it.
- Never weaken an oracle to make a run pass; narrowing it hides real
  divergences between the decoder and the encoder.
