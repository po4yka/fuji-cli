# Installation

Prebuilt binaries are not currently published. Build from source using one of
the options below.

## NixOS / Nix

```sh
nix run github:po4yka/fuji-cli
```

Or add the flake to your system inputs and use the `fujicli` package from
`overlays.default`.

For a dev shell with the pinned Rust toolchain, CUE, and native dependencies:

```sh
git clone https://github.com/po4yka/fuji-cli.git
cd fuji-cli
nix develop
cargo build --locked --release
```

## From Source (non-Nix)

You need:

- [rustup](https://rustup.rs/), which installs the repository's pinned Rust
  toolchain automatically.
- [CUE](https://cuelang.org/) on `PATH` - the build script invokes `cue export`
  to materialize the schema into JSON.
- A C toolchain and `libusb-1.0` headers, for the `rusb` dependency.

Then:

```sh
git clone https://github.com/po4yka/fuji-cli.git
cd fuji-cli
cargo build --locked --release
./target/release/fujicli --help
```

## Per-Platform Notes

### Linux

Usually no extra setup. If you hit permission errors when listing devices, add a
`udev` rule for Fujifilm's vendor ID (`0x04cb`):

```udev
# /etc/udev/rules.d/70-fujifilm.rules
SUBSYSTEM=="usb", ATTRS{idVendor}=="04cb", MODE="0666"
```

Reload with `sudo udevadm control --reload-rules && sudo udevadm trigger`.

### macOS

Usually no driver changes are required. Connect the camera, make sure it is in
PTP / USB mode in its menus, and `fujicli device list` should see it.

### Windows

Windows binds the camera to its default WPD / photo-import driver, which blocks
raw PTP. Replace the driver with WinUSB or libusbK using Zadig:

1. Install Zadig from <https://zadig.akeo.ie/>.
2. Connect the camera in PTP / USB mode.
3. In Zadig: **Options -> List All Devices**.
4. Select the camera (often listed as "USB PTP" or by model name).
5. Pick **WinUSB** (recommended) or **libusbK** as the target driver.
6. Click **Replace Driver**. You can revert from Zadig later.
