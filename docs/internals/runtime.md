# Runtime

The runtime is the small Rust crate that mounts the generated module from
Cargo's build `OUT_DIR` and dispatches user requests through trait objects.
There is deliberately not much here; the heavy lifting is at build time.

## The Trait Hierarchy

`CameraBase` is the root. Every camera struct implements it; feature
participation is signalled by overriding `as_<feature>_*` methods that default
to `None`.

```rust
pub trait CameraBase {
    type Context: rusb::UsbContext;

    fn camera_definition(&self) -> &'static SupportedCamera;
    fn chunk_size_ceiling(&self) -> usize { 1024 * 1024 }

    fn as_backup_manager(&self) -> Option<&dyn CameraBackupManager<...>> { None }
    fn as_simulation_parser(&self) -> Option<&dyn CameraSimulationParser> { None }
    fn as_simulation_manager(&self) -> Option<&dyn CameraSimulationManager<...>> { None }
    fn as_render_manager(&self) -> Option<&dyn CameraRenderManager<...>> { None }

    fn get_info(&self, ptp: &mut Ptp) -> anyhow::Result<Box<dyn CameraInfo>> { ... }
}
```

The codegen emits overrides only for the features the camera spec declares:

```rust
// $OUT_DIR/generated/cameras.rs (sketch)
impl CameraBase for XT5 {
    type Context = rusb::GlobalContext;
    fn camera_definition(&self) -> &'static SupportedCamera { &C_X_T5 }
    fn chunk_size_ceiling(&self) -> usize { 16128 * 1024 }

    fn as_backup_manager(&self) -> Option<&dyn CameraBackupManager<...>> { Some(self) }
    fn as_simulation_parser(&self)  -> Option<&dyn CameraSimulationParser>  { Some(self) }
    fn as_simulation_manager(&self) -> Option<&dyn CameraSimulationManager<...>> { Some(self) }
    fn as_render_manager(&self)     -> Option<&dyn CameraRenderManager<...>>     { Some(self) }
}
```

If a camera doesn't declare a feature, the override is omitted, the default
`None` is returned, and the runtime's high-level method errors with a friendly
message.

## PTP Bulk Chunk Policy

The generated camera value is a ceiling, never the initial allocation. Runtime
reads the negotiated USB speed and both bulk endpoint packet sizes, then starts
with a packet-aligned 256 KiB chunk at low/full or unknown speed, and 1 MiB only
when the backend reports a faster link.

Read and write policy are independent. Writes remain at the conservative size
for the entire session. Three successful, naturally occurring large read-only
transactions may promote the next read transaction through 1, 4, 8, and 16 MiB
tiers, capped by the camera ceiling; the schema limits that ceiling to the
runtime's 16 MiB bulk read window, so promotion always terminates. The
measured aggregate rate must also imply
that the candidate chunk can fill within the bulk I/O timeout. Promotion records
the sampled bytes and duration; it never repeats a command to manufacture a
measurement.

Any transport failure keeps the existing fail-closed behavior: the in-flight
transaction is not replayed with a smaller chunk and an ambiguous session is
poisoned. Allocation failure before a promoted transaction keeps the previous
read size, and resizing preserves the terminating-ZLP boundary state across
transactions. Debug startup diagnostics record the OS, runtime libusb version,
negotiated speed, endpoint packet sizes, and initial/ceiling sizes. One trace
summary records the effective sizes and duration for each logical transaction;
payload bytes and camera identifiers are never logged.

Claiming is deliberately plain: `claim_interface` followed by the alternate
setting, with no `set_auto_detach_kernel_driver`. The still-image class has no
kernel driver bound in practice, and the Linux conflicts that do occur come
from userspace clients holding the interface, which a detach would not fix.

## Interrupt Latch

A bulk transfer abandoned mid-transaction wedges the camera's USB pipe; the
X-T5 audit needed a physical cable reconnection after one aborted `GetObject`.
The transport therefore owns the interrupt latch (`fujicli::interrupt`):
every `Ptp` transaction holds a `TransactionGuard`, and the CLI's Ctrl-C
handler records an interrupt instead of exiting whenever a guard is alive.
Once the transaction completes the transport drains the recorded interrupt
and returns an `Interrupted` error, so the caller unwinds and `Camera::drop`
closes the session normally; `main` maps that marker to exit status 130.

A `CriticalRegionGuard`, held by the CLI's `critical_camera_write`, keeps the
recorded interrupt pending across all the transactions of one camera write.
The transport does not unwind inside the region; the region owner drains the
count afterwards and reports the unknown-state outcome. Session-control
commands never convert a recorded interrupt: they complete regardless, and
`Drop` should not log a spurious close failure.

`send_mutating` also sets a sticky `camera_write_sent` flag on the latch
before dispatching. Every interrupt honoured after that point, in the
transport or in the signal handler between transactions, is reported as an
unknown camera state (exit status 3), so the verification window after a
restore cannot be mistaken for a clean interruption.

Simulation writes run inside `with_restored_simulation_selector`, the write
counterpart of the read scope below: the slot selector is snapshotted before
the target slot is selected and restored and verified after the transaction,
whatever its outcome, so `simulation set` and `simulation import` leave the
camera on the slot it had before. A restore that fails after a successful
write is reported as a `SelectorRestore`-phase `SimulationTransactionError`
with `CameraStateUnknown`; a restore that fails after a failed write
escalates that error's state to unknown and attaches the restore error.

The temporary-selector scope (`with_temporary_simulation_selector`) is itself
a critical region. `simulation list`, `get`, and `export` write the `0xD18C`
slot selector before every property read, so the scope holds a
`CriticalRegionGuard` from the selector snapshot through the restore and its
verification, then honours any recorded interrupt. Regions nest through a
depth counter. Selector writes under a `SimulationAccess` permit do not set
`camera_write_sent`: the scope restores and verifies them, so an interrupt
honoured after it exits 130, not 3. A forced quit inside the scope still
reports the unknown-state exit status, because the restore did not run.

## Top-Level Dispatch

[`src/lib/mod.rs`](../../src/lib/mod.rs) wraps the trait dispatch:

```rust
impl Camera {
    pub fn export_backup(
        &mut self,
        purpose: BackupPurpose,
    ) -> anyhow::Result<BackupArtifact> {
        let identity = self.backup_identity()?;
        if let Some(backups) = self.r#impl.as_backup_manager() {
            let payload = backups.export_backup(&mut self.ptp)?;
            BackupArtifact::create(purpose, identity, &payload)
        } else {
            bail!(ERROR_CAMERA_DOES_NOT_SUPPORT_BACKUP_MANAGEMENT);
        }
    }
    // similar for simulations, render, ...
}
```

The CLI doesn't see the per-camera types at all; it always goes through
`Camera`. Adding a new camera to the schema automatically extends the dispatch
surface; nothing in `src/cli/` needs to know.

## Physical and Logical Camera Identity

`Camera` records the physical USB VID:PID read from the connected descriptor
separately from the logical generated implementation. Native mode binds both to
the same supported registry entry. Emulation may choose a different logical
entry, but it cannot replace the physical identity or open an unsupported USB
Image/PTP device selected by bus and address.

Every high-level operation is classified before file or USB I/O and is checked
again at the `Camera` boundary. Emulated read-only access is allowed; transient
selector writes, persistent settings writes, opaque restore, and
destructive/recovery-sensitive operations are denied.
`reverse-tools` adds narrow read-only library probes consumed only by the
unpublished `fujicli-dev` package. The production parser contains no reverse
command, and the feature does not expose raw PTP or a restore bypass.

## State-Changing Preflight

Backup restore, simulation selector access/writes, and RAW conversion start in
one centralized preflight. `Camera::preflight_*` validates the physical USB
VID/PID, the exact PTP manufacturer/model/serial/firmware returned by
`GetDeviceInfo`, the advertised operation and property lists, the current USB
mode and battery level, and the firmware-specific FML profile. Unknown or
unverified firmware fails closed; there is deliberately no experimental
override for a normal binary.

Preflight also sends `GetDevicePropDesc` for every required property. Its parser
supports scalar and array PTP datatypes plus no-form, range, and enum
constraints. The descriptor's property code, datatype, current/default values,
and writability must be internally consistent with the generated policy. Every
subsequent property write is checked dynamically against that descriptor before
the command is sent, and only properties the profile declares `writable: true`
may be written at all: a descriptor the camera reports as writable never
widens the permit beyond what the profile asked for.

A physical X-T5 on firmware 4.31 in USB mode 0x6 answers `GeneralError` to
every `GetDevicePropDesc` while serving `GetDevicePropValue`; the firmware
image shows why: the DeviceInfo for that mode is assembled at run time and the
static descriptor table has no rows for the simulation settings (see the
[firmware analysis](x-t5-firmware-4.31-static-analysis-2026-09-03.md)). Only
that specific `GeneralError` (0x2002) counts as this documented, structural
refusal; preflight reads the value instead only for it. Any other PTP
response code on the descriptor read — a transient `DeviceBusy`, an
`AccessDenied`, an unsupported-property `DevicePropNotSupported`, or anything
else the camera answers — fails preflight outright and never reaches the
fallback, so it cannot silently widen write authority. Two outcomes follow
for the documented refusal:

- A requirement without a `static_descriptor` is checked only for wire shape
  against the declared datatype (scalar width or PTP string framing). That is
  enough for read-only requirements such as the USB mode and battery
  properties, but it never enters the mutation permit: such a property cannot
  be written, and a requirement declared `writable` or array-typed fails
  closed, naming the FML declaration that would resolve it.
- A requirement whose FML profile declares a `static_descriptor` yields a
  writable descriptor built from the pinned datatype, the declared form, and the
  live value. The live value must decode exactly and satisfy the form, or
  preflight fails with the declaration's `evidence` in the error. The permit
  then validates every write candidate against that descriptor exactly as it
  would against a camera-served one. Static descriptors are scalar or string
  only, and codegen rejects one whose datatype disagrees with the option codec.

Transport and decoding failures are not treated as a refusal.

Success returns `ValidatedCameraSession<Operation>`. Only that typestate exposes
the relevant mutation. It privately owns a non-cloneable mutation permit bound
to that operation, the exact USB device and interface, the active PTP session,
the live property descriptors, and the generated firmware profile. Dropping the
validated session invalidates that permit. The PTP transport does not retain
ambient mutation authority that another caller could reuse. Simulation enum
codecs use the selected profile's canonical/read wire mappings. Simulation and
render inputs are checked against its logical-value allowlists before selector
or object-upload mutations.

RAW conversion has an additional evidence gate. Each exact firmware profile may
carry a direction-specific descriptor with an evidence status, manifests, USB
mode binding, exact profile-code text, declared count, total length, padding,
and ordered read/write fields. `write_verified` is reserved but rejected by
codegen until manifests/hashes, live camera state, and lossless opaque bytes are
machine-checked. Runtime also derives the read fingerprint from the successfully
decoded live bytes before retaining it in transport authorization; static codec
constants plus a matching length cannot set that flag. X-T5 4.31 is currently
`unverified`, so preflight fails before the first mutation.

The `reverse-tools` render-profile capture is deliberately read-only and emits
a lossless JSON payload artifact without inferring padding or field semantics.
Golden payloads plus exact model/firmware/state HIL evidence and the missing
machine validators are required before `write_verified` may be accepted.

The transport exhaustively classifies every known PTP command. Generic send
methods accept only read-only commands; session lifecycle commands require a
private session-control token, while property writes, object upload/delete, and
Fuji vendor writes require the operation-bound mutation permit. Adding a new
`CommandCode` without classifying it therefore fails compilation. This makes
bypassing preflight from another high-level caller both a compile-time and a
transport-boundary failure rather than a CLI convention.

Dangerous operations also bind to the SHA-256 fingerprint of the live PTP
serial. Backup restore uses the source artifact's binding unless an explicit
target is supplied; simulation write and RAW conversion require an explicit
`--target-serial-sha256`.

## Public API Boundary

The public library surface contains `Camera`, data and outcome types, and the
validated high-level operations. USB transport, PTP command/property types,
camera implementation traits, and feature-manager SPI remain crate-private.
Generated implementations receive narrow authorized adapters instead of a raw
`Ptp`, and those adapters verify that their permit matches the requested
high-level operation before every mutation.

`Camera`, preflight, the PTP transport, and the capability constructor share a
private ownership module. Mutation and session-lifecycle methods are visible
only inside that subtree; other runtime, generated, and CLI modules cannot mint
the linear `AuthorizedPtp` capability. New modules must not be placed in this
trusted subtree merely to gain transport access, and the capability must not
gain `Clone`, `Deref`, raw getters, or a wider constructor.

There is deliberately no `raw-ptp` Cargo feature and no Rust `unsafe` marker for
operational risk. A feature would make the bypass available in the distributed
library, while `unsafe` would incorrectly describe camera-state risk as memory
safety. `reverse-tools` remains a CLI-only collection of named read-only probes;
it does not expose a generic command sender.

## Feature Traits

### `CameraBackupManager`

[`features/backup/manager.rs`](../../src/lib/features/backup/manager.rs)
provides default implementations of `export_backup` and `import_backup` against
`Ptp`; the underlying PTP exchange has been uniform across the cameras we've
seen, so this is a blanket
`impl<T> CameraBackupManager for T where T: CameraBase`. A camera opts in by
overriding `as_backup_manager` to return `Some(self)`.

The manager owns the opaque Fuji wire exchange only. The guarded public boundary
is [`features/backup/artifact.rs`](../../src/lib/features/backup/artifact.rs): a
versioned envelope with a strict JSON manifest, exact EOF framing, source
schema/PTP identity, purpose, bounded payload length, and SHA-256 fingerprints.
Main import accepts only a parsed artifact. Raw bytes can reach
`SendObjectInfo` only through the explicitly named unchecked reverse helper.

The artifact is an integrity and compatibility envelope, not a vendor-authentic
container: the native payload remains opaque and unsigned. Destructive import
therefore also requires an independently trusted complete-artifact SHA-256. A
manifest and digest supplied together by an attacker remain forgeable.

Export validates that `GetObjectInfo` describes `FujiBackup`, ends either
immediately after the `ObjectInfo` (as a physical X-T5 on firmware 4.31 answers)
or after the 1020 zero padding bytes seen in the original capture, and reports
the exact subsequent payload length. Import
checks live identity, capabilities, firmware policy, USB mode, battery,
property descriptors, and serial binding before the CLI durably creates a
no-clobber recovery artifact. Transport or framing failures poison the PTP
session; backup import classifies both metadata and data failures as unknown
camera state and never retries automatically.

The current X-T5 exchange uses zero object handles for `SendObjectInfo` and
`SendObject`, matching the existing reverse-engineered sequence. The transport
does not retain response parameters, so the runtime cannot validate a
camera-returned object handle. Changing this sequence requires a captured X-T5
exchange and physical-device validation rather than an inferred protocol fix.

There is no claimed rollback, `CancelTransaction`, `GetDeviceStatus`, device
reset, or byte-for-byte post-import verification. Those require captured X-T5
behavior and physical fault-injection evidence; a successful import currently
means only that the final PTP response accepted the restore operation.

### `CameraSimulationParser` + `Simulation`

[`features/simulation/`](../../src/lib/features/simulation/).
`CameraSimulationParser` is JSON serialize/deserialize (so users can round-trip
a slot to disk). `Simulation` is the trait object the parser yields: `try_pull`,
`try_update_from`, `name`, `to_base`. Codegen implements both for every
simulation-capable camera. Full-profile writes are deliberately not part of
this public trait; all writes cross the transaction boundary below.

`SimulationListItem` is a small helper struct used by `simulation
list`.

### `CameraSimulationManager`

The PTP-talking interface: `custom_settings_slots`, `get_simulation`,
`get_simulations`, `update_simulation`, `set_simulation`. Reads are classified
as `ReadWithTemporaryMutation`: codegen snapshots the raw custom-setting
selector, selects and verifies the requested slot around every property read,
then restores and verifies the exact raw snapshot. The batch API used by
`simulation list` performs one outer snapshot/restore around the complete slot
sequence and returns no partial list on failure.

Risk classification is separate from emulation availability. The production
CLI still rejects emulation for these reads because validated mutation
preflight requires a native physical model binding; it must not offer an
acknowledgement that can never reach an authorized camera session.

`update_simulation` and `set_simulation` snapshot the selected profile, build a
complete candidate, and pass both to the runtime transaction executor. The
generated `SimulationTransactionProfile` adapter contributes the camera's
dependency-safe property order and typed per-property read/write dispatch. The
executor writes only changed `Some` properties and journals a property only
after its framed PTP response succeeds.

A selected-slot I/O adapter reselects and verifies the target custom-setting
slot before and after every profile property read or write, including apply
verification and reverse rollback. If a confirmed property write is followed
by selector drift, that write remains journaled but the outcome is permanently
`CameraStateUnknown`; recovery cannot be overstated as verified. Selector and
property access are still separate PTP commands, so proving that an
away-and-back UI switch cannot occur wholly between them requires physical
device evidence for the active USB mode.

Every successful apply is followed by a complete, presence-aware readback. A
healthy apply failure or readback mismatch rolls back only confirmed writes,
in reverse order, and then reads the profile again. The typed outcomes are
`AppliedAndVerified`, `NoChangeVerified`, `RejectedWithoutChange`,
`RollbackVerified`, and `CameraStateUnknown`. A rollback is never reported as
verified without an exact readback of the original profile. If an original
field was absent, its latent wire value is unknown; a failure after changing
that property therefore remains `CameraStateUnknown` even after best-effort
recovery of other journal entries.

Transport failures, USB timeouts/disconnects, malformed containers, and PTP
stream mismatches poison the session. Once poisoned, the transaction executor
does not attempt rollback, readback, retry, or session reopening. A fully
framed PTP rejection, including `DeviceBusy`, leaves the session healthy, so
verified recovery remains possible.

### Fuji upload opcodes in USB mode 0x6

Mode 0x6 advertises `0x900C`, `0x900D`, and `0x901D` alongside the standard
object operations. These are Fujifilm's own file-upload family, not the
coincidentally same-numbered Leica/Sigma opcodes libgphoto2 defines:
petabyt/libfuji names them `SendObjectInfo` (create file), `SendObject2`
(write to file), and `SendObject` (apparently equivalent to `0x900D`), first
seen in Fuji's FPUPDATE.DAT update utility. This runtime uses the
`0x900C`/`0x900D` pair for RAW upload, matching its captures, and does not send
`0x901D`. See
[fuji-ptp-ecosystem-research](fuji-ptp-ecosystem-research.md) for sources.

### Object stores in USB mode 0x6

In USB RAW CONV./BACKUP RESTORE mode the X-T5 does not expose the memory card.
`GetStorageIDs` returns two virtual stores, `0x10000001` ("Still") and
`0x10000002` ("Live"), both reported as fixed RAM with generic hierarchical
filesystem, read-only-with-deletion access, unknown capacity, and no objects at
rest. Uploaded RAFs and rendered JPEGs live there, which is why render and
`image recover` handles refer to camera-side objects rather than card files, and
why a handle that no longer exists answers `InvalidObjectHandle`. The card
itself is only visible in USB CARD READER mode, where none of the Fujifilm
vendor properties exist.

### `CameraRenderManager`

[`features/render/manager.rs`](../../src/lib/features/render/manager.rs)
provides the upload, object-discovery, validated fetch, explicit recovery, and
cleanup primitives shared by render-capable cameras. Codegen snapshots the raw
conversion profile, builds the candidate, validates and uploads an X-T5 RAF,
writes and verifies the candidate, triggers the render, then restores and
verifies the original raw profile. A render failure and a restore failure are
both retained in the typed outcome.

Object discovery takes a pre-trigger handle baseline and requires two identical
non-empty delta polls. Every stable candidate is inspected with
`GetObjectInfo`; exactly one EXIF/JPEG object may be fetched. Its reported size,
fetched byte count, JPEG frame/scan structure, and terminal EOI are verified.
No fetch or validation path deletes an object. Discovery/access errors retain
all observed handles so the CLI can print an actionable `image recover`
command.

Local output is opened before camera mutation. A path output is written,
synced, and atomically committed before `DeleteObject`; stdout and explicit
recovery retain the camera object by default. Recovery fetch and recovery
cleanup use separate least-privilege preflight profiles, so fetching retained
bytes does not depend on writable conversion-profile properties. Because the
cleanup session is not the one that fetched the object, cleanup re-reads
`GetObjectInfo` and refuses any handle that is not an EXIF/JPEG before it
sends `DeleteObject`, then proves the deletion through `GetObjectHandles`.
The CLI runs that whole cleanup inside the interrupt critical region, so a
Ctrl-C cannot separate the deletion from its verification and is reported as
an unknown camera state if it arrives meanwhile.

Render-object polling has a five-minute absolute deadline. Each
`GetObjectHandles` exchange uses that same deadline instead of starting a fresh
five-minute transaction budget, so an empty object list or a peer that only
makes partial progress cannot keep the CLI alive past the polling deadline.

### X-T5 physical acceptance matrix

Local fault injection proves control flow, not camera semantics. Before calling
the selector/profile lifecycle physically verified on a new firmware, run this
matrix with privacy-reviewed `-vvv` traces and record the pre/post camera UI
state and object handles:

| Mode | Operation | Inject/interrupt after | Required observation |
| --- | --- | --- | --- |
| Still and movie | `simulation get` / `export` | selector write; each property read | Original D18C raw value and visible slot restored, or explicit unknown-state failure |
| Still and movie | `simulation list` | each C1-C7 selection | One outer restore; no partial CLI output; correct namespace identified |
| Still and movie | selector lifecycle | disconnect/reconnect and power cycle | Determine whether D18C is active/persistent state rather than assuming query-only context |
| Still | `image render` | upload, profile write/readback, trigger, every handle poll, `GetObjectInfo`, fetch | Original D185 bytes restored/read back; every observed new handle retained on failure |
| Still | `backup import` | zero-byte USB timeout during payload; final response delayed beyond 10 seconds; disconnect after full payload | Same transaction resumes at the confirmed offset; no restore replay; delayed success is accepted; disconnect reports unknown state and skips close |
| Still | `image render` | repeated `DeviceBusy`; processing beyond 10 seconds; five-minute poll expiry | Bounded increasing poll backoff; delayed result is fetched once; expiry retains handles and sends neither a second trigger nor `CloseSession` |
| Still | local output | write, sync, rename | No `DeleteObject` until the final path is durable |
| Still | `image recover` | fetch and optional cleanup | Default retains object; explicit cleanup occurs only after durable save and cleanup preflight |

Repeat render rows for success, framed PTP rejection, timeout, disconnect, and
malformed/truncated response. Verify the saved JPEG with an independent decoder
before accepting the structural in-process validator as sufficient for the
specific X-T5 firmware.

For the cross-state semantics that `try_update_from` enables, see
[fml/rules / cross-state rules](../fml/rules.md#cross-state-rules).

## PTP and Option Traits

[`src/lib/ptp/option.rs`](../../src/lib/ptp/option.rs) declares the two
option-level traits the codegen targets:

- **`SimulationSetting`**: for options with a `prop_code`. Carries
  `prop_code() -> u16` plus `try_push` / `try_pull` defaulting to
  `Ptp::set_prop` / `get_prop`.
- **`ConversionProfileField`**: for every option (including inline-only ones).
  Reads/writes the 32-bit lifted form used inside render profiles.

The PTP wire boundary lives in
[`src/lib/ptp/codec.rs`](../../src/lib/ptp/codec.rs). `binrw` owns primitive,
struct, and enum mechanics; the local module retains PTP UTF-16 strings,
u32-counted arrays, their allocation limit, and exact-buffer validation.

PTP timing is split into independent limits:

- a ten-second USB transfer slice bounds one `rusb` call;
- command and data phases allow thirty seconds without confirmed byte progress;
- ordinary responses allow thirty seconds, camera-processing responses five
  minutes, and post-large-transfer responses ten minutes;
- ordinary/camera-processing transactions have a five-minute hard cap, while
  backup/RAF/JPEG transfers have a fifteen-minute hard cap;
- render handle polling has its own five-minute absolute deadline and a
  `DeviceBusy` backoff from 100 ms up to one second.

The larger limits are conservative bootstrap values for the bounded 512 MiB
input and the X-T5's roughly 16 MiB transfer chunk, not measured throughput or
firmware guarantees. They must be calibrated with the physical acceptance
matrix rather than copied to another camera as model facts.

A zero-byte USB timeout does not by itself end a transaction. The transport
continues the same bulk read or write, at the same confirmed offset, until the
phase or hard deadline; confirmed progress refreshes only the data idle
deadline. It never replays a PTP write command or a completed data prefix.
Expiry after dispatch, disconnect, malformed framing, or transaction/operation
mismatch poisons the session because the wire position or camera state is then
ambiguous. Further commands fail without USB I/O, and `Camera::drop` skips
`CloseSession`. RAW conversion also suppresses automatic `CloseSession` while a
successfully triggered camera-side conversion has not produced stable object
handles. A fully framed non-OK response does not poison the session;
`DeviceBusy` is retried only by the bounded read-only render polling loop.

## Input Helpers

[`src/lib/input/`](../../src/lib/input/) provides:

- **`CleanAlphanumeric`**: strips whitespace/punctuation and lower-cases a
  string. Used at the start of every option's `FromStr` so variations all
  collide into the same key.
- **`Choices`**: Levenshtein-based closest-match suggestions used in `FromStr`
  error messages.

## Adding a Runtime Feature

If you want `fujicli` to support a brand-new feature (not just adding a camera
to an existing one):

1. Add the data model to `fml/camera.cue` under `#Features`.
2. Add the AST in `crates/codegen/src/ast/cameras.rs` (mirror the structure of
   `Simulation` / `Render`).
3. Add a feature trait in `src/lib/features/<name>/`, with sensible default
   impls if possible, like `CameraBackupManager`.
4. Add `as_<name>(&self)` to `CameraBase`.
5. Add an emitter in `crates/codegen/src/common/<name>/`.
6. Wire dispatch through `Camera` in `src/lib/mod.rs`.
7. If the feature has CLI surface, generate `<Feature>Args` in
   `crates/codegen/src/cli/<name>.rs` and consume it from the appropriate
   `src/cli/` module.

The grammar / DNF / presence / repair machinery is feature-agnostic and can be
reused if your feature has its own typed setting list and validation rules.
