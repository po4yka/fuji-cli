# macOS Camera Access

No driver change is required. Connect the camera, select PTP or USB mode in its
menus, and verify that `fujicli device list` can see it.

macOS starts `ptpcamerad` through Image Capture for every PTP camera. It claims
the interface, so `device info` and other live commands can fail with
`Access denied (insufficient permissions)`.

Stop it immediately before running `fujicli`:

```sh
pkill -x ptpcamerad; fujicli device info
```

Fujifilm's own X RAW Studio talks to the camera through `ptpcamerad` too, not
directly over USB, so killing the daemon disconnects X RAW Studio until it
relaunches. Issue the kill and the `fujicli` command back to back, as the
single line above does: macOS restarts `ptpcamerad` within a fraction of a
second of the kill, and a pause between the two commands loses that race.

`ptpcamerad` runs as your user, and macOS starts it again on the next USB event.
Do not run simultaneous PTP clients during a state-changing command.

A stopped `ptpcamerad` leaves its PTP session open on the camera. When
`OpenSession` returns `SessionAlreadyOpen (0x201e)`, `fujicli` sends
`CloseSession` and retries once, so no camera reconnect is needed.

For other access failures, follow
[device-access troubleshooting](troubleshoot-device-access.md).
