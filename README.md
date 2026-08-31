# fujicli

`fujicli` is an experimental, schema-driven command-line tool for inspecting
Fujifilm cameras over USB/PTP and running camera-native workflows whose exact
model and firmware contracts have been verified.

[![CI status](https://github.com/po4yka/fuji-cli/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/po4yka/fuji-cli/actions/workflows/ci.yml?query=branch%3Amain)

Current production boundary:

- Read-only discovery is implemented for schema-recognized cameras; physical
  coverage and required USB modes vary by model.
- X-T5 backup export and inspection are physically verified on PTP firmware
  `4.31`. Restore is authorized only for that exact firmware and the required
  preflight conditions.
- X-T5 simulation access and RAW conversion remain disabled until their wire
  contracts are verified on physical hardware.

> Camera writes are intentionally fail-closed. The current production policy
> authorizes backup restore only for a Fujifilm X-T5 with exact PTP firmware
> `4.31` and the required preflight conditions. See the
> [camera support matrix](docs/users/support.md) before using a state-changing
> command. A green CI run is local/fixture evidence, not physical-camera proof.

## Quick Start

There are no published binary releases yet. First verify the CLI without
hardware. With Nix installed:

```sh
nix run github:po4yka/fuji-cli -- --help
nix run github:po4yka/fuji-cli -- device list
```

Or install the [source prerequisites](docs/users/installation.md#source-build-without-nix)
(the pinned Rust toolchain, CUE 0.16.1, a C toolchain, and libusb headers), then
build:

```sh
git clone https://github.com/po4yka/fuji-cli.git
cd fuji-cli
cargo build --locked --release
./target/release/fujicli --help
./target/release/fujicli device list
```

With a camera connected in its documented USB mode, read live information:

```sh
./target/release/fujicli device info
```

`device list` prints each supported connected camera. With no supported camera,
it prints `No supported cameras connected` and exits successfully. `device
info` opens a live PTP session and requires a supported camera in the documented
USB mode.

Linux USB permissions and Windows driver setup require additional care. Follow
the [first safe session](docs/users/getting-started/first-safe-session.md) and
[device-access troubleshooting](docs/users/how-to/troubleshoot-device-access.md)
instead of running the CLI with elevated privileges.

## Documentation

- [Installation](docs/users/installation.md)
- [First safe camera session](docs/users/getting-started/first-safe-session.md)
- [Task guides and CLI reference](docs/README.md)
- [Camera support and safety boundaries](docs/users/support.md)
- [Contribute](CONTRIBUTING.md)
- [FML schema reference](docs/fml/README.md)
- [Architecture](docs/internals/README.md)

Packaged distributions include Bash, Zsh, Fish, and PowerShell completions plus
section 1 man pages. Source checkouts keep the generated assets under
`assets/share/`.

## Development

`build.rs` exports the camera model from `fml/` with CUE and generates Rust into
Cargo's build output. Use `nix develop` for the pinned toolchain and native USB
dependencies, then follow the [local CI guide](docs/contributors/ci.md).

Reverse-engineering and dangerous probe commands are isolated in the
non-distributable `fujicli-dev` crate behind explicit feature gates. They are
not part of the production `fujicli` command surface.

## Getting Help and Security

Use [SUPPORT.md](SUPPORT.md) for installation, compatibility, and bug-report
routing. Report suspected vulnerabilities through the private path in
[SECURITY.md](SECURITY.md), not a public issue.

## License

[MIT](LICENSE)
