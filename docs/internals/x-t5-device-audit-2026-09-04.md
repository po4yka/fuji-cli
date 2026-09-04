# X-T5 device run and macOS transport findings (2026-09-04)

A read-only continuation of the [2026-08-31 audit](x-t5-device-audit-2026-08-31.md) against the same physical FUJIFILM X-T5 (PTP firmware 4.31, USB mode 0x6, 100% battery), run on macOS on 2026-09-04 against repository commit `3428956` (debug build). Method: physical-device evidence comes from `fujicli`/`fujicli-dev` read-only commands issued to the camera; host-side evidence comes from static analysis of Fujifilm X RAW Studio's macOS binaries and from `ptpcamerad`'s unified log, with no PTP traffic involved; one section is a read-only experiment design that has not been run yet. No state-changing PTP command was sent to the camera in this session. See [fuji-ptp-ecosystem-research](fuji-ptp-ecosystem-research.md) for the `ptpcamerad`/`PTPCamera` background this run extends, and [reversing](../contributors/reversing.md) for how these findings change the `simulation-namespace` probe design.

Evidence classification, so physical-device proof is never confused with host-side analysis or design:

| Topic | Evidence type |
| --- | --- |
| `fujicli` device and simulation commands | physical-device |
| `fujicli-dev discover surface` | physical-device |
| Surface-artifact privacy caveat | host-side static analysis |
| X RAW Studio's macOS transport | host-side static and log analysis |
| FTLPTP.dylib's dormant debug log | host-side static analysis |
| `ptpcamerad`'s unified-log signal | host-side log analysis |
| ImageCaptureCore pass-through experiment | physical-device |
| Read-only namespace experiment | design, not yet run |

## `fujicli` device and simulation commands

Physical-device evidence. `device list` found the camera at USB 0.1 (`0x04cb:0x02fc`). `device info --json` returned manufacturer FUJIFILM, model X-T5, `deviceVersion` 4.31, mode "Raw Conversion", battery 100. `simulation list` and `simulation get C1` both failed closed with "firmware 4.31 has only an unverified SimulationAccess profile" and exit code 1, before any camera write - the same fail-closed behavior the 2026-08-31 audit's descriptor findings predict, now confirmed against the production CLI rather than `fujicli-dev`.

| Command | Result |
| --- | --- |
| `device list` | camera found at USB 0.1 (`0x04cb:0x02fc`) |
| `device info --json` | FUJIFILM X-T5, deviceVersion 4.31, mode "Raw Conversion", battery 100 |
| `simulation list` | failed closed: "firmware 4.31 has only an unverified SimulationAccess profile", exit code 1 |
| `simulation get C1` | same failure, same exit code, before any camera write |

## `fujicli-dev discover surface`

Physical-device evidence. `discover surface` reported an advertised surface of 20 operations, 0 events, 61 properties, and 7 image formats; descriptors were served for 0/61 properties and values for 58/61 (`0xD17B`, `0xD183`, `0xD185` refused, matching the 2026-08-31 descriptor asymmetry); 28 FML pins were checked with 0 contradictions. Every one of these numbers is identical to the 2026-08-31 audit, so the advertised surface is stable across sessions, not just across battery levels.

| Metric | 2026-09-04 | 2026-08-31 |
| --- | --- | --- |
| Operations advertised | 20 | 20 |
| Events advertised | 0 | 0 |
| Properties advertised | 61 | 61 |
| Image formats advertised | 7 | 7 |
| Descriptors served | 0/61 | 0/61 |
| Values served | 58/61 (`0xD17B`, `0xD183`, `0xD185` refused) | 58/61 (same three refused) |
| FML pins checked | 28 | 28 |
| Contradictions | 0 | 0 |

`discover simulation --print-values` decoded all 24 slot properties (`0xD18D`-`0xD1A4`) against the FML wire types with no gaps; the selected slot (`0xD18C`) was 7. Per the project rule against reproducing slot names or setting values, only the slot number and the fact of a clean decode are recorded here. `discover info --print-values` read `0xD16E` = 6 (USB mode, raw_conversion) and `0xD36B` = `"100,0,0"` (battery info, first field the percentage) - both consistent with the property inventory already published in the 2026-08-31 audit.

## Surface-artifact privacy caveat

Host-side static analysis, not a device finding. The `discover surface` JSON artifact records a SHA-256 digest of each property value instead of the raw bytes. For the 2- and 4-byte scalar-valued properties that digest is invertible by exhaustive search: hashing every possible 2- or 4-byte value and matching against the recorded digest recovers the original value. All 53 scalar values from this run's surface artifact were recovered this way, locally, in seconds. String-valued properties are unaffected - their value space is too large to exhaust.

The claim in `docs/contributors/reversing.md` (near the `discover surface` description) that the artifact "never records payload bytes, so it needs no privacy review before sharing" is therefore true only for string-valued properties; for scalar-valued properties the digest is payload-equivalent. That sentence is corrected as part of this documentation pass. Changing the `discover surface` tool itself - for example salting the digest or dropping it for scalar-typed values - is a separate, out-of-scope follow-up (see below).

## X RAW Studio's macOS transport

Host-side static and log analysis of Fujifilm X RAW Studio 1.28.0 on macOS; no PTP traffic was sent by this repository's tooling for this section. The app's main binary links `IOKit` only. `Contents/Resources/FTLPTP.dylib` links `ImageCaptureCore` and implements an Objective-C `CCameraLayer` conforming to `ICDeviceBrowserDelegate` and `ICCameraDeviceDelegate`, with a `sendPTPCommand:PTPCommand:` method and a `didSendPTPCommand:inData:response:error:contextInfo:` completion callback. The dylib also exports a C API, `FTL_PTP_*`: `OpenSession`, `GetDeviceInfo`, `GetDevicePropDesc_Int`, `GetDevicePropDesc_Str`, `GetDevicePropValue`, `SetDevicePropValue`, `GetObject`, `GetObjectInfo`, `GetObjectHandles`, `DeleteObject`, `VendorExtensionOperation`, and `SetDeviceBusyRetryInfo`.

`ptpcamerad`'s unified log registers the app as a client (`PTPCameraDevice | + <pid>-com.fujifilm.denji.X-RAW-STUDIO`), and `ioreg` shows X RAW Studio holding no USB user client while `ptpcamerad` holds the PTP interface. So X RAW Studio talks to the camera through `ImageCaptureCore` and `ptpcamerad`, never through direct USB.

Consequences for this project's transport work:

- `pkill ptpcamerad` disconnects X RAW Studio, not just this repository's own tooling; the app reconnects only after the daemon relaunches.
- A host-side USB capture of X RAW Studio's traffic is not available on this Apple Silicon host, because the app never opens the USB interface directly. The "capture X RAW Studio's USB traffic first" plan in [reversing](../contributors/reversing.md) needs a different mechanism on macOS; see the "macOS findings" subsection added there.
- The app is signed with a hardened runtime (Developer ID, flags `0x10000`), so in-process hooking of its `ImageCaptureCore` calls would require a re-signed local copy rather than an unsigned injected agent.

## FTLPTP.dylib's dormant debug log

Host-side static analysis. `FTLPTP.dylib` carries a dormant debug logger, `CDebugLogMac`, that writes files with prefix `FTLPTPMacLog` and name pattern `%s%s_%03d.txt`, including hex dumps of its own data. Static analysis shows `LOG_Start` is reachable only from the size-rotation path of an already-open log - there is no call path that opens the log fresh - and the helper that resolves the `FTLPTP.plist` path has no caller at all. Neither can be triggered without code injection, so this log is not a usable evidence source without altering the running process.

## `ptpcamerad`'s unified-log signal

Host-side log analysis, no static binary inspection. `/usr/bin/log stream --predicate 'process == "ptpcamerad"' --debug` records each PTP pass-through as `'sendPTPCommand:andPayload:withReply:': Transaction created` and a matching `released` line, each timestamped, but with no opcode and no payload. Over a 9-minute window with X RAW Studio idle (camera connected, no user interaction), the app polled the camera with two commands every 3 seconds - 40 commands per minute. The log therefore gives timing and counts of PTP activity, never its content, and cannot substitute for a payload capture.

## ImageCaptureCore pass-through experiment

Physical-device evidence: this is the one macOS-transport section backed by real PTP traffic to the camera, sent by this project's own scratch tooling rather than by X RAW Studio. A throwaway Swift tool built on `ICDeviceBrowser`, `requestOpenSession`, and `requestSendPTPCommand` sent `GetDevicePropValue(0xD18C)` and received payload `07 00` with response code `0x2001` (OK) and a transaction ID assigned by `ptpcamerad` itself. The received value (`0x0007`) matches the slot 7 read independently via `fujicli-dev discover surface` in the section above. Throughout the call, `ptpcamerad` kept the PTP interface and X RAW Studio stayed connected - the pass-through coexists with the daemon and with other PTP clients, unlike direct USB access. In the completion handler, the first `Data` argument is the command payload and the second is the response container (status code and parameters).

This establishes `ImageCaptureCore` pass-through as a candidate macOS transport for `fujicli` itself: it needs no daemon-killing, coexists with vendor apps, and is the mechanism those vendor apps use (see the section above). Bulk-size limits for `GetObject`/`SendObject` over this path are unverified - the experiment only exercised a single small `GetDevicePropValue` round trip.

## Design: a read-only namespace experiment

Design only; not yet run against the camera. The unresolved question is whether `0xD18C` addresses the X-T5's still C1-C7 custom-setting slots, its movie C1-C7 slots, or something else (see the `simulation-namespace` probe design in [reversing](../contributors/reversing.md)). A fully read-only experiment can narrow this before any mutating probe:

1. Give still C1-C7 and movie C1-C7 distinguishable names on the camera body.
2. Select a different slot in the still menu and in the movie menu.
3. With the camera's STILL/MOVIE switch in each position, disconnect and reconnect, then read `0xD18C` and `0xD18D`.
4. Compare the readings across both switch positions.

`0xD18D` supplies the observable the mutating probe design currently lacks: after the probe writes `0xD18C = n`, reading `0xD18D` back and comparing it against the operator-declared still and movie slot names for slot `n` identifies which namespace the write landed in. This is a candidate `NamespaceSignal` for `crates/fujicli-dev/src/decision.rs`'s decision table. `discover simulation` does not print `0xD18C` today; printing it is a small, separate follow-up that would make this experiment's step 3 observable through the existing CLI instead of a new probe.

## Follow-ups

- Change the `discover surface` tool so it no longer stores an exhaustively-invertible digest for scalar-valued properties (for example, salt the digest or drop it for scalar types); tracked separately, out of scope for this documentation pass.
- Print `0xD18C` in `discover simulation` output, so the read-only namespace experiment in the design section above can be run through the existing CLI.
- Implement the `NamespaceSignal` read of `0xD18D` in `crates/fujicli-dev/src/decision.rs` and run the read-only namespace experiment above as the maintainer's next step, ahead of any mutating `simulation-namespace` probe run.
- Verify `GetObject`/`SendObject` bulk-size behavior over the `ImageCaptureCore` pass-through path before relying on it for anything beyond small property reads.
- If `ImageCaptureCore` pass-through matures into a supported transport, add it as a maintainer-reviewed opt-in rather than silently replacing the direct-USB path.
