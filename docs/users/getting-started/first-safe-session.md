# First Safe Session

This session verifies the CLI and discovers a camera without performing a
state-changing operation. Complete the [installation](../installation.md)
first.

## 1. Verify the Executable

Run help before connecting hardware:

```sh
fujicli --help
```

Success prints the top-level command families and options. This check does not
access USB or require a camera.

## 2. Prepare the Host and Camera

Connect the camera directly with a data-capable cable and select its PTP or USB
mode. Apply only the access setup for your host:

- [Linux USB access](../how-to/linux-usb-access.md)
- [macOS camera access](../how-to/macos-camera-access.md)
- [Windows driver setup](../how-to/windows-driver.md)

Do not start with `sudo`. Raw USB access can issue state-changing PTP commands,
and elevated execution bypasses the narrow host policy described in the Linux
guide.

## 3. Discover the USB Device

```sh
fujicli device list
```

`device list` reads USB descriptors only. It does not claim the camera
interface or open a PTP session. A successful match identifies a supported USB
device; it does not prove that later commands are authorized for its firmware.

If no camera is listed, or the command reports access or ownership trouble,
follow [device-access troubleshooting](../how-to/troubleshoot-device-access.md).

## 4. Read Live Camera Information

Close photo importers and other PTP clients, then run:

```sh
fujicli device info
```

This command opens a live PTP session and reports the selected camera's model,
serial, battery, and USB mode. Keep the raw serial private; use the serial
fingerprint printed by the CLI when a later command requires target binding.

If multiple supported cameras are connected, select one on a leaf command with
`--device BUS.ADDRESS`. See the [CLI reference](../reference/cli.md).

## 5. Stop Before a Write

Check the [camera support matrix](../support.md) before backup restore,
simulation access, or RAW conversion. Command help and a successful discovery
run are not physical evidence that a state-changing operation is authorized.

Read the [fail-closed safety model](../explanation/fail-closed-safety-model.md)
before the first mutation. If a command ever reports unknown camera state, do
not retry it automatically; investigate the physical camera first.
