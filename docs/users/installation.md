# Installation

Prebuilt binaries are not currently published. Build from source using one of
the options below.

## NixOS / Nix

```sh
nix run github:po4yka/fuji-cli
```

Or add the flake to your system inputs and use the `fujicli` package from
`overlays.default`. The package installs Bash, Zsh, Fish, and PowerShell
completion files plus section 1 man pages below its `share/` output.

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
- [CUE](https://cuelang.org/) 0.16.1 on `PATH` - the build script invokes
  `cue export` to materialize the schema into JSON.
- A C toolchain and `libusb-1.0` headers, for the `rusb` dependency.

Then:

```sh
git clone https://github.com/po4yka/fuji-cli.git
cd fuji-cli
cargo build --locked --release
./target/release/fujicli --help
```

Source builds produce only the executable. The generated completion files and
man pages used by the Nix and release packages are available under
`assets/share/`; copy that tree into the matching `share/` directory of your
installation prefix when installing manually.

## Per-Platform Notes

### Linux

Usually no extra setup is required. First use your distribution's current
`libgphoto2` and systemd `udev` rules. If `fujicli device info` works as your
normal user, do not install another rule.

Do not grant `MODE="0666"` to Fujifilm vendor ID `04cb`. Such a rule makes the
raw USB node for every matching Fujifilm product writable by every local user
and process, including products that `fujicli` does not support.

Raw access is not read-only. A process can bypass `fujicli` safeguards and send
PTP operations that change camera properties, restore backups, upload or delete
objects, and invoke vendor-specific commands.

The supported X-T5 USB identity is `04cb:02fc`. If distribution rules do not
grant access, use one of the narrowly scoped options below.

#### Desktop Linux

On a systemd desktop, grant the active local seat user an ACL with `uaccess`:

```udev
# /etc/udev/rules.d/69-fujicli-x-t5.rules
SUBSYSTEM=="usb", ENV{DEVTYPE}=="usb_device", ATTR{idVendor}=="04cb", ATTR{idProduct}=="02fc", TAG+="uaccess"
```

This keeps the normal single-user desktop flow: plug in the camera and run the
CLI without `sudo`. Access follows the active seat instead of becoming
world-writable or permanently available to every user.

The ACL applies to the active user's processes, not only to `fujicli`. Desktop
sandboxes or other host policy may restrict it further.

The `69-` prefix matters. The tag must be present before systemd's late seat
rule applies the ACL. Store local rules under `/etc/udev/rules.d`; do not edit
distribution-owned rules under `/usr/lib/udev/rules.d`.

#### Headless Linux

`uaccess` needs an active local seat and is unsuitable for an SSH-only host or
a system service. Create a dedicated group and grant that group access instead:

```sh
sudo groupadd --system fujicli
sudo usermod --append --groups fujicli USER
```

```udev
# /etc/udev/rules.d/69-fujicli-x-t5.rules
SUBSYSTEM=="usb", ENV{DEVTYPE}=="usb_device", ATTR{idVendor}=="04cb", ATTR{idProduct}=="02fc", GROUP="fujicli", MODE="0660"
```

Replace `USER` with the one human or service account that needs the camera.
Log out and back in after changing group membership. For a systemd service, add
only its service account or use `SupplementaryGroups=fujicli`.

Group membership is a persistent capability to issue raw X-T5 USB commands.
Do not use a broad group such as `users`, and review membership periodically.
On a host that also has an active desktop seat, distribution rules may add a
separate `uaccess` ACL; the group rule is a durable base grant, not an exclusive
access policy.

Some distributions already provide an intentionally managed camera group. It
may replace `fujicli` only when its membership policy matches the access you
want. Package names and default group policies are distribution-specific.

On Debian/Ubuntu, Fedora, Arch Linux, and derivatives, prefer the udev rules
shipped with the distribution's current `libgphoto2` packaging. Use a local
rule only when that integration does not cover the connected X-T5.

#### NixOS

For a desktop `uaccess` rule, use `services.udev.packages` so the file retains
the required `69-` ordering:

```nix
services.udev.packages = [
  (pkgs.writeTextFile {
    name = "fujicli-udev";
    destination = "/lib/udev/rules.d/69-fujicli-x-t5.rules";
    text = ''
      SUBSYSTEM=="usb", ENV{DEVTYPE}=="usb_device", ATTR{idVendor}=="04cb", ATTR{idProduct}=="02fc", TAG+="uaccess"
    '';
  })
];
```

Do not put the desktop rule in `services.udev.extraRules`: NixOS emits those
rules as `99-local.rules`, after systemd's seat ACL processing. A headless
`GROUP`/`MODE` rule does not depend on that ordering and may use `extraRules`.

Declare the dedicated group and account membership in NixOS configuration when
using the headless variant. `nix run` does not and should not alter host USB
permissions automatically.

#### Rule scope and interface class

`SUBSYSTEM=="usb"`, `DEVTYPE=="usb_device"`, and exact `ATTR` vendor/product
matching limit the rule to the X-T5 device node. `ATTRS` is not used because it
can match an attribute on a parent device.

Do not match USB interface class `ff` for Fujifilm-specific PTP commands. The
camera uses the standard USB imaging interface class; vendor-specific operation
codes exist inside PTP and are not a vendor-specific USB interface.

An `ID_USB_INTERFACES` match can narrow a rule to a particular camera USB mode,
but the repository has no verified descriptor for every supported X-T5 mode.
Exact device identity is the reliable boundary for the documented rule.

Permissions still apply to the whole USB device node, not one interface. An
interface-class condition controls when access is granted; it cannot confine an
authorized process to that interface.

#### Apply and verify

After adding, replacing, or removing a rule, reload it and reconnect the camera:

```sh
sudo udevadm control --reload-rules
# Unplug and reconnect the camera.
lsusb -d 04cb:02fc
```

Use the bus and device numbers shown by `lsusb` in place of `BBB` and `DDD`:

```sh
stat -c '%A %U:%G %n' /dev/bus/usb/BBB/DDD
getfacl /dev/bus/usb/BBB/DDD
udevadm info --query=property --name=/dev/bus/usb/BBB/DDD
fujicli device info
```

The node must not be world-writable. On a desktop, `getfacl` should show an ACL
for the active user. On a headless host, `stat` should show group `fujicli` and
mode `0660`.

Reloading rules alone does not change permissions on an already connected
device. Reconnect it before testing.

#### Camera already in use

USB permissions do not coordinate multiple camera clients. Close gphoto2,
digiKam, Shotwell, Darktable, photo importers, and other PTP applications before
running a command that changes camera state.

GVFS may open a newly connected camera automatically. Eject or unmount the
camera in the file manager, or inspect mounts with `gio mount --list`, before
retrying `fujicli`.

Use the device path from `lsusb` to look for another owner:

```sh
fuser -v /dev/bus/usb/BBB/DDD
lsof /dev/bus/usb/BBB/DDD
```

A permission or `LIBUSB_ERROR_ACCESS` failure points to the rule or ACL. A busy
or `LIBUSB_ERROR_BUSY` failure usually means another camera service has claimed
the interface. Do not work around either error with `sudo fujicli`.

Do not run simultaneous PTP clients during backup restore, simulation changes,
or rendering. Losing ownership or disconnecting mid-transaction can leave the
camera state unknown.

#### Migrate or revoke an old rule

Search local and distribution rule directories for broad Fujifilm or
world-writable grants:

```sh
grep -R -n -E '04cb|MODE="?0666"?' \
  /etc/udev/rules.d /usr/lib/udev/rules.d
```

Remove the old `MODE="0666"` line or its local file before adding the new rule.
Adding a narrow rule does not cancel a broad rule that still matches. Do not
delete package-owned rules; update the owning package instead.

Earlier documentation used `/etc/udev/rules.d/70-fujifilm.rules`. If that file
contains only the old broad rule, remove the file. Otherwise, edit out only the
matching `04cb`/`0666` line and preserve its unrelated rules.

To remove the grant introduced by this local rule, delete
`69-fujicli-x-t5.rules`, reload rules, and reconnect the camera. For the
headless variant, also remove each account from the `fujicli` group.

```sh
sudo gpasswd --delete USER fujicli
sudo udevadm control --reload-rules
# Unplug and reconnect the camera, then verify the node again.
```

On NixOS, remove the rule and group membership from system configuration and
rebuild instead of deleting files from the generated system profile.

End existing login or service sessions after revoking group membership. A
running process keeps the supplementary groups with which it started. Verify
the effective mode and ACL afterward: distribution `libgphoto2` or systemd PTP
rules may independently grant access.

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
