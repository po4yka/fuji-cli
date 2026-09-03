# X-T5 firmware 4.31 USB device audit (2026-08-31)

A read-only audit of a physical FUJIFILM X-T5 (PTP firmware 4.31, serial SHA-256 `1adff71a...9fe77bd`) performed over USB on macOS on 2026-08-31. Method: `fujicli`/`fujicli-dev` transport traces plus ad-hoc pyusb probes issuing only `GetDeviceInfo`, `GetDevicePropDesc`, `GetDevicePropValue`, `GetStorageIDs`, `GetStorageInfo`, `GetObjectHandles`, and `GetObjectInfo`. No state-changing command was ever sent. Property names below come from `libgphoto2` `camlibs/ptp2/ptp.h` and `petabyt/libfuji` `lib/fujiptp.h`; see [fuji-ptp-ecosystem-research](fuji-ptp-ecosystem-research.md) for the source links and cross-validation. Values are from this camera and firmware only; per the project rules they must not be generalized to other models or firmware.

## Identity

`GetDeviceInfo`: manufacturer `FUJIFILM`, model `X-T5`, device version `4.31`, vendor extension `fujifilm.co.jp: 1.0;`, standard version 100, functional mode 0. USB identity `0x04cb:0x02fc`, SuperSpeed+, PTP interface 0, bulk endpoints `0x81`/`0x01`.

## Per-USB-mode surface

The camera's advertised PTP surface differs completely between USB modes (camera menu: SET UP > CONNECTION SETTING > USB MODE).

| | USB RAW CONV./BACKUP RESTORE (`0xD16E` = 6) | USB CARD READER |
|---|---|---|
| Operations | `1001-100b`, `100c`, `100d`, `1014`, `1015`, `1016`, `1017`, `900c`, `900d`, `901d` | `1001-100d`, `100f`, `1014`, `1015`, `1016`, `101b`, `900c`, `900d`, `901d`, `9801-9805` |
| Events | none | none |
| Properties | 61 (all Fuji `0xD0xx`/`0xD1xx`/`0xD2xx`/`0xD3xx` plus `5005`, `5015`) | 4 (`5001`, `d303`, `d406`, `d407`) |
| Storage | two virtual stores, card hidden (below) | the memory card (DCF, ~1 TB in this run) |
| `GetDevicePropDesc` | refused (`GeneralError`) for all 61 properties | works (verified on `0x5001`) |

In Card Reader mode every Fuji vendor property read answers `DevicePropNotSupported (0x200a)`; `device info` detects the missing `0xD16E` advertisement and names the required USB mode instead of surfacing that raw error.

## The GetDevicePropDesc asymmetry

In mode 0x6 the camera advertises `GetDevicePropDesc (0x1014)` but answers `GeneralError (0x2002)` to it for every one of its 61 properties, while `GetDevicePropValue` succeeds for 58 of them. The three exceptions (`D17B`, `D183` = StartRawConversion, `D185` = RawConvProfile) refuse `GetDevicePropValue` too when no RAF is loaded for conversion, at any battery level. `libgphoto2` has no fallback for this shape either; its Fuji path simply never depends on descriptors. Consequence for this runtime: preflight validates read-only required properties by value shape when the camera refuses the descriptor (see the preflight section of [runtime](runtime.md)); requirements declared writable still fail closed, which currently blocks simulation writes on this camera independently of the `0xD18C` namespace question.

## Object stores in mode 0x6

`GetStorageIDs` returns `0x10000001` ("Still") and `0x10000002` ("Live"): storage type 3 (fixed RAM), filesystem `0x0002` (generic hierarchical), access `0x0002` (read-only with deletion), capacity and free space `u64::MAX` (unknown), zero objects at rest. The volume labels embed a serial-derived digit string and are therefore not reproduced here. The memory card is not visible in this mode. `GetObjectInfo` on a nonexistent handle answers `InvalidObjectHandle (0x2009)`.

## Backup object

`GetObjectInfo(0x0)` returns a bare 56-byte `ObjectInfo` - format `0x5000` (FujiBackup), compressed size 38780, all strings empty - with no trailing padding, unlike the 1076-byte padded shape of the project's original capture (export accepts both since `e0654cf`). Two full `backup export` runs at different battery levels produced byte-identical payloads (payload SHA-256 `bd195617...3096e`), so the backup blob is deterministic for unchanged settings. An aborted mid-transfer `GetObject` wedged the camera's bulk pipe; a subsequent `libusb_reset_device` timed out and the camera dropped off the bus until the cable was physically reconnected - do not probe `GetObject` outside the managed transport.

## Session lifecycle quirks

macOS `ptpcamerad` claims the camera on every USB event and leaves a PTP session open when killed. The camera then answers `SessionAlreadyOpen (0x201e)` to `OpenSession`, and it requires the retried `OpenSession` to carry TransactionID 0: a retry that continues the transaction counter answers `ParameterNotSupported (0x2006)`. The runtime recovers automatically since `3706f01` (CloseSession, counter reset, one retry).

## Property inventory (mode 0x6, values from this camera)

Values are little-endian; scalars shown decoded. "unnamed" means the code appears in neither `libgphoto2` nor `libfuji`.

| Code | Name (source) | Observed value | Note |
|---|---|---|---|
| 5005 | WhiteBalance (PTP standard) | 2 | auto |
| 5015 | Sharpness (PTP standard) | 0 | |
| d001 | FilmSimulation (gphoto2) | 1 | PROVIA per `config.c` |
| d007 | DRangeMode (gphoto2) | 200 | DR200% |
| d008 | ColorMode (gphoto2) | 0 | |
| d00a | ColorSpace (gphoto2) | 1 | |
| d00b/d00c | WhitebalanceTune1/2 (gphoto2) | 0 / 0 | |
| d017 | ColorTemperature (gphoto2) | 0 | |
| d018 | Quality (gphoto2) | 4 | |
| d01c | NoiseReduction (gphoto2) | 0x2000 | |
| d023 | GrainEffect (gphoto2) | 6 | Off Large per `config.c` |
| d029 | Shadowing (gphoto2) | 1 | |
| d02e | WideDynamicRange (libfuji) | 0 | |
| d030/d031/d032 | unnamed | 1 / 0 / 0 | |
| d104 | BlackImageTone (gphoto2) | 0 | |
| d16e | USBMode (both; modeled here) | 6 | raw_conversion |
| d17b | unnamed | refuses read | render-adjacent |
| d183 | StartRawConversion (libfuji) | refuses read | |
| d184 | IOPCode (libfuji) | string `F179502,FA179502` | see profile-code discrepancy below |
| d185 | RawConvProfile (libfuji) | refuses read | needs a loaded RAF |
| d186/d187 | TetherRawConditionCode/CompatibilityCode (both) | string `X-T5_0200` | identical values |
| d189 | unnamed | 2 | |
| d18c | custom setting slot (this project) | 7 | C7 selected; unnamed in prior art |
| d18d-d1a4 | simulation settings (this project) | current C7 slot values | all 24 decode against the FML tables with no gaps |
| d1a5 | unnamed | 7 | equals the slot count |
| d208 | unnamed | 0x0304 | |
| d20b | DeviceName (gphoto2) | 3 bytes `01 00 00` | not a PTP string here |
| d212 | CurrentState (gphoto2) / EventsList (libfuji) | 0 | gphoto2 polls this as its event substitute in tether mode |
| d21c | UnknownD21C (libfuji) | 3 at 65% battery, 1 at 100% | charge-state correlated (this audit) |
| d320/d321 | HighLightTone/ShadowTone (gphoto2) | 0 / 0 | |
| d34d | LMOMode (gphoto2) | 2 | |
| d36a | BatteryInfo1 (gphoto2) | 11 at 65%, 12 at 100% | charge-state correlated (this audit) |
| d36b | BatteryInfo2 (gphoto2; modeled here) | string `65,0,0` / `100,0,0` | first field is the percentage |

A full 61-property re-dump at 100% battery was byte-identical to the 65% dump except for `d36b`, `d36a`, and `d21c`, and the descriptor refusals and operation list were unchanged - the surface is stable across charge states.

## Open questions and follow-ups

- Raw-conversion profile constants: the declared `profile_code: "ff179502"` (8 chars) and `header_padding: 0x1ee` in `fml/camera.cue` are arithmetically consistent with each other and with `libfuji`'s construction. This project's `PtpExactString` writes no NUL terminator, so `2 + 1 + 16 + 0x1ee` places the first value at wire offset `0x201`, the same fixed offset `libfuji` reaches with its two-byte terminator and `0x1ec` of padding; the verified 629-byte write length also requires an 8-character code, since a 7-character code would give 627. (An earlier version of this note called the pair inconsistent by counting the terminator once on one side and not the other.) What remains unconfirmed is the code value itself: `libfuji`'s X-T5 captures embed `FF129504` (firmware unrecorded) and the live `D184` on 4.31 reads the 7-character pair `F179502,FA179502`, which is a different property. Resolving this needs a `GetDevicePropValue(0xD185)` capture with a RAF loaded for conversion; see the reconciliation section of [fuji-ptp-ecosystem-research](fuji-ptp-ecosystem-research.md). Codegen now cross-checks the descriptor against the render feature declaration, so the two FML declarations cannot drift apart, but no build check can stand in for the capture.
- The 601-byte (23-field) vs 629-byte (29-field) D185 read/write asymmetry in `libfuji`'s captures is unexplained. The FML declares `n_props` 29 in both directions, with 28 read slots (`tail_0` is write-only, 625 bytes) and 29 write slots (629 bytes).
- Simulation writes stay blocked by two independent gates: the unresolved `0xD18C` still/movie namespace and the impossibility of proving writability without descriptors. A vendor (X Raw Studio / X Acquire) USB capture would resolve both.
- Shooting-settings modeling (ISO, exposure, menus outside the custom slots) is feasible: most codes and value tables can be cross-checked against `libgphoto2`'s 208-entry Fuji property catalog instead of reversed, part of the surface already answers in mode 0x6 (the `d0xx` rows above), the full set lives in USB tether mode (`0xD16E` = 5), and per-camera value pinning can be done entirely read-only by changing a setting on the camera body and re-reading the property. Writes from the CLI would additionally need writability evidence and are out of scope for the audited camera for now.
- Backup restore (`backup import`) passes the descriptor stage since `50cc37c` and all preconditions were met at 100% battery, but a live restore was deliberately not run; its `SendObjectInfo` padding assumption remains verified only against the original capture.
- `0x901d` (`SendObject` in libfuji, apparently equivalent to `0x900d`) is advertised but unexercised by any capture this project follows.
