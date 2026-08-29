# Replacing the PTP codecs and derive macros with `binrw`

Research date: 2026-08-29

Implementation status: completed in the repository on 2026-08-29. The runtime,
transport, generated options, conversion-profile fields, render profiles, and
feature-specific codecs now use `binrw`; the former `ptp_cursor` and
`ptp_macro` workspace crates have been removed. Local validation is recorded in
the implementation handoff; physical-camera behavior remains a separate proof
boundary.

## Decision

Adopt `binrw`, but do not treat it as a one-for-one replacement for every line
in the current PTP crates.

The implemented final state is:

- use `binrw::BinRead` and `binrw::BinWrite` for primitive values, ordinary PTP
  structs, and unit enums;
- delete the `ptp_macro` crate entirely;
- delete the byte-order and primitive implementations in `ptp_cursor`;
- keep a small PTP-specific codec module for the protocol's length-prefixed
  UTF-16 strings, exact strings, arrays, exact-buffer validation, and contextual
  errors;
- keep generated/manual `BinRead` and `BinWrite` implementations where encoding
  is semantic rather than mechanical: alternate option values, `ContainerCode`,
  conversion-profile fields, and render profiles.

This removes the generic binary-format machinery that the project currently
maintains while preserving the camera-specific knowledge in FML/codegen and the
wire-specific knowledge at the PTP boundary.

Use the current release with the minimal feature set:

```toml
binrw = { version = "0.15.2", default-features = false, features = ["std"] }
```

As of this research, 0.15.2 is the latest release and declares Rust 1.70 as its
minimum version. Its normal dependencies are `array-init`, `binrw_derive`, and
`bytemuck`. [`binrw` is MIT-licensed](https://docs.rs/crate/binrw/0.15.2), while
those transitive packages offer MIT-compatible choices. The repository does not
currently contain a `deny.toml` or another recorded dependency-license policy,
so final license acceptance still belongs in dependency review rather than this
technical recommendation.

Disable the default `verbose-backtrace` feature. It adds `owo-colors` and the
prototype showed ANSI-rich, multi-line parser errors. A CLI should instead add
short, domain-specific context at the PTP call boundary.

## Why the replacement is viable

`binrw` directly covers the mechanical behavior implemented by the project's
derive crate:

- `#[derive(BinRead, BinWrite)]` reads and writes struct fields in declaration
  order;
- `#[brw(little)]` fixes PTP byte order;
- `#[brw(repr = u16)]` and signed equivalents encode C-style enums;
- `count`, `temp`, and `try_calc` describe count-prefixed collections;
- `pad_after` describes the fixed render/backup padding;
- `assert`, `try_map`, `parse_with`, and `write_with` cover validation and
  exceptional fields.

These are first-class supported directives, including custom parsers/writers
and positional errors, rather than incidental macro behavior. See the official
[`binrw` overview](https://docs.rs/binrw/latest/binrw/) and
[attribute reference](https://docs.rs/binrw/latest/binrw/docs/attribute/).

The trait boundary is also a fit. `BinRead` accepts `Read + Seek`, and `BinWrite`
accepts `Write + Seek`, with GAT-based `Args` for formats that need external
parameters ([`BinRead`](https://docs.rs/binrw/latest/binrw/trait.BinRead.html),
[`BinWrite`](https://docs.rs/binrw/latest/binrw/trait.BinWrite.html)). The
current `ptp_cursor::Read` trait is implemented only for `Cursor<T>`, so the
seek requirement does not exclude a reader currently supported by Fuji CLI.

The write path does need an explicit refactor: `Vec<u8>` is currently used as
an append-only writer, while `BinWrite` expects `Write + Seek`. Payload builders
should own a `Cursor<Vec<u8>>` and call `into_inner()` at the transport boundary.
Do not encode each field into a temporary allocation merely to append it.

## Pre-migration surface and blast radius

Before the migration, the hand-written codec surface consisted of:

- `crates/ptp/cursor`: 542 lines for
  primitive little-endian I/O, eight numeric array variants, two PTP string
  forms, exact-buffer validation, two project traits, and 22 tests;
- `crates/ptp/macro`: 227 lines of derive
  generation and 13 integration tests;
- [`src/lib/ptp`](../../src/lib/ptp): transport-facing uses and standard PTP
  structures;
- [`crates/codegen/src/common/options`](../../crates/codegen/src/common/options):
  generated derives for simple types and generated manual codecs for semantic
  lookup types;
- [`crates/codegen/src/common/renders/camera.rs`](../../crates/codegen/src/common/renders/camera.rs):
  generated render-profile serialization and deserialization.

The pre-migration generated snapshot contained 117 `PtpSerialize` and 116
`PtpDeserialize` references. Most are repetitions emitted by codegen, not
independent migration decisions. The migration should therefore change the
generators before judging the generated diff, and must never patch
the generated modules in Cargo's `OUT_DIR` directly.

`byteorder` was reachable only through `ptp_cursor` and `ptp_macro`, and was
removed with those crates. With `verbose-backtrace` disabled, `binrw` added five
external packages that were not previously in the lockfile:
`binrw`, `binrw_derive`, `array-init`, `bytemuck`, and `either`; the existing
`proc-macro2`, `quote`, `syn`, and `unicode-ident` packages are reused. Net of
removing `byteorder`, this is four additional external packages.

## Contract-by-contract mapping

| Current contract | `binrw` target | Residual project logic |
| --- | --- | --- |
| Numeric primitives | Built-in `BinRead`/`BinWrite` | None; invoke with little endian |
| Named and tuple structs | `derive(BinRead, BinWrite)` | Field annotations only where PTP differs |
| Unit structs | `derive(BinRead, BinWrite)` | Exact-buffer helper rejects trailing data |
| `#[repr(u16/i16)]` enums | `#[brw(repr = ...)]` | Add type context if an error reaches the CLI |
| PTP `String` | Custom field parser/writer or `PtpString` newtype | u8 length, UTF-16 units, NUL, 254-unit limit |
| `ExactString` | `PtpExactString` newtype | u8 length, no NUL, 255-unit limit |
| Numeric `Vec<T>` | `temp` count plus `count`, or `PtpArray<T>` | u32 count and 1,000,000-item read cap |
| Full-buffer decode | `decode_exact<T>(&[u8])` | Verify final cursor position |
| Streaming decode | `T::read_le(&mut cursor)` | Caller decides when the enclosing frame ends |
| Append serialization | `Cursor<Vec<u8>>` plus `write_le` | Transport owns cursor and extracts bytes once |
| `ContainerCode` | Manual `BinRead`/`BinWrite` | Resolve command-vs-response semantic enum |
| Lookup options | Generated manual `BinRead`/`BinWrite` | Accept alternates, write canonical raw value |
| Conversion-profile fields | Keep/rename the domain trait | Select lifted 32-bit representation |
| Render profiles | Generated manual `BinRead`/`BinWrite` | Validate header, read raw fields, topological conversion |
| Backup object info | Derived/manual `BinWrite` | Synthesize `ObjectInfo`, then fixed padding |

### PTP strings are not `NullWideString`

`binrw::NullWideString` reads UTF-16 values until a zero word and writes the
same terminator. Its implementation has no PTP u8 length prefix
([source](https://docs.rs/binrw/latest/src/binrw/strings.rs.html)). Using it
directly would therefore change the wire format and allow malformed input to
scan beyond the PTP field boundary.

Keep one small, tested implementation that:

1. reads the u8 unit count;
2. handles zero as the PTP empty-string representation;
3. reads exactly `len - 1` UTF-16 units;
4. consumes the final u16 terminator;
5. converts with `String::from_utf16`;
6. on write, counts UTF-16 code units rather than Unicode scalar values.

The migrated reader verifies that the final u16 is zero and reports malformed
device data as `InvalidData`. This intentional hardening is covered by a
dedicated regression test rather than being hidden inside the mechanical
migration.

### Arrays need their PTP count and safety cap

`binrw`'s `Vec` reader accepts a `count` argument, but it does not know that PTP
puts a u32 count before every supported numeric array. Encode that relationship
with a hidden temporary count field or a `PtpArray<T>` wrapper. Preserve the
existing 1,000,000-element read cap before allocation; a derive alone is not a
resource-exhaustion defense.

### Exact and streaming decode must stay separate

`BinRead` consumes one value and intentionally allows following bytes. Fuji CLI
also needs a message-level operation that rejects trailing bytes. Keep an exact
helper like this:

```rust
pub fn decode_exact<T>(bytes: &[u8]) -> binrw::BinResult<T>
where
    T: for<'a> binrw::BinRead<Args<'a> = ()>,
{
    let mut reader = std::io::Cursor::new(bytes);
    let value = T::read_le(&mut reader)?;
    if reader.position() != bytes.len() as u64 {
        return Err(binrw::Error::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "trailing bytes after PTP value",
        )));
    }
    Ok(value)
}
```

Use `decode_exact` for USB response bodies and property values. Use
`BinRead::read_le` only when reading a component from an already-bounded PTP
container.

## Target module shape

The final repository does not need another generic codec crate. Put the small
protocol residue in `src/lib/ptp/codec.rs`:

```text
src/lib/ptp/codec.rs
  encode<T>(&T) -> BinResult<Vec<u8>>
  decode_exact<T>(&[u8]) -> BinResult<T>
  PtpString parser/writer
  PtpExactString
  PtpArray<T> or array parser/writer
  contextual error helpers
```

This is a deeper boundary than preserving `ptp_cursor`: binary mechanics come
from `binrw`; only PTP rules stay local. Generated root-module types can refer
to `crate::ptp::codec`, so codegen does not need to link `binrw` itself merely
to emit tokens.

Ordinary wire types become declarative:

```rust
#[derive(Debug, binrw::BinRead, binrw::BinWrite)]
#[brw(little)]
pub struct ContainerInfo {
    pub length: u32,
    pub container_type: ContainerType,
    pub code: u16,
    pub transaction: u32,
}

#[derive(Debug, Clone, Copy, binrw::BinRead, binrw::BinWrite)]
#[brw(little, repr(u16))]
pub enum ContainerType {
    Command = 1,
    Data = 2,
    Response = 3,
    Event = 4,
}
```

Do not keep final compatibility traits named `PtpSerialize` and
`PtpDeserialize`. `SimulationSetting` should bind directly to argument-free
`BinRead`/`BinWrite`, and `Ptp::get_prop`/`set_prop` should call the codec
boundary. Internal compatibility adapters are acceptable only while a branch is
being migrated; none should remain in the final tree.

## Codegen-specific design

### Simple generated options

Scaled numeric newtypes can derive `BinRead` and `BinWrite`. Generated string
newtypes need field-level PTP string parsing/writing because Rust's orphan rules
prevent implementing `binrw` traits for `String`.

### Lookup options and alternate values

The lookup generators accept multiple raw values for one logical variant and
write only the canonical value. A `repr` derive cannot express that many-to-one
mapping. Continue generating implementations, but generate `BinRead` and
`BinWrite` instead of the project traits:

```rust
impl binrw::BinRead for GeneratedOption {
    type Args<'a> = ();

    fn read_options<R: Read + Seek>(
        reader: &mut R,
        endian: binrw::Endian,
        (): (),
    ) -> binrw::BinResult<Self> {
        let raw = Repr::read_options(reader, endian, ())?;
        Self::try_from_wire(raw).map_err(Into::into)
    }
}
```

The writer delegates the canonical raw value to `Repr::write_options`. This is
still generated codec code, but it is irreducible domain mapping rather than a
reimplementation of byte order, cursors, or derive machinery.

### Render profiles

Do not force render profiles into a giant attribute expression. Their decoder
does more than parse fields:

- validates the generated property count;
- parses and validates a camera profile code;
- consumes fixed padding;
- reads raw i32 fields in declaration order;
- converts them in a separately computed topological order;
- applies presence gates and inverses.

Keep a generated manual `BinRead`/`BinWrite` implementation or split it into a
small derived wire header plus a generated semantic conversion phase. The
second option is preferred because it lets `binrw` own the header and padding
without obscuring the conversion DAG.

The existing hard-coded `RENDER_HEADER_PADDING = 0x1EE` remains a separate FML
modeling issue. Moving it into the camera schema is desirable, but combining
that change with the codec migration would make byte-equivalence harder to
review.

## Error-model changes

The current public-internal codec methods return `std::io::Result`; `binrw`
returns `BinResult<T>` with position-aware variants such as `NoVariantMatch`,
`AssertFail`, `Custom`, and `Io`. Let `binrw::Error` propagate into `anyhow` at
application boundaries rather than flattening it immediately into
`io::Error`.

One observable difference needs a decision: the current enum derive test
requires the enum type name in an invalid-discriminant message, while a
`binrw` repr enum's root error reports an unexpected value and position without
the Rust type name. Add context such as `decoding ContainerType` in
`decode_exact` callers where that distinction matters, and assert structured
error classes or contextual messages rather than the upstream library's full
display text.

`binrw` may restore a seekable reader to the start of a derived parse after an
error. Current decoding generally abandons the cursor on error, so this is not a
functional regression. Do not rely on partial-consumption positions in new
tests.

## Implemented migration sequence

The implementation followed these layers and leaves no compatibility traits or
old workspace crates:

1. **Freeze the exercised wire contract.** Replace the old crate tests with
   protocol-level golden and round-trip tests for the codec boundary, standard
   PTP structs, container headers, command parameters, and backup object info.
   Generator tests cover the emitted `binrw` shape; alternate-value round trips
   and a complete generated render-profile byte fixture remain gaps listed
   below.
2. **Add `binrw` and the PTP codec boundary.** Add `encode`, `decode_exact`, PTP
   string/exact-string handling, arrays, and their limits. Cover exact decoding,
   excessive array counts, and the explicit string-terminator policy before
   migrating callers.
3. **Migrate standard PTP types.** Convert container types, `DeviceInfo`,
   `ObjectInfo`, and transport composition to `Cursor<Vec<u8>>`. Preserve exact
   byte fixtures.
4. **Migrate simple codegen.** Emit `binrw` derives for scaled and string option
   newtypes; update generated compile/token tests.
5. **Migrate semantic codegen.** Emit manual `BinRead`/`BinWrite` for lookup
   options, conversion-profile fields, and render profiles. Keep raw-read order
   distinct from semantic conversion order.
6. **Migrate feature-specific writers.** Convert backup padding and render
   request/response arrays.
7. **Delete the old crates.** Remove both workspace members, both dependencies,
   `byteorder`, old trait imports, and old documentation. Regenerate only through
   the normal Cargo/CUE build.
8. **Run the available local gates and record external gaps.** `Cargo.lock`,
   generated output, the dependency tree, focused codec tests, and workspace
   compilation are covered locally. A real-camera read/write smoke test still
   requires physical X-T5 access and was not substituted with local evidence;
   unrelated failures in the shared working snapshot also prevent claiming a
   wholly green repository gate.

## Required regression suite

The completion bar should include all of the following:

- primitive signed/unsigned little-endian golden bytes;
- named, tuple, and unit structs;
- signed and unsigned repr enums, including unknown discriminants;
- PTP empty string, BMP text, surrogate-pair text, maximum lengths, invalid
  UTF-16, truncation, and the terminator-policy decision;
- exact string with zero, 255, and 256 UTF-16 units;
- empty and populated arrays for all used numeric widths, excessive count,
  truncation, and write-side u32 overflow;
- exact decode rejecting trailing bytes and streaming decode consuming only one
  value;
- `ContainerCode` command/response resolution;
- every generated option representation family, including alternate raw values
  decoding to a logical value and re-encoding canonically;
- render header count/profile validation, fixed padding length, raw field order,
  topological conversion, gated fields, inverses, truncation, and trailing data;
- USB container request/response golden bytes and existing transport tests.

There is still no full generated render-profile golden-byte fixture. Generator
tests cover the manual `binrw` shape and preserve declaration-order raw reads
plus topological semantic conversion, but a captured complete profile remains
the largest local verification gap.

Other residual test gaps are an executable alternate-wire-value to canonical
re-encode round trip, focused `ContainerCode` command/response/unknown round
trips, and the complete string/array boundary and truncation matrix described
above. These gaps limit verification depth; they do not keep the removed codec
or derive implementations alive.

## Prototype evidence

An isolated edition-2024 crate was built outside the repository against exact
`binrw 0.15.2` with default features disabled. It tested:

- derived struct and u16 repr enum byte equivalence;
- unit structs;
- exact versus streaming decode;
- PTP UTF-16 strings and exact strings;
- a generic u32-prefixed array with the current safety limit;
- a declarative `temp`/`count` array;
- fixed `pad_after` behavior;
- alternate raw lookup values with canonical output;
- invalid enum and string inputs.

Observed command:

```text
build-gate -- cargo test --locked --jobs 4
running 8 tests
test result: ok. 8 passed; 0 failed
```

This prototype preceded the repository migration. The repository build now
also exercises CUE generation and compiles the emitted Rust; neither form of
local evidence is physical-camera proof.

## Risks and mitigations

| Risk | Consequence | Mitigation |
| --- | --- | --- |
| Treating `String` or `Vec` as directly compatible | Changed PTP wire bytes | Dedicated parser/writer and golden bytes |
| Dropping exact-buffer checks | Accepting malformed trailing data | Single `decode_exact` boundary |
| Allocating from untrusted u32 count | Memory exhaustion | Preserve cap before allocation |
| Flattening `binrw::Error` to `io::Error` too early | Lost offset and cause | Propagate `BinResult`, add `anyhow` context later |
| Using default verbose errors | Noisy ANSI CLI diagnostics and extra dependency | Disable defaults, enable only `std` |
| Replacing semantic generated codecs with `repr` derives | Alternate values or render gates break | Keep generated manual trait implementations |
| Editing generated Rust | Non-deterministic/stale change | Modify generators and rebuild through Cargo/CUE |
| Mixing protocol hardening with migration | Hard-to-review behavior drift | Byte-equivalent migration first, hardening second |
| Relying only on local tests | Vendor/device behavior remains unproved | Run a real-camera smoke test after full local gate |

## Final assessment

The migration replaces project-owned binary mechanics and derive maintenance
while intentionally retaining the manual lines that encode PTP and Fujifilm
semantics.

The success criterion is not “zero manual codecs.” It is:

- zero project-owned primitive/endian codec implementations;
- zero project-owned derive macros;
- one small, explicit PTP wire-boundary module;
- semantic generated codecs expressed through standard `binrw` traits;
- byte-for-byte equivalence proven before device testing.
