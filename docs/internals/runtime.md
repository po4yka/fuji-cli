# Runtime

The runtime is the small Rust crate that mounts the generated module from
Cargo's build `OUT_DIR` and dispatches user requests through trait objects.
There is deliberately not much here; the heavy lifting is at build time.

## Physical and logical camera identity

`Camera` retains two distinct identities. `PhysicalUsbIdentity` always comes
from the connected USB descriptor. `LogicalCameraIdentity` identifies the
generated implementation selected for dispatch. Native mode makes them match;
emulation may select a different logical model but never replaces the physical
identity. Both the physical and emulated vendor/product pairs must exist in the
generated `SUPPORTED` registry.

Before CLI I/O and again before high-level library PTP operations, the
`CommandRisk` policy authorizes the binding. Read-only device info is allowed;
simulation reads require acknowledgement because selecting a slot is a
transient write; persistent settings, opaque restore, and render operations are
denied. The raw `Ptp` inside `Camera` is private. A deliberate `reverse-tools`
Cargo feature exposes the reverse-only escape hatch and is absent from default
builds.

## The Trait Hierarchy

`CameraBase` is the root. Every camera struct implements it; feature
participation is signalled by overriding `as_<feature>_*` methods that default
to `None`.

```rust
pub trait CameraBase {
    type Context: rusb::UsbContext;

    fn camera_definition(&self) -> &'static SupportedCamera;
    fn chunk_size(&self) -> usize { 1024 * 1024 }

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
    fn chunk_size(&self) -> usize { 16128 * 1024 }

    fn as_backup_manager(&self) -> Option<&dyn CameraBackupManager<...>> { Some(self) }
    fn as_simulation_parser(&self)  -> Option<&dyn CameraSimulationParser>  { Some(self) }
    fn as_simulation_manager(&self) -> Option<&dyn CameraSimulationManager<...>> { Some(self) }
    fn as_render_manager(&self)     -> Option<&dyn CameraRenderManager<...>>     { Some(self) }
}
```

If a camera doesn't declare a feature, the override is omitted, the default
`None` is returned, and the runtime's high-level method errors with a friendly
message.

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

Export validates that `GetObjectInfo` describes `FujiBackup`, has the expected
1020-byte zero padding, and reports the exact subsequent payload length. Import
checks live model and firmware plus serial policy before the CLI durably creates
a no-clobber recovery artifact. Transport or framing failures poison the PTP
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
`try_push`, `try_update_from`, `name`, `to_base`. Codegen implements both for
every simulation-capable camera.

`SimulationListItem` is a small helper struct used by `simulation
list`.

### `CameraSimulationManager`

The PTP-talking interface: `custom_settings_slots`, `get_simulation`,
`update_simulation`, `set_simulation`. The codegen-emitted impl uses the
option's `SimulationSetting::try_push` to select the slot before
reading/writing.

### `CameraRenderManager`

[`features/render/manager.rs`](../../src/lib/features/render/manager.rs)
provides default `send_image` / `render_image` (uniform across cameras). Each
render-capable camera must override `render`, which the codegen does: send the
image, fetch the current `<Camera>RenderProfile`, `try_update_from(partial)`,
write back, then `render_image`.

Render-object polling has a five-minute absolute deadline. Each
`GetObjectHandles` exchange uses that same deadline instead of starting a fresh
five-minute transaction budget, so an empty object list or a peer that only
makes partial progress cannot keep the CLI alive past the polling deadline.

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

Every PTP transaction uses one absolute deadline across all of its bulk reads
and writes, capped at five minutes and shortened when its caller has an earlier
deadline. The per-transfer USB timeout is capped at ten seconds or the remaining
transaction time. A transport failure, malformed container, or
transaction/operation mismatch poisons the session because the stream position
is then ambiguous. Further commands fail without USB I/O, and `Camera::drop`
skips `CloseSession`. A fully framed non-OK PTP response does not poison the
session and the next transaction may proceed.

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
