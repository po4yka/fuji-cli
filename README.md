# fujicli

`fujicli` is an experimental, schema-driven command-line tool for inspecting
and managing Fujifilm cameras over USB/PTP. It provides device discovery,
camera-native backup artifacts, film-simulation profiles, and in-camera RAW
conversion workflows.

> Camera writes are intentionally fail-closed. The current production policy
> authorizes backup restore only for a Fujifilm X-T5 with exact PTP firmware
> `4.31` and the required preflight conditions. X-T5 simulation access and RAW
> conversion remain disabled until their wire contracts are verified on
> physical hardware. See the [camera support matrix](docs/users/support.md)
> before using a state-changing command.

## Quick Start

There are no published binary releases yet. With Nix installed:

```sh
nix run github:po4yka/fuji-cli -- device list
```

Or install the [source prerequisites](docs/users/installation.md#from-source-non-nix)
(the pinned Rust toolchain, CUE 0.16.1, a C toolchain, and libusb headers), then
build:

```sh
git clone https://github.com/po4yka/fuji-cli.git
cd fuji-cli
cargo build --locked --release
./target/release/fujicli device list
```

Start with the read-only discovery commands:

```sh
fujicli device list
fujicli device info
```

Linux USB permissions and Windows driver setup require additional care. Follow
the [installation guide](docs/users/installation.md) instead of running the CLI
with elevated privileges.

## Documentation

- [Installation](docs/users/installation.md)
- [Usage and process contract](docs/users/usage.md)
- [Camera support and safety boundaries](docs/users/support.md)
- [Contributor guide](docs/contributors/README.md)
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

## License

[MIT](LICENSE)
