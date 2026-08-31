# Installation

Prebuilt binaries are not currently published. Choose a method below, then
follow the [first safe session](getting-started/first-safe-session.md) before a
state-changing workflow.

| Method | Platforms | Prerequisites | Binary | Man pages and completions | Upgrade | Uninstall |
| --- | --- | --- | --- | --- | --- | --- |
| Nix one-off | Flake-supported Linux and macOS | Nix with flakes | Runs from the Nix store with `nix run github:po4yka/fuji-cli` | Included in the package under `share/` | Run the desired refreshed flake revision | No profile install; remove unreachable store paths with normal Nix garbage collection |
| Nix package or overlay | Flake-supported Linux and macOS | A Nix system or profile consuming the flake | Managed by the consuming Nix configuration | Included in the package under `share/` | Update the pinned flake input and rebuild | Remove the package from the consuming configuration or profile and rebuild |
| Source build | Linux, macOS, and Windows | Git, rustup, CUE 0.16.1, a C toolchain, and `libusb-1.0` headers | `target/release/fujicli`; copy it to a directory on `PATH` if desired | Not installed automatically; use the runtime completion command and `assets/share/` | Update the checkout deliberately, then rebuild with `--locked` | Remove the copied binary and any completion or man files you installed |

## Nix Development Build

The development shell supplies the pinned Rust toolchain, CUE, and native
dependencies:

```sh
git clone https://github.com/po4yka/fuji-cli.git
cd fuji-cli
nix develop
cargo build --locked --release
```

The flake also exposes the `fujicli` package through `overlays.default` for a
system configuration that consumes this repository as an input.

## Source Build Without Nix

Install [rustup](https://rustup.rs/), [CUE 0.16.1](https://cuelang.org/), a C
toolchain, and the development headers for `libusb-1.0`. The repository selects
its pinned Rust toolchain.

`build.rs` invokes `cue export`, so `cue` must be on `PATH` during a source
build.

```sh
git clone https://github.com/po4yka/fuji-cli.git
cd fuji-cli
cargo build --locked --release
./target/release/fujicli --help
```

Source builds produce only the executable. Generated section 1 man pages remain
under `assets/share/man/man1/`; copy them into the matching `share/` directory
of the installation prefix when installing manually.

## Shell Completions

Generate a completion script from the installed executable:

```sh
fujicli completion bash > /path/to/bash-completion/completions/fujicli
fujicli completion zsh > /path/to/zsh/site-functions/_fujicli
fujicli completion fish > /path/to/fish/vendor_completions.d/fujicli.fish
fujicli completion powershell > /path/to/Completions/fujicli.ps1
```

The command supports `bash`, `zsh`, `fish`, and `powershell`. It writes only the
script to stdout, so choose a path loaded by the relevant shell. Nix packages
already install generated completion files under `share/`.

## Configure Camera Access

Camera access is a separate host step:

- [Linux USB access](how-to/linux-usb-access.md)
- [macOS camera access](how-to/macos-camera-access.md)
- [Windows driver setup](how-to/windows-driver.md)
- [Device-access troubleshooting](how-to/troubleshoot-device-access.md)
