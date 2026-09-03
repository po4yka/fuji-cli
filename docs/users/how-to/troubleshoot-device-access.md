# Troubleshoot Device Access

First identify which boundary fails. Do not use `sudo fujicli` as a workaround,
and do not retry a state-changing command when the camera state is unknown.

## The Camera Is Not Listed

Run:

```sh
fujicli device list
```

This reads USB descriptors without claiming the interface or opening a PTP
session. If no supported device appears, check the cable, power, and camera USB
mode, then follow the host setup:

- [Linux USB access](linux-usb-access.md)
- [macOS camera access](macos-camera-access.md)
- [Windows driver setup](windows-driver.md)

A schema entry, nearby model, or visible USB device does not prove that a live
command is supported. Check the [camera support matrix](../support.md).

## Discovery Works but a Live Command Fails

`device info` claims the interface and opens a PTP session. A permission or
`LIBUSB_ERROR_ACCESS` result points to host access policy. A busy or
`LIBUSB_ERROR_BUSY` result normally means another PTP client owns the interface.
`fujicli` never detaches a kernel driver from the interface: the still-image
class normally has none bound, and a busy interface on Linux means another
userspace PTP client, which detaching would not resolve.

Close gphoto2, digiKam, Shotwell, Darktable, photo importers, and other camera
applications. Do not run simultaneous PTP clients during backup restore,
simulation changes, or rendering.

On Linux, GVFS may open the camera automatically. Eject or unmount it in the
file manager, or inspect mounts:

```sh
gio mount --list
```

Use the path reported by `lsusb` to find another owner:

```sh
fuser -v /dev/bus/usb/BBB/DDD
lsof /dev/bus/usb/BBB/DDD
```

On macOS, stop Image Capture's `ptpcamerad` as described in the
[macOS guide](macos-camera-access.md).

## A State-changing Command Was Interrupted

Losing ownership, timing out, or disconnecting during backup restore,
simulation access, or rendering can leave camera state unknown. Do not replay
the operation automatically.

Exit status `3` is the process-level unknown-state signal. Inspect the physical
camera and follow the [exit-code reference](../reference/exit-codes.md) before
deciding what to do next.

## Share Only Reviewed Diagnostics

`-vvv` output can contain device and host context even when payloads are
omitted. Before posting publicly, remove serials, backup artifacts, RAF/JPEG
files, custom-setting names, full paths, secrets, and unnecessary logs.
