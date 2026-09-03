# X-T5 firmware 4.31 static analysis (2026-09-03)

A static analysis of the public FUJIFILM X-T5 firmware update file
`FWUP0030.DAT`, version 4.31 (104 MiB, SHA-256
`790d6deecf26766188b0e506e40645e6b816addfeba39a52334349405421724e`).
Nothing here was sent to a camera. Every fact below is *vendor evidence from the
firmware image*, not physical-device proof; per the project rules it must be
confirmed on a camera before `docs/users/support.md` changes. Where a finding
coincides with the physical run in
[x-t5-device-audit-2026-08-31](x-t5-device-audit-2026-08-31.md) this page says
so explicitly.

## Why this matters for the project

- The firmware carries the camera's *static PTP surface*: the operation, event
  and object-format lists that `GetDeviceInfo` returns, the property lists per
  USB mode, and a per-property descriptor table with PTP datatype, access, form
  (enum/range) and default. That is exactly the information `fml/` declares by
  hand today and which the camera refuses to serve through
  `GetDevicePropDesc` in tether mode.
- The container format and compression are now decoded, so the same procedure
  applies to any X-Processor 5 body that ships the same container (X-H2S
  `FWUP0027.DAT` is referenced inside this image) and can be repeated for
  future 4.xx releases to diff the PTP surface.

## Container format

Public prior art (FujiHack wiki and `fujihack/patcher`) documents the header
and the bit-flip; the block layout, the section header and the compression were
not documented anywhere and were derived here.

| Offset | Content |
|---|---|
| `0x000` | `u32 LE = 6`: header type. Type 6 means a 512-byte model-code field. |
| `0x004..0x204` | 512 ASCII hex digits. Decoded they read as 8-digit body codes `00056881 00056882 00056883 00056891 ... 00055973`, zero padded. |
| `0x204` | `u32 = 4`, `0x208` `u32 = 0x31`: shown as version `4.31`. |
| `0x20C..0x214` | `0x33ea0222`, `0xff7cbb29`: checksum-like words. The FujiHack byte-sum rule did not reproduce them for this type-6 file; treat as unverified. |
| `0x214..0x274` | `0xffffffff`, `1`, then offsets into the image tail (`0x068400c0`, `0x0684c0c0`+`0x190`, `0x0684c250`+`0x318`), test patterns `55555555 aaaaaaaa`. |
| `0x274..end` | Flash image in `0x20000`-byte erase blocks. Every payload byte is bit-inverted (`XOR 0xFF`). `0x00` and `0xFF` fill runs sit between sections unchanged. |
| tail | A table of `(offset, length)` `u32` pairs followed by a 32-byte value that looks like a hash or signature. Not analysed further. |

After inverting the payload the image contains, in order:

| NOT-image offset | Content |
|---|---|
| `0x274..0x80274` | Plain AArch64 boot/updater code. ThreadX banner `Copyright (c) 1996-2019 Express Logic Inc. * ThreadX Cortex-A5x-SMP/ARM Version G5.9.5.5`. |
| `0xc037a` | Model block: `X-T5`, `FUJIDSCFBLOG_DSF.R98.X-T5`, `FUJIFILM`, body code `56881`. |
| `0x300274`, `0x820274`, `0xf20274`, `0x24a0274` | Four compressed sections (the camera firmware proper, see below). |
| `0x2a30000..0x3800000` | Plain UTF-16LE menu strings for all UI languages. |
| `0x3c40274` | Flattened device tree, model `Socionext MARBLE Development Board`. |
| `0x3cb0274` | ARM64 Linux `Image` (`Linux version 4.9.92 (oe-user@oe-host) (gcc version 7.3.0)`), ELF64 AArch64 objects and a cpio/xz initramfs follow. |
| `0x4300000..0x5600000` | High-entropy blob (8.0 bits/byte), not identified; possibly a signed or encrypted payload for a coprocessor. |

## Section compression

Each compressed section starts with a 24-byte header of six `u32 LE`:

```text
uncompressed_size, stored_size (header included), page_count, 0x4000, 4, 3
```

`page_count * 0x4000 >= stored_size`. `0x4000` matches the 16 KiB DMA page of
the hardware decompressor described by prior art; pages are *not* independently
compressed, the stream is continuous. The stream is a byte-oriented LZSS with a
2 KiB window:

```text
byte < 0x80  : literal run; the next `byte` bytes are copied verbatim
byte >= 0x80 : match token of two bytes b0 b1
               length   = b1 & 0x0F                      (1..15)
               distance = ((b0 & 0x7F) << 4) | (b1 >> 4)  (0..2047)
               distance 0 emits `length` zero bytes (window starts zeroed)
```

Validation: the parse of all four sections ends exactly on `stored_size`, the
sum of literal bytes plus match lengths equals `uncompressed_size` (minus the
page padding), and the output is valid AArch64 (24k `ret` instructions in the
first section) with intact strings. The four sections decompress to 11.7, 15.4,
14.5 and 7.3 MiB. Code in the decompressed buffers is misaligned by one byte
(sections 1 and 2) or two bytes (section 3) relative to the buffer start; align
before disassembling.

Reference decoder (Python, standard library only):

```python
def lzss(data: bytes) -> bytes:
    out = bytearray(); i = 0
    while i < len(data):
        c = data[i]
        if c < 0x80:
            out += data[i + 1:i + 1 + c]; i += 1 + c
            continue
        b1 = data[i + 1]; i += 2
        length = b1 & 0xF
        dist = ((c & 0x7F) << 4) | (b1 >> 4)
        if dist == 0:
            out += bytes(length); continue
        p = len(out) - dist
        for k in range(length):
            out.append(out[p + k] if p + k >= 0 else 0)
    return bytes(out)

img = bytes(b ^ 0xFF for b in open("FWUP0030.DAT", "rb").read())
# section header at `off`: struct.unpack_from("<6I", img, off)
# payload = img[off + 0x18 : off + stored_size]
```

## PTP surface found in the firmware

All offsets are into the *decompressed* second section (`0x820274`).

### `GetDeviceInfo` static lists (`0xa002da`)

The vendor extension string `fujifilm.co.jp: 1.0;` is followed by
zero-terminated `u16` arrays:

| List | Codes |
|---|---|
| Properties, card-reader variant | `5001 d303 d406 d407` |
| Events | `4002 4003 4004 4005 4006 4008 4009` |
| Object formats | `3001 3008 300d b982 3800 3801 3812 380d b802 c802` |
| Capture formats | `3801 3808 3812` |
| Operations, variant A | `1001-100b 100f 1014 1015 1016 101b 900c 900d 901d 9801 9802 9803 9805` |
| Operations, variant B | `1001-100d 100f 1014 1015 1016 101b 900c 900d 901d 9801 9802 9803 9805` |

Cross-check: variant B and the four-property list are byte-for-byte what the
physical camera returned in USB CARD READER mode in the device audit. The mode
`0x6` operation list from the audit (`1001-100d 1014-1017 900c 900d 901d`) is
not among the static lists, so that DeviceInfo is assembled at run time.

Vendor operations `0x9801-0x9805` are advertised by the firmware and the
camera but not used by this project yet; their semantics are unknown.

### Property lists

Two `0xffff`-terminated `u16` lists, each present twice:

- `0x90817d` (copy at `0x908415`): **61 codes**. Identical to the 61 properties
  the camera advertised in USB mode `0x6` during the device audit:
  `5005 5015 d001 d007 d008 d00a d00b d00c d017 d018 d01c d023 d029 d02e d030 d031 d032 d104 d16e d17b d183-d187 d189 d18c-d1a5 d208 d20b d212 d21c d320 d321 d34d d36a d36b`.
- `0x907f65` (copy at `0x9081fd`): **264 codes**, the full property universe of
  this firmware (listed at the end of this page).

### Property descriptor table (`0x90d715`, 264 entries of 76 bytes)

Entry layout, one per code of the 264-code list in the same order:

```text
u16 code, u16 0, u32 datatype, u32 access, u32 form, u32 flags,
char[52] default, u32 index
```

- `datatype` uses PTP codes: `0x0004` UINT16 (169 entries), `0xFFFF` STR (41),
  `0x0003` INT16 (27), `0x0006` UINT32 (18), `0x0000` undefined (7), `0x0005`
  INT32 (2).
- `access`: `1` get/set (211), `0` get only (43), `2` (10, not interpreted).
- `form`: `2` enumeration (166), `1` range (60), `0` none (38).
- `default` is an ASCII string. For range properties it is
  `default/min/max/step`, for example `0/-180/180/10`. `**` marks values the
  firmware fills at run time.

A second table at `0x908495` (264 entries of 80 bytes) holds nineteen `u32`
flags per property with values `0x80000800`, `0x80001000` or `0`; it looks like
per-connection-mode visibility but is not interpreted here.

Enumeration value lists for `form = 2` properties are not in these tables; they
are produced at run time. This matches the audit observation that the camera
refuses `GetDevicePropDesc` in mode `0x6`.

### The 61 tether-mode properties with their firmware descriptors

Names come from the device audit (`libgphoto2` / `libfuji` / this project).
"no static descriptor" means the code is in the 61-code list but not in the
264-code table, so the firmware has no datatype/default row for it. FML
lists whether `fml/option.cue` or `fml/camera.cue` declares the code today.

| Code | Name (prior art) | Type | Access | Form | Default / range | FML |
|---|---|---|---|---|---|---|
| `5005` | WhiteBalance | UINT16 | get/set | enum | `0x0002` | no |
| `5015` | Sharpness | INT16 | get/set | range | `0/-40/40/10` | no |
| `D001` | FilmSimulation | UINT16 | get/set | enum | `0x0001` | no |
| `D007` | DRangeMode | UINT16 | get/set | enum | `100` | no |
| `D008` | ColorMode | INT16 | get/set | range | `0/-40/40/10` | no |
| `D00A` | ColorSpace | UINT16 | get/set | enum | `0x0001` | no |
| `D00B` | WhitebalanceTune1 | INT16 | get/set | range | `0/-9/9/1` | no |
| `D00C` | WhitebalanceTune2 | INT16 | get/set | range | `0/-9/9/1` | no |
| `D017` | ColorTemperature | UINT16 | get/set | enum | `10000` | no |
| `D018` | Quality | UINT16 | get/set | enum | `0x0002` | no |
| `D01C` | NoiseReduction | UINT16 | get/set | enum | `0x2000` | no |
| `D023` | GrainEffect | UINT16 | get/set | enum | `0x0001` | no |
| `D029` | Shadowing | UINT16 | get/set | enum | `0x0001` | no |
| `D02E` | WideDynamicRange | UINT16 | get/set | enum | `0x0000` | no |
| `D030` | unnamed | UINT16 | get/set | enum | `0x0001` | no |
| `D031` | unnamed | INT16 | get/set | range | `0/-180/180/10` | no |
| `D032` | unnamed | INT16 | get/set | range | `0/-50/50/10` | no |
| `D104` | BlackImageTone | INT16 | get/set | range | `0/-180/180/10` | no |
| `D16E` | USBMode | UINT16 | get | range | `0x01` | yes |
| `D17B` | unnamed | UINT16 | 2 | enum | `0x0001` | no |
| `D183` | StartRawConversion | no static descriptor | | | | yes |
| `D184` | IOPCode | no static descriptor | | | | no |
| `D185` | RawConvProfile | no static descriptor | | | | yes |
| `D186` | TetherRawConditionCode | no static descriptor | | | | no |
| `D187` | TetherRawCompatibilityCode | no static descriptor | | | | no |
| `D189` | unnamed | UINT16 | get/set | enum | `0x0001` | no |
| `D18C` | custom setting slot | UINT16 | get/set | enum | `0x0001` | yes |
| `D18D` | simulation setting | STR | get/set | none | `0x0000` | yes |
| `D18E-D1A4` | simulation settings (23 codes) | no static descriptor | | | | yes |
| `D1A5` | unnamed (equals slot count) | no static descriptor | | | | no |
| `D208` | unnamed | UINT16 | get/set | enum | `0x0304` | no |
| `D20B` | DeviceName | STR | get/set | none | empty | no |
| `D212` | CurrentState / EventsList | undefined | get | none | `-` | no |
| `D21C` | UnknownD21C | UINT16 | get/set | enum | `0x0000` | no |
| `D320` | HighLightTone | INT16 | get/set | range | `0/-20/40/5` | no |
| `D321` | ShadowTone | INT16 | get/set | range | `0/-20/40/5` | no |
| `D34D` | LMOMode | UINT16 | get/set | enum | `0x0002` | no |
| `D36A` | BatteryInfo1 | UINT32 | get | range | `0/0/0xFFFFFFFF/1` | no |
| `D36B` | BatteryInfo2 | STR | get | none | empty | yes |

Observed defaults differ from the camera's live values in the audit (for
example `D007` default `100` versus observed `200`), so the `default` column is
the factory default, not the current setting.

### Debug identifier tables

The first section (`0x300274`) contains about 35,000 zero-terminated C
identifiers: `UI_SETP_*` (12k), `ICO_*`, `MSG_VALS_*` (4k), `BACKUP_*`/`BKUP_*`,
`MODE_*`, `TSK_*`, `KEY_*`. They enumerate menu values by name, for example
`UI_SETP_FILM_SIM_ACROS`, `MSG_VALS_ISOBASE_12800`,
`MSG_VALS_PCCONNECTIONMODE_WEBCAM`, `FS_NOSTALGIC_NEG`,
`ETERNA_BLEACHBYPASS`, `UI_WEBCAM_UVC_USB_ON`. They are useful for naming
enumeration values once a property's value list is captured from a camera, and
for spotting features (webcam UVC, Frame.io, FTP, Bluetooth remote) before
probing.

## Consequences and candidate work

1. Treat the 61-code list plus the descriptor table as the reference for
   `fml/` declarations on X-T5 4.31: datatypes and range bounds for
   `5015 D008 D00B D00C D031 D032 D104 D320 D321` come straight from the
   firmware and can be pinned per camera without probing writes.
2. The 29 codes without a static descriptor (`D183-D187`, `D18E-D1A5`) are the
   raw-conversion and simulation-slot properties; the firmware handles them on a
   separate path, which is consistent with `GetDevicePropDesc` failing for the
   whole list in mode `0x6`.
3. `0x9801-0x9805` and the 264-code universe are candidates for read-only
   discovery in `fujicli-dev`, one narrow command per operation, per
   [reversing](../contributors/reversing.md).
4. Re-running this analysis on another `FWUP*.DAT` (X-H2S, X-H2, X-S20, or a
   later X-T5 release) and diffing the lists is a cheap way to detect PTP
   surface changes before a device is available.

## Public prior art consulted

- FujiHack wiki `firmware` page and `fujihack/patcher`: header struct and the
  bit-flip, no decompression.
- `fujihack/fujihack` issue #2 and `danielc.dev` "2 years of Fujihack": about
  half of the image is compressed; the PTP/USB code sits in the compressed
  part.
- `tiredboffin/fffw` wiki: hardware LZ decompressor working on 16 KiB pages,
  X-Processor 5 is quad Cortex-A53 with 64-bit ThreadX plus a Linux subsystem;
  its decompressor `ffun` is unpublished.
- `kaneda2004/fuji-xe1-firmware-research`: header parser and bit-flip
  extractor for older models.

New in this page: the section header, the LZSS token format, the location of
the DeviceInfo lists, the property lists and the descriptor table.

## Full 264-code property list (firmware order)

```text
5003 5005 5007 500A 500B 500D 500E 500F 5010 5011 5012 5015 501C D001 D007 D008
D00A D00B D00C D017 D018 D01B D01C D020 D022 D023 D024 D025 D026 D027 D028 D029
D02A D02B D02D D02E D02F D030 D031 D032 D033 D034 D035 D036 D038 D039 D040 D100
D104 D106 D10A D10B D112 D136 D145 D153 D154 D155 D156 D157 D158 D159 D15A D15B
D16D D16E D16F D170 D171 D173 D174 D17B D17E D17F D180 D181 D182 D189 D18A D18B
D18C D18D D1B0 D1B1 D1B2 D1B3 D1B4 D1B5 D1B6 D1B8 D1B9 D1BA D1BB D1BC D1BD D1BE
D1BF D1C0 D1C5 D201 D207 D208 D209 D20A D20B D20C D20D D20E D211 D212 D215 D216
D21C D21D D222 D223 D224 D225 D226 D227 D228 D229 D22A D22B D22C D22D D22E D22F
D230 D231 D232 D233 D234 D235 D236 D237 D238 D239 D23A D23B D23C D23E D23F D240
D241 D242 D243 D246 D247 D248 D249 D24A D24B D24C D24D D24E D251 D252 D253 D254
D255 D256 D257 D258 D259 D25A D25B D25C D25D D25E D262 D263 D264 D265 D266 D267
D268 D269 D26A D26B D26C D26D D26E D26F D270 D271 D272 D273 D274 D275 D276 D277
D278 D279 D27A D27B D27C D27D D27E D27F D280 D281 D282 D283 D284 D285 D286 D287
D288 D289 D28A D28B D28C D28D D28E D28F D290 D291 D292 D293 D294 D297 D304 D305
D306 D307 D310 D320 D321 D322 D323 D33F D346 D347 D34A D34B D34D D34E D351 D352
D359 D35E D35F D364 D365 D366 D369 D36A D36B D36D D36E D36F D370 D372 D374 D375
D376 D381 D38A D38B D38C D38D D38E D395
```

## Limitations

- Static analysis only; no camera was touched. Nothing here changes
  `docs/users/support.md`.
- Enumeration value lists, the semantics of `access = 2`, the 80-byte flag
  table, the checksum words and the trailer signature are not decoded.
- Offsets are specific to `FWUP0030.DAT` 4.31 and will move in other releases;
  the search patterns (zero-terminated `u16` lists after
  `fujifilm.co.jp: 1.0;`, `0xffff`-terminated property lists, 76-byte
  descriptor rows) are what to reuse.
