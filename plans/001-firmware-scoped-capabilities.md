# Plan 001: Make camera capabilities and wire codecs firmware-scoped

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report; do not improvise. When done, update the status row for this plan in
> `plans/README.md` unless a reviewer explicitly owns the index.
>
> **Drift check (run first)**:
> `git diff --stat cd5d4413602697f7653d32a7ca9160ace7e094a2..HEAD -- fml crates/codegen src docs tests`
> If any in-scope file changed since this plan was written, compare the
> "Current state" facts against the live code before proceeding. A semantic
> mismatch is a STOP condition.

## Status

- **Priority**: P1
- **Effort**: L (multi-day schema, generator, runtime, CLI, and fixture migration)
- **Risk**: HIGH (changes all generated option codecs and both write paths)
- **Depends on**: none
- **Category**: correctness / safety architecture
- **Planned at**: commit `cd5d4413602697f7653d32a7ca9160ace7e094a2`, 2026-08-30
- **Completed**: exact firmware dispatch, full X-T5 enum maps, firmware-aware
  simulation/RAW codecs, structural RAW signature gate, and unknown-firmware
  fail-closed policy implemented on 2026-08-30.

The implementation retains global option encodings as schema defaults and
parser vocabulary, but they are not write authority: every verified mutating
profile must cover every enum it consumes, and runtime codecs use only the
selected exact profile. This is safer and smaller than duplicating the logical
option AST while preserving the plan's no-fallback invariant.

## Why this matters

The current schema represents logical options, PTP property codes, canonical
wire values, simulation property sets, and RAW profile layout primarily at the
global option or camera-model level. Firmware is checked by preflight, but the
selected preflight entry is not connected to the generated simulation or render
codec. Adding an older or newer firmware to the compatibility matrix can
therefore authorize a model-wide codec that contains values or a structure not
verified for that firmware.

`Reala Ace` is the concrete regression case. Fujifilm documents that X-T5
firmware 4.00 added it; `fml/option.cue` nevertheless places the logical variant
and wire value `0x14` in the single global `film_simulation` encoding. The same
global option is used for X-T5 simulation properties and its RAW conversion
profile. The fix must make successful preflight select the exact generated
capability and wire codec used by every subsequent operation.

## Audit findings

### Confirmed current safeguards

- `src/lib/preflight.rs` exact-matches the PTP firmware string and rejects
  unknown or unverified entries. The only X-T5 production entry is 4.31.
- Preflight reads `GetDevicePropDesc` for required properties. The PTP transport
  validates datatype, writability, range, and enumeration before a property
  write.
- These safeguards mean a pre-4.00 X-T5 cannot currently reach normal mutation:
  it has no exact preflight entry. This plan must preserve that fail-closed
  behavior throughout the migration.

### Confirmed architecture gaps

1. `fml/generation.cue:39-116` gives X-Trans V all 23 X-Trans IV simulation
   settings plus `smooth_skin_effect`. It explicitly carries a TODO to limit
   generation option values.
2. `fml/camera.cue:355-435` gives X-T5 those inherited settings and one fixed
   model-level render profile, while firmware 4.31 exists only in separate
   preflight entries.
3. `fml/option.cue:165-199,346-399` combines logical variants with one global
   encoding. `RealaAce = 0x14` is therefore present for every camera and firmware
   that references `film_simulation`.
4. `crates/codegen/src/common/options/enum/mod.rs` emits one `#[repr(...)]`
   logical enum. The first lookup number is always serialized and later numbers
   are only read aliases, so it cannot express firmware-specific canonical
   writes.
5. `crates/codegen/src/common/simulations/camera.rs` emits one simulation struct
   and manager per camera model. Its write loop knows no selected firmware
   profile.
6. `crates/codegen/src/common/renders/camera.rs` emits one fixed field count,
   profile code, padding, field order, and option codec per camera model. The
   render flow uploads the RAF before reading and validating the current profile,
   so a layout mismatch is discovered after the first state-changing exchange.
7. `crates/codegen/src/common/cameras/mod.rs` emits preflight entries containing
   operations and property requirements only; they carry no simulation or render
   capability/codec reference.
8. Dynamic property descriptors are necessary but insufficient. They can stop
   `film_simulation = 0x14` if firmware advertises an enumeration without it, but
   a no-form descriptor does not; and the descriptor of the RAW-profile blob
   cannot validate its internal field semantics.

### X-T5 properties inherited from generation templates

The first 23 entries come from X-Trans IV; X-Trans V appends the last entry.

| Origin | Logical option | PTP property | Current constraint |
| --- | --- | ---: | --- |
| IV | `custom_setting_name` | `0xD18D` | string, max 25 |
| IV | `image_size` | `0xD18E` | 15 global enum values |
| IV | `image_quality` | `0xD18F` | 5 global enum values |
| IV | `film_simulation` | `0xD192` | 20 global enum values including Reala Ace |
| IV | `monochromatic_color_temperature` | `0xD193` | -18..18 step 1 |
| IV | `monochromatic_color_tint` | `0xD194` | -18..18 step 1 |
| IV | `grain_effect` | `0xD195` | 5 global enum values |
| IV | `color_chrome_effect` | `0xD196` | strong/weak/off |
| IV | `color_chrome_fx_blue` | `0xD197` | strong/weak/off |
| IV | `white_balance` | `0xD199` | 15 global enum values |
| IV | `white_balance_shift_red` | `0xD19A` | -9..9 step 1 |
| IV | `white_balance_shift_blue` | `0xD19B` | -9..9 step 1 |
| IV | `white_balance_temperature` | `0xD19C` | 2500..10000 step 10 |
| IV | `highlight_tone` | `0xD19D` | -2.0..4.0 step 0.5 |
| IV | `shadow_tone` | `0xD19E` | -2.0..4.0 step 0.5 |
| IV | `color` | `0xD19F` | -4..4 step 1 |
| IV | `sharpness` | `0xD1A0` | -4..4 step 1 |
| IV | `noise_reduction` | `0xD1A1` | -4..4 step 1 |
| IV | `clarity` | `0xD1A2` | -5..5 step 1 |
| IV | `lens_modulation_optimizer` | `0xD1A3` | on/off |
| IV | `color_space` | `0xD1A4` | sRGB/Adobe RGB |
| IV | `dynamic_range` | `0xD190` | 6 global enum values |
| IV | `dynamic_range_priority` | `0xD191` | 5 global enum values |
| V | `smooth_skin_effect` | `0xD198` | strong/weak/off |

Treat every row as potentially firmware-sensitive until descriptor captures
prove datatype, writability, form, and values stable for an explicit firmware
set. Public release notes only prove the Reala Ace delta among these modeled
options; they are not a complete PTP specification.

## Evidence and X-T5 compatibility table

Authoritative public facts:

- Fujifilm's X-T5 firmware page states that version 4.00 added Reala Ace.
- The same page says 4.21 fixed a problem where USB devices might not work
  correctly, and records other connection/menu changes at 4.30.
- Fujifilm X RAW Studio 1.24.0 added Reala Ace RAW development, confirming that
  the firmware-dependent value affects the RAW-conversion surface as well as the
  camera menu.
- Public notes do not disclose PTP property codes, RAW profile code, field count,
  order, padding, or binary encodings. Those facts require privacy-reviewed
  captures from the exact firmware; do not infer them from generation or model.

| X-T5 firmware band | Publicly documented relevant change | Reala Ace | Repository wire evidence/policy after migration |
| --- | --- | --- | --- |
| 1.00-1.04 | Initial line and bug fixes | No | `documented` logical absence only; all writes denied until captured |
| 2.00-2.10 | XApp support including settings backup/restore introduced at 2.00 | No | backup/simulation/render wire profiles unverified; writes denied |
| 3.01 | XApp/Frame.io/connection feature changes | No | use as the pre-Reala regression fixture; production writes denied |
| 4.00, 4.10, 4.11, 4.20 | Reala Ace added at 4.00; later AF/custom-setting changes | Yes | logical capability documented; exact PTP/render signature unverified; writes denied |
| 4.21 | USB-device connection bug fixed | Yes | separate profile identity; writes denied until exact capture |
| 4.30 | Wireless security and connection-menu changes | Yes | separate profile identity; writes denied until exact capture |
| 4.31 | Minor fixes over 4.30 | Yes | preserve the current exact production allowlist and current captured wire constants |
| Any unlisted/new version | Unknown | Unknown | no profile match; all state-changing operations fail closed |

The table intentionally separates documented feature presence from verified
wire compatibility. Do not use a version range merely because public release
notes do not mention a wire change.

## Target domain model

Separate three layers and make only the last one runtime-authoritative:

1. **Model capabilities**: stable physical identity and feature families only:
   VID/PID, PTP manufacturer/model, chunk size, and whether the body family can
   conceptually expose backup/simulation/render. Do not place option allowlists
   or binary layout here.
2. **Generation defaults**: CUE-only reusable templates for common setting IDs,
   rules, and proven common mapping fragments. They reduce duplication but grant
   no runtime support and are never a fallback selector.
3. **Firmware profiles**: fully materialized effective capabilities selected by
   exact firmware. They own allowed properties/values, datatype and writability,
   per-operation PTP requirements, USB modes, option codecs, and every RAW
   profile structural constant. Only a verified operation inside a matched
   profile can authorize mutation.

## Proposed FML schema

The exact CUE syntax may be adjusted to repository style, but preserve these
semantics. Logical options must no longer own camera wire encoding.

```cue
// option.cue: global logical vocabulary only
options: film_simulation: #Option & {
    spec: {
        name: "Film Simulation"
        kind: "enum"
        variants: [
            // stable logical ids, names, aliases; includes reala_ace
        ]
    }
}

#FirmwareSelector: {
    // Exactly one selector form. Versions are canonical numeric components,
    // never floats or lexicographically compared strings.
    versions?: [string, ...string]
    range?: {
        min_inclusive: string
        max_exclusive: string
    }
}

#WireValue: {
    write: int                 // exactly one canonical outgoing value
    read: [int, ...int]        // accepted values for this firmware only
    write: read[0]             // canonical must also be readable
}

#FirmwareOption: {
    ref: #RefOption
    prop_code?: uint16
    data_type: uint16
    writable: bool
    allowed?: [...string]      // enum ids; must equal keys in values
    values?: [string]: #WireValue
    range?: {min: number, max: number, step: number}
    scale?: number
    string?: {min_length?: uint, max_length?: uint}
}

#FirmwareProfile: {
    selector: #FirmwareSelector
    evidence: {
        status: "verified" | "documented" | "unverified"
        source: [...string]    // capture ids/official document references
    }

    operations: {
        backup_restore?: #OperationCapability
        simulation_read?: #OperationCapability
        simulation_write?: #OperationCapability
        raw_conversion?: #OperationCapability
    }

    capabilities: {
        simulation?: {
            slots: uint
            settings: [...#FirmwareOption]
            transformations?: [...#Transformation]
            rules?: [...#Rule]
        }
        render?: {
            profile_code: uint32
            header_padding: uint32
            fields: [...#FirmwareField] // exact wire order
            options: [string]: #FirmwareOption
            transformations?: [...#Transformation]
            rules?: [...#Rule]
        }
    }
}

#OperationCapability: {
    status: "verified" | "unverified"
    minimum_battery_percent: uint8 & <=100
    allowed_usb_modes: [uint32, ...uint32]
    required_operations: [uint16, ...uint16]
    required_properties: [#RequiredProperty, ...#RequiredProperty]
}
```

Illustrative X-T5 profile composition:

```cue
generations: x_trans_v: spec: _defaults: {
    // Reusable field list/rules and stable fragments only. This is not a
    // runtime profile and must not contain an implicit "all firmware" claim.
    simulation: settings: [...]
}

cameras: x_t5: spec: {
    model: {
        name: "FUJIFILM X-T5"
        generation: "x_trans_v"
        usb: {vendor_id: 0x04cb, product_id: 0x02fc, ...}
        ptp: {manufacturer: "FUJIFILM", model: "X-T5"}
        feature_families: {backup: true, simulation: true, render: true}
    }

    firmware_profiles: {
        fw_3_01_documented: #FirmwareProfile & {
            selector: versions: ["3.01"]
            evidence: {status: "documented", source: ["fujifilm-x-t5-fw-history"]}
            operations: {
                simulation_write: status: "unverified"
                raw_conversion: status: "unverified"
            }
            capabilities: simulation: {
                settings: _generation._defaults.simulation.settings
                // Explicit 19-value map; no reala_ace key.
            }
        }

        fw_4_31_verified: #FirmwareProfile & {
            selector: versions: ["4.31"]
            evidence: {status: "verified", source: ["x-t5-4.31-capture-id"]}
            operations: {/* current verified preflight facts */}
            capabilities: {
                simulation: {/* explicit 20-value map, reala_ace write/read 0x14 */}
                render: {
                    profile_code: 0xff179502
                    header_padding: 0x1ee
                    fields: [/* current exact 29 entries */]
                    options: film_simulation: values: reala_ace: {
                        write: 0x14
                        read: [0x14]
                    }
                }
            }
        }
    }
}
```

Schema invariants enforced by CUE and repeated in Rust semantic analysis:

- selectors are canonical, non-empty, and non-overlapping per camera;
- no implicit/default/nearest profile exists;
- a verified operation must reference a fully materialized capability;
- enum `allowed` ids equal the codec map keys and refer to logical variants;
- write value belongs to read values; wire read sets are unambiguous per profile;
- numeric ranges have valid min/max/positive step and fit the declared datatype;
- every simulation property has an exact datatype and prop code;
- every render profile owns exact count/order/code/padding and per-field codec;
- a generation template cannot itself set `verified` or become selectable;
- a range selector is allowed only with explicit evidence covering the whole
  range; otherwise list exact versions.

## AST and semantic-analysis changes

Update `crates/codegen/src/ast/` so the JSON AST mirrors the target model:

- make `OptionSpec` logical-only; remove `prop_code` and wire encoding from
  global option variants;
- add `FirmwareVersion`, `FirmwareSelector`, `FirmwareProfile`,
  `OperationCapability`, `FirmwareOption`, `WireValue`, and firmware-scoped
  simulation/render AST nodes;
- parse firmware into numeric components (`4.10` sorts after `4.9`); retain the
  canonical source string for diagnostics;
- add semantic passes under `crates/codegen/src/schema/` for selector overlap,
  effective-profile completeness, logical-id resolution, codec injectivity,
  datatype bounds, and operation-to-capability consistency;
- make generation inheritance disappear before AST emission: codegen consumes
  fully materialized firmware profiles and never resolves a runtime fallback.

Add focused AST/semantic tests before changing emitters. Negative fixtures must
cover overlapping selectors, string/float version comparison, missing codecs,
unsupported ids, duplicate wire values, empty read sets, canonical write absent
from reads, verified operation without a capability, and render layout missing
one structural field.

## Codegen changes

### Logical options

Emit logical Rust types without `#[repr]` and without direct `BinRead`/`BinWrite`.
Parsing, display, serde, aliases, and iteration remain global. Replace global
`SimulationSetting` and `ConversionProfileField` implementations with generated
firmware-profile codecs.

For each profile and option, emit a codec with:

- exact property code and datatype;
- `encode_write(logical) -> wire` using that profile's canonical value;
- `decode_read(wire) -> logical` using only that profile's read set;
- `validate_logical(logical)` before any selector or property write;
- range/string checks scoped to the profile.

### Firmware profile registry

Extend the generated camera registry so `SupportedCamera` contains
`firmware_profiles`, each with a parsed selector, operation policies, and static
feature dispatch. Use generated stateless profile ZSTs or function-pointer
vtables implementing firmware-scoped simulation/render traits. Do not make the
runtime match arbitrary profile ids in handwritten code.

### Simulation generation

Emit one effective simulation implementation per firmware capability signature,
not one per model. Deduplicate identical signatures only by an explicit generated
signature id; do not infer compatibility from generation.

Validate an entire `SimulationBase` against the selected profile before writing
the custom-setting selector. Pull/push must use the selected option codecs. A
descriptor failure or unsupported logical value must occur before the first
state-changing PTP command, not midway through rollback-protected writes.

### Render generation

Emit one render profile codec per firmware capability signature. Field count,
profile code, padding, field order, skip flags, transformations, and option
codecs all come from that profile. Do not implement conversion-profile wire I/O
on global logical option types.

Before `send_image`, read and exactly decode the current RAW profile with the
selected codec. Only after the structure matches may the session upload the RAF
and write a modified profile. Keep exact EOF checking. This turns an incorrect
field count/code/padding into a pre-mutation refusal.

### CLI generation

Global CLI parsing may continue to accept all logical values because firmware is
unknown at clap parse time. After preflight, validate the resulting base against
the selected profile and return a targeted diagnostic such as:

`film simulation Reala Ace is unsupported by FUJIFILM X-T5 firmware 3.01; it requires a verified profile that includes this value`

Move simulation-file parsing/validation behind the validated session. Version
the exported JSON envelope and include camera id plus firmware capability
profile id. Break and update the old bare per-model JSON contract rather than
adding a compatibility shim.

## Runtime lookup and API changes

Make `ValidatedCameraSession` retain the selected generated profile:

```rust
pub struct ValidatedCameraSession<'camera, Operation> {
    camera: &'camera mut Camera,
    profile: &'static CameraFirmwareProfile,
    evidence: PreflightEvidence,
    operation: PhantomData<Operation>,
}
```

Preflight order:

1. bind physical USB and exact PTP identity;
2. parse the reported firmware canonically;
3. find exactly one matching firmware profile; zero or multiple matches fail;
4. require the requested operation to be verified in that profile;
5. validate USB mode, battery, serial binding, operations, and properties;
6. read all descriptors and intersect them with static policy without widening
   static capabilities;
7. for RAW conversion, read and exactly decode the current profile before any
   upload/vendor write;
8. authorize transport mutations and return the typed session pinned to the
   selected profile.

Session methods must dispatch through `self.profile`, never through a model-only
`CameraBase::as_simulation_manager`/`as_render_manager`. Remove or change every
model-only mutating trait call site; do not retain a parallel legacy path.

Descriptor policy:

- static firmware profile is the maximum allowed capability;
- dynamic descriptor may narrow it but never widen it;
- exact datatype and writability are mandatory for writes;
- descriptor enum/range must contain the candidate;
- a descriptor with no form still requires static enum/range validation;
- RAW blob descriptors validate only outer datatype/framing; inner structure is
  always validated by the selected render codec.

## Unknown/new firmware policy

- Normal state-changing commands fail closed when there is no exact/range match,
  the match is ambiguous, or the operation is unverified.
- Never choose the newest, nearest, same-major, same-generation, or model default
  profile.
- Do not add an experimental write override to the normal binary. Existing
  `reverse-tools` may expose read-only identity/descriptor/profile capture, with
  explicit output marked unverified; it must not authorize property writes,
  object uploads, vendor render commands, or backup restore.
- Error output lists exact verified versions/profile ids for the requested
  operation and tells maintainers which capture evidence is missing.

## Implementation sequence

### Step 1: Add characterization and regression tests

Add tests proving the current/global behavior and target fail-safe semantics.
Use synthetic firmware fixtures for 3.01 and 4.00 so tests do not claim physical
support. Include the X-S20 global-Reala case as proof that this is architectural,
not X-T5-only.

**Verify**:
`build-gate -- cargo test --locked -p codegen --jobs 4`
→ all tests pass before the refactor except newly added target tests, which must
initially fail for the intended reason.

### Step 2: Split logical options from wire encodings in FML and AST

Introduce the schema above, migrate logical option definitions, add firmware
selector/codec AST types and semantic checks, and keep old emitters compiling
only long enough to complete this step on the branch. Do not commit a mixed
runtime contract as final state.

**Verify**:
`mise exec cue@0.17.1 -- cue export ./fml --out json >/dev/null`
→ exit 0; negative codegen fixtures reject every listed invalid schema.

### Step 3: Generate firmware profile registry and option codecs

Emit structured selectors, operation policies, and per-profile option codecs.
Delete global wire repr/codec implementations and update codegen unit tests.

**Verify**:
`build-gate -- cargo test --locked -p codegen --jobs 4`
→ all codegen tests pass, including pre/post-Reala and multi-wire fixtures.

### Step 4: Bind preflight to selected feature dispatch

Extend `ValidatedCameraSession` with the selected profile and change simulation
and render traits so mutating methods cannot be called without it. Remove the
model-only write dispatch and all call sites.

**Verify**:
`build-gate -- cargo test --locked -p fujicli --lib preflight --jobs 4`
→ unknown/unverified/ambiguous firmware and wrong-operation profiles fail.

### Step 5: Make simulation fully firmware-scoped

Move parsing, validation, pulling, pushing, and rollback codecs to the selected
profile. Validate the complete candidate before selecting a slot. Update CLI
import/export envelope and docs as a deliberate breaking change.

**Verify**:
focused library and CLI tests prove 3.01 rejects Reala before any mutation,
4.00 synthetic profile accepts/encodes `0x14`, and descriptor policy cannot
widen the static allowlist.

### Step 6: Make RAW conversion fully firmware-scoped

Move layout and field codecs to selected profiles. Reorder the flow so exact
profile read/decode precedes `send_image`. Validate the complete `RenderBase`
before any upload.

**Verify**:
focused tests prove mismatched count, code, padding, order signature, or
firmware-specific option fails before `SendObjectInfo`, `SendObject`,
`SetDevicePropValue`, `0x900C`, or `0x900D`.

### Step 7: Migrate X-T5 production data conservatively

Preserve only the current exact 4.31 verified write profile in production FML.
Add 3.01 and 4.00 synthetic fixtures to tests, not production verification
claims. Add documented/unverified production entries only if they improve
diagnostics without making any write path selectable.

Do not claim that 4.00-4.30 share profile code, count, order, or padding until
exact captures exist. Never downgrade a user's camera to collect evidence; use
an already-versioned body or archived, provenance-checked capture.

**Verify**:
`mise exec cue@0.17.1 -- cue export ./fml --out json | jq ...`
→ X-T5 has exactly one verified write-capable production selector: 4.31.

### Step 8: Update documentation and support claims

Document logical/model/generation/firmware separation, capture requirements,
unknown firmware policy, new JSON envelope, and exact X-T5 table. Keep physical
verification claims separate from schema/fixture results.

**Verify**: documentation contains no claim that a fixture or official menu
note proves the PTP wire layout.

### Step 9: Run the full gate and inspect generated output

```sh
cargo fmt --all --check
build-gate -- cargo check --locked --all-features --all-targets --workspace --jobs 4
build-gate -- cargo clippy --locked --all-features --all-targets --workspace --jobs 4 -- -D warnings
build-gate -- cargo test --locked --all-features --all-targets --workspace --jobs 4
```

Expected: every command exits 0. Inspect generated `OUT_DIR` artifacts locally;
do not commit them.

## Test matrix

| Layer | Case | Expected result |
| --- | --- | --- |
| Firmware parser | `4.9`, `4.10`, `4.31` | numeric ordering; canonical round trip |
| Selector analysis | overlapping exact/range profiles | codegen error |
| Selector analysis | no matching / two matching profiles | runtime fail closed |
| Logical option | parse `reala-ace` | logical parse succeeds independent of camera |
| X-T5 synthetic 3.01 | simulation candidate Reala Ace | rejected before selector/property write |
| X-T5 synthetic 3.01 | Nostalgic Negative | accepted by static profile, then descriptor checked |
| X-T5 synthetic 4.00 | simulation Reala Ace | encodes `0x14` |
| X-T5 synthetic 4.00 | RAW profile Reala Ace | field encodes profile-specific `0x14` |
| Multi-wire fixture | one logical value, old/new canonical values | selected profile writes its own canonical value; reads only its own aliases |
| Descriptor narrower | static value absent from device enum | candidate rejected |
| Descriptor wider | device enum contains unknown value | static policy does not expose/accept it |
| Descriptor no-form | unsupported static enum candidate | rejected by static policy |
| Property change | datatype, writability, code, range, or step differs | preflight fails |
| RAW layout | wrong field count | rejected before image upload |
| RAW layout | wrong profile code | rejected before image upload |
| RAW layout | wrong padding/trailing bytes | rejected before image upload |
| RAW layout | same fields in wrong order signature | rejected before image upload |
| Firmware 4.31 | current exact profile | existing simulation/render fixtures remain green |
| Firmware 4.32 | no production profile | all state-changing commands fail closed |
| X-S20 pre-Reala fixture | global logical Reala exists | firmware profile rejects it; no generation fallback |
| CLI import | envelope profile differs from connected profile | explicit compatibility error |
| CLI help/input | unsupported logical value parses before connection | post-preflight capability error names model and firmware |

## Done criteria

- [ ] Global option types contain no wire repr, property code, or canonical
  outgoing encoding.
- [ ] Every mutation is dispatched through the exact profile retained by
  `ValidatedCameraSession`.
- [ ] The effective static capability is never wider than the selected firmware
  profile, even when a descriptor advertises more.
- [ ] A simulation candidate is fully validated before the custom-setting
  selector or any setting is written.
- [ ] A RAW profile is read and exactly decoded before the image is uploaded.
- [ ] Pre-4.00 and 4.00+ Reala regression tests pass for simulation and RAW
  conversion.
- [ ] A synthetic multi-wire-value test proves per-firmware canonical writes.
- [ ] Unknown/new firmware has no normal CLI override and fails closed.
- [ ] Production X-T5 write support remains exact 4.31 unless new physical
  capture evidence was explicitly supplied and reviewed.
- [ ] All full-gate commands exit 0.
- [ ] No generated `OUT_DIR` files or unrelated workspace changes are committed.

## STOP conditions

Stop and report instead of improvising if:

- a claimed X-T5 firmware profile lacks exact PTP identity, `GetDeviceInfo`, all
  relevant `GetDevicePropDesc`, and RAW-profile capture evidence;
- a proposed version range is based only on absence of public changelog entries;
- the camera reports a firmware format that cannot be parsed without guessing;
- the same firmware matches more than one effective profile;
- firmware-specific logical field shapes cannot be represented without changing
  the public simulation JSON contract (make the break explicit; do not add a
  silent compatibility path);
- validating RAW layout would still require `send_image` or another mutation
  before the mismatch can be detected;
- implementation would require adding a new production dependency without the
  user's explicit approval and maintenance/security/license review;
- physical evidence would require downgrading or otherwise risking the user's
  only camera.

## Maintenance notes

- Every firmware release must be triaged as a new compatibility identity. Copy a
  profile only after comparing device info, descriptors, simulation enum/ranges,
  USB modes/operations, and RAW profile signature.
- Store capture provenance next to the profile evidence; public release notes
  establish user-visible features but not private PTP layout.
- Reviewers should reject changes that add a firmware selector without tests for
  no-match, overlap, dynamic-descriptor intersection, and all affected write
  codecs.
- Keep generation templates as authoring conveniences only. Runtime support is
  always the result of exact model plus selected firmware plus operation.

## Primary references

- Fujifilm X-T5 firmware history:
  https://www.fujifilm-x.com/en-ie/support/download/firmware/cameras/x-t5/
- Fujifilm X RAW Studio history:
  https://www.fujifilm-x.com/global/?page_id=81781&post_type=supportsoftware&preview_id=81781
