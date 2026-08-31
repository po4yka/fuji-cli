# Linux USB Access

First use the current `libgphoto2` and systemd `udev` rules from your
distribution. If `fujicli device info` works as your normal user, do not install
another rule.

Never grant `MODE="0666"` to Fujifilm vendor ID `04cb`. That makes every
matching raw USB node writable by every local user and process, including
products that `fujicli` does not support.

Raw USB access is not read-only. A process can bypass `fujicli` safeguards and
change properties, restore backups, upload or delete objects, or invoke
vendor-specific commands.

The supported X-T5 USB identity is `04cb:02fc`. If distribution rules do not
grant access, choose one narrowly scoped setup below.

## Desktop Linux

On a systemd desktop, grant the active local seat user an ACL with `uaccess`:

```udev
# /etc/udev/rules.d/69-fujicli-x-t5.rules
SUBSYSTEM=="usb", ENV{DEVTYPE}=="usb_device", ATTR{idVendor}=="04cb", ATTR{idProduct}=="02fc", TAG+="uaccess"
```

Access follows the active seat rather than becoming world-writable or
permanently available to every user. The ACL applies to all active-user
processes, not only `fujicli`; host policy or sandboxes may restrict it further.

The `69-` prefix matters. The tag must exist before systemd's late seat rule
applies the ACL. Put local rules in `/etc/udev/rules.d`; do not edit
distribution-owned files in `/usr/lib/udev/rules.d`.

## Headless Linux

`uaccess` needs an active local seat. For an SSH-only host or system service,
create a dedicated group and grant only that group access:

```sh
sudo groupadd --system fujicli
sudo usermod --append --groups fujicli USER
```

```udev
# /etc/udev/rules.d/69-fujicli-x-t5.rules
SUBSYSTEM=="usb", ENV{DEVTYPE}=="usb_device", ATTR{idVendor}=="04cb", ATTR{idProduct}=="02fc", GROUP="fujicli", MODE="0660"
```

Replace `USER` with the one human or service account that needs the camera. Log
out and back in after changing group membership. For a systemd service, add
only its account or use `SupplementaryGroups=fujicli`.

Group membership is a persistent capability to issue raw X-T5 USB commands.
Do not use a broad group such as `users`, and review membership periodically.

On a host with an active desktop seat, distribution rules may add a separate
`uaccess` ACL. The group rule is a durable base grant, not an exclusive access
policy.

Some distributions provide a managed camera group. It may replace `fujicli`
only when its membership policy matches the intended access. Package names and
default group policies are distribution-specific.

On Debian/Ubuntu, Fedora, Arch Linux, and derivatives, prefer the udev rules
shipped by the distribution's current `libgphoto2` package. Add a local rule
only when that integration does not cover the connected X-T5.

## NixOS

For desktop `uaccess`, use `services.udev.packages` so the file keeps the
required `69-` ordering:

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

Do not put the desktop rule in `services.udev.extraRules`: NixOS emits those as
`99-local.rules`, after systemd's seat ACL processing. A headless `GROUP` and
`MODE` rule does not depend on that ordering and may use `extraRules`.

Declare the dedicated group and membership in NixOS configuration for the
headless variant. `nix run` does not and should not alter host USB permissions.

## Understand the Rule Boundary

`SUBSYSTEM=="usb"`, `DEVTYPE=="usb_device"`, and exact `ATTR` vendor/product
matching limit the rule to the X-T5 device node. `ATTRS` is not used because it
can match an attribute on a parent device.

Do not match USB interface class `ff` for Fujifilm PTP commands. The camera uses
the standard USB imaging class; vendor-specific operation codes exist inside
PTP, not in a vendor-specific USB interface.

An `ID_USB_INTERFACES` match can narrow a rule to one USB mode, but the project
has no verified descriptor for every supported X-T5 mode. Exact device identity
is the reliable boundary for this rule.

Permissions still apply to the whole USB device node. An interface-class match
controls when access is granted; it cannot confine a process to that interface.

## Apply and Verify

Reload the rules and reconnect the camera:

```sh
sudo udevadm control --reload-rules
# Unplug and reconnect the camera.
lsusb -d 04cb:02fc
```

Use the bus and device numbers from `lsusb` in place of `BBB` and `DDD`:

```sh
stat -c '%A %U:%G %n' /dev/bus/usb/BBB/DDD
getfacl /dev/bus/usb/BBB/DDD
udevadm info --query=property --name=/dev/bus/usb/BBB/DDD
fujicli device info
```

The node must not be world-writable. On a desktop, `getfacl` should show an ACL
for the active user. On a headless host, `stat` should show group `fujicli` and
mode `0660`.

Reloading alone does not change an already connected device. Reconnect it
before testing. If access succeeds but the device is busy, continue with
[device-access troubleshooting](troubleshoot-device-access.md).

## Migrate or Revoke an Old Rule

Search local and distribution directories for broad Fujifilm or world-writable
grants:

```sh
grep -R -n -E '04cb|MODE="?0666"?' \
  /etc/udev/rules.d /usr/lib/udev/rules.d
```

Remove the old `MODE="0666"` line or local file before adding the narrow rule.
A narrow rule does not cancel a broad rule that still matches. Do not delete
package-owned rules; update the owning package instead.

Earlier documentation used `/etc/udev/rules.d/70-fujifilm.rules`. If it
contains only the old broad rule, remove the file. Otherwise remove only the
matching `04cb` and `0666` line, preserving unrelated rules.

To revoke this local grant, delete `69-fujicli-x-t5.rules`, reload rules, and
reconnect. For the headless variant, remove each account from the group:

```sh
sudo gpasswd --delete USER fujicli
sudo udevadm control --reload-rules
# Unplug and reconnect the camera, then verify the node again.
```

On NixOS, remove the rule and group membership from system configuration and
rebuild instead of deleting generated-profile files.

End existing login or service sessions after revoking group membership. A
running process retains the supplementary groups with which it started.

Verify the effective mode and ACL afterward. Distribution `libgphoto2` or
systemd PTP rules may independently grant access.
