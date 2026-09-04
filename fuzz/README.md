# fujicli-fuzz

Coverage-guided fuzzing for the PTP wire parsers. Every target feeds a
camera-shaped byte stream into one parser and asserts the parser never
panics, hangs, or aborts. The crate is deliberately outside the workspace:
it has its own lockfile, and the production binary never links Honggfuzz.

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

Prerequisites: the pinned stable toolchain, a C toolchain, `libusb-1.0`
headers, CUE on `PATH` (the `fujicli` build script exports `fml/`), and
Honggfuzz (`cargo install honggfuzz --version 0.5.62 --locked`). On Linux,
Honggfuzz also needs `binutils-dev`, `libunwind-dev`, `libblocksruntime-dev`,
and `liblzma-dev`. Run the native fuzzer on Linux: Honggfuzz 0.5.62's bundled
macOS driver rejects current macOS versions and ships an x86_64-only crash
reporter. `cargo check` remains portable across supported project hosts.

```sh
# Check every target.
cargo check --locked --all-targets

# Time-boxed smoke run of one target.
HFUZZ_BUILD_ARGS="--locked --jobs 4" \
HFUZZ_RUN_ARGS="--run_time 30 --exit_upon_crash" \
  cargo hfuzz run ptp_container_info

# Longer run.
HFUZZ_BUILD_ARGS="--locked --jobs 4" HFUZZ_RUN_ARGS="--run_time 600" \
  cargo hfuzz run ptp_descriptor
```

Run state and findings land in `fuzz/hfuzz_workspace/` and built targets in
`fuzz/hfuzz_target/`; both are git-ignored. Do not add nightly-only sanitizer
flags: stable Honggfuzz keeps feedback-driven fuzzing and crash detection, but
does not provide the previous compiler sanitizer instrumentation.

## Rules

- A crash, hang, or out-of-memory report is a finding. Turn the reproducing
  input into a unit test next to the parser it breaks, fix in a separate
  commit, and keep the artifact out of git.
- Every new wire parser that reads device bytes gets a target in the same
  release that adds it.
- Never weaken an oracle to make a run pass; narrowing it hides real
  divergences between the decoder and the encoder.
